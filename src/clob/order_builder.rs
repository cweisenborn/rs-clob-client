use std::marker::PhantomData;
use std::time::{SystemTime, UNIX_EPOCH};

use alloy::primitives::{FixedBytes, U256};
use chrono::{DateTime, Utc};
use rand::RngExt as _;
use rust_decimal::prelude::ToPrimitive as _;

use crate::Result;
use crate::auth::Kind as AuthKind;
use crate::auth::state::Authenticated;
use crate::clob::Client;
use crate::clob::types::request::OrderBookSummaryRequest;
use crate::clob::types::{
    Amount, AmountInner, AnySignableOrder, Order, OrderType, OrderV2, OrderVersion, Side,
    SignableOrder, SignableOrderV2, SignatureType,
};
use crate::error::Error;
use crate::types::{Address, Decimal};

pub(crate) const USDC_DECIMALS: u32 = 6;

/// Maximum number of decimal places for `size`
pub(crate) const LOT_SIZE_SCALE: u32 = 2;

/// Placeholder type for compile-time checks on limit order builders
#[non_exhaustive]
#[derive(Debug)]
pub struct Limit;

/// Placeholder type for compile-time checks on market order builders
#[non_exhaustive]
#[derive(Debug)]
pub struct Market;

/// Used to create an order iteratively and ensure validity with respect to its order kind.
#[derive(Debug)]
pub struct OrderBuilder<OrderKind, K: AuthKind> {
    pub(crate) client: Client<Authenticated<K>>,
    pub(crate) signer: Address,
    pub(crate) signature_type: SignatureType,
    pub(crate) salt_generator: fn() -> u64,
    pub(crate) timestamp_ms_source: fn() -> i64,
    pub(crate) token_id: Option<U256>,
    pub(crate) price: Option<Decimal>,
    pub(crate) size: Option<Decimal>,
    pub(crate) amount: Option<Amount>,
    pub(crate) side: Option<Side>,
    pub(crate) nonce: Option<u64>,
    pub(crate) expiration: Option<DateTime<Utc>>,
    pub(crate) taker: Option<Address>,
    pub(crate) order_type: Option<OrderType>,
    pub(crate) post_only: Option<bool>,
    pub(crate) funder: Option<Address>,
    pub(crate) order_version: OrderVersion,
    pub(crate) metadata: FixedBytes<32>,
    pub(crate) builder_field: FixedBytes<32>,
    pub(crate) _kind: PhantomData<OrderKind>,
}

/// Convert a millisecond-timestamp closure into a [`U256`] suitable for
/// the `timestamp` field of an [`OrderV2`].  Extracted so it can be
/// unit-tested independently of the async network stack.
pub fn compute_timestamp(source: fn() -> i64) -> U256 {
    U256::from(u64::try_from(source()).expect("system clock is post-1970"))
}

impl<OrderKind, K: AuthKind> OrderBuilder<OrderKind, K> {
    /// Sets the `token_id` for this builder. This is a required field.
    #[must_use]
    pub fn token_id(mut self, token_id: U256) -> Self {
        self.token_id = Some(token_id);
        self
    }

    /// Sets the [`Side`] for this builder. This is a required field.
    #[must_use]
    pub fn side(mut self, side: Side) -> Self {
        self.side = Some(side);
        self
    }

    /// Sets the nonce for this builder.
    #[must_use]
    pub fn nonce(mut self, nonce: u64) -> Self {
        self.nonce = Some(nonce);
        self
    }

    #[must_use]
    pub fn expiration(mut self, expiration: DateTime<Utc>) -> Self {
        self.expiration = Some(expiration);
        self
    }

    #[must_use]
    pub fn taker(mut self, taker: Address) -> Self {
        self.taker = Some(taker);
        self
    }

    #[must_use]
    pub fn order_type(mut self, order_type: OrderType) -> Self {
        self.order_type = Some(order_type);
        self
    }

    /// Sets the `postOnly` flag for this builder.
    #[must_use]
    pub fn post_only(mut self, post_only: bool) -> Self {
        self.post_only = Some(post_only);
        self
    }

    /// Opt into V2 order signing for this builder. Defaults to V1.
    #[must_use]
    pub fn version(mut self, v: OrderVersion) -> Self {
        self.order_version = v;
        self
    }

    /// V2-only: set the `metadata` bytes32 field. Defaults to all zero.
    /// Silently ignored for V1 orders (V1 has no metadata field).
    #[must_use]
    pub fn metadata(mut self, m: FixedBytes<32>) -> Self {
        self.metadata = m;
        self
    }

    /// V2-only: set the `builder` bytes32 field. Defaults to all zero.
    /// Silently ignored for V1 orders.
    #[must_use]
    pub fn builder(mut self, b: FixedBytes<32>) -> Self {
        self.builder_field = b;
        self
    }
}

impl<K: AuthKind> OrderBuilder<Limit, K> {
    /// Sets the price for this limit builder. This is a required field.
    #[must_use]
    pub fn price(mut self, price: Decimal) -> Self {
        self.price = Some(price);
        self
    }

    /// Sets the size for this limit builder. This is a required field.
    #[must_use]
    pub fn size(mut self, size: Decimal) -> Self {
        self.size = Some(size);
        self
    }

    /// Validates and transforms this limit builder into a [`SignableOrder`]
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(skip(self), err(level = "warn"))
    )]
    pub async fn build(self) -> Result<SignableOrder> {
        let Some(token_id) = self.token_id else {
            return Err(Error::validation(
                "Unable to build Order due to missing token ID",
            ));
        };

        let Some(side) = self.side else {
            return Err(Error::validation(
                "Unable to build Order due to missing token side",
            ));
        };

        let Some(price) = self.price else {
            return Err(Error::validation(
                "Unable to build Order due to missing price",
            ));
        };

        if price.is_sign_negative() {
            return Err(Error::validation(format!(
                "Unable to build Order due to negative price {price}"
            )));
        }

        let fee_rate = self.client.fee_rate_bps(token_id).await?;
        let minimum_tick_size = self
            .client
            .tick_size(token_id)
            .await?
            .minimum_tick_size
            .as_decimal();

        let decimals = minimum_tick_size.scale();

        if price.scale() > minimum_tick_size.scale() {
            return Err(Error::validation(format!(
                "Unable to build Order: Price {price} has {} decimal places. Minimum tick size \
                {minimum_tick_size} has {} decimal places. Price decimal places <= minimum tick size decimal places",
                price.scale(),
                minimum_tick_size.scale()
            )));
        }

        if price < minimum_tick_size || price > Decimal::ONE - minimum_tick_size {
            return Err(Error::validation(format!(
                "Price {price} is too small or too large for the minimum tick size {minimum_tick_size}"
            )));
        }

        let Some(size) = self.size else {
            return Err(Error::validation(
                "Unable to build Order due to missing size",
            ));
        };

        if size.scale() > LOT_SIZE_SCALE {
            return Err(Error::validation(format!(
                "Unable to build Order: Size {size} has {} decimal places. Maximum lot size is {LOT_SIZE_SCALE}",
                size.scale()
            )));
        }

        if size.is_zero() || size.is_sign_negative() {
            return Err(Error::validation(format!(
                "Unable to build Order due to negative size {size}"
            )));
        }

        let nonce = self.nonce.unwrap_or(0);
        let expiration = self.expiration.unwrap_or(DateTime::<Utc>::UNIX_EPOCH);
        let taker = self.taker.unwrap_or(Address::ZERO);
        let order_type = self.order_type.unwrap_or(OrderType::GTC);
        let post_only = Some(self.post_only.unwrap_or(false));

        if !matches!(order_type, OrderType::GTD) && expiration > DateTime::<Utc>::UNIX_EPOCH {
            return Err(Error::validation(
                "Only GTD orders may have a non-zero expiration",
            ));
        }

        if post_only == Some(true) && !matches!(order_type, OrderType::GTC | OrderType::GTD) {
            return Err(Error::validation(
                "postOnly is only supported for GTC and GTD orders",
            ));
        }

        // When buying `YES` tokens, the user will "make" `size` * `price` USDC and "take"
        // `size` `YES` tokens, and vice versa for sells. We have to truncate the notional values
        // to the combined precision of the tick size _and_ the lot size. This is to ensure that
        // this order will "snap" to the precision of resting orders on the book. The returned
        // values are quantized to `USDC_DECIMALS`.
        //
        // e.g. User submits a limit order to buy 100 `YES` tokens at $0.34.
        // This means they will take/receive 100 `YES` tokens, make/give up 34 USDC. This means that
        // the `taker_amount` is `100000000` and the `maker_amount` of `34000000`.
        let (taker_amount, maker_amount) = match side {
            Side::Buy => (
                size,
                (size * price).trunc_with_scale(decimals + LOT_SIZE_SCALE),
            ),
            Side::Sell => (
                (size * price).trunc_with_scale(decimals + LOT_SIZE_SCALE),
                size,
            ),
            side => return Err(Error::validation(format!("Invalid side: {side}"))),
        };

        let salt = to_ieee_754_int((self.salt_generator)());

        let order = Order {
            salt: U256::from(salt),
            maker: self.funder.unwrap_or(self.signer),
            taker,
            tokenId: token_id,
            makerAmount: U256::from(to_fixed_u128(maker_amount)),
            takerAmount: U256::from(to_fixed_u128(taker_amount)),
            side: side as u8,
            feeRateBps: U256::from(fee_rate.base_fee),
            nonce: U256::from(nonce),
            signer: self.signer,
            expiration: U256::from(expiration.timestamp().to_u64().ok_or(Error::validation(
                format!("Unable to represent expiration {expiration} as a u64"),
            ))?),
            signatureType: self.signature_type as u8,
        };

        #[cfg(feature = "tracing")]
        tracing::debug!(token_id = %token_id, side = ?side, price = %price, size = %size, "limit order built");

        Ok(SignableOrder {
            order,
            order_type,
            post_only,
        })
    }

    /// Build into an [`AnySignableOrder`], dispatching on the builder's configured
    /// `order_version`. V1 produces the existing [`SignableOrder`]; V2 produces
    /// [`SignableOrderV2`] with the V2 field layout.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(skip(self), err(level = "warn"))
    )]
    pub async fn build_any(self) -> Result<AnySignableOrder> {
        match self.order_version {
            OrderVersion::V1 => Ok(AnySignableOrder::V1(self.build().await?)),
            OrderVersion::V2 => Ok(AnySignableOrder::V2(self.build_v2().await?)),
        }
    }

    /// Internal: build a V2 signable order. Mirrors [`build`] but targets
    /// [`OrderV2`] fields. `taker`, `nonce`, `fee_rate_bps`, and `expiration`
    /// are not part of the V2 EIP-712 digest. `metadata` and `builder` are
    /// picked up from the builder's state (default zero).
    async fn build_v2(self) -> Result<SignableOrderV2> {
        let Some(token_id) = self.token_id else {
            return Err(Error::validation(
                "Unable to build Order due to missing token ID",
            ));
        };

        let Some(side) = self.side else {
            return Err(Error::validation(
                "Unable to build Order due to missing token side",
            ));
        };

        let Some(price) = self.price else {
            return Err(Error::validation(
                "Unable to build Order due to missing price",
            ));
        };

        if price.is_sign_negative() {
            return Err(Error::validation(format!(
                "Unable to build Order due to negative price {price}"
            )));
        }

        let minimum_tick_size = self
            .client
            .tick_size(token_id)
            .await?
            .minimum_tick_size
            .as_decimal();

        if price.scale() > minimum_tick_size.scale() {
            return Err(Error::validation(format!(
                "Unable to build Order: Price {price} has {} decimal places. Minimum tick size \
                {minimum_tick_size} has {} decimal places. Price decimal places <= minimum tick size decimal places",
                price.scale(),
                minimum_tick_size.scale()
            )));
        }

        if price < minimum_tick_size || price > Decimal::ONE - minimum_tick_size {
            return Err(Error::validation(format!(
                "Price {price} is too small or too large for the minimum tick size {minimum_tick_size}"
            )));
        }

        let Some(size) = self.size else {
            return Err(Error::validation(
                "Unable to build Order due to missing size",
            ));
        };

        if size.scale() > LOT_SIZE_SCALE {
            return Err(Error::validation(format!(
                "Unable to build Order: Size {size} has {} decimal places. Maximum lot size is {LOT_SIZE_SCALE}",
                size.scale()
            )));
        }

        if size.is_zero() || size.is_sign_negative() {
            return Err(Error::validation(format!(
                "Unable to build Order due to negative size {size}"
            )));
        }

        let order_type = self.order_type.unwrap_or(OrderType::GTC);
        let post_only = self.post_only.unwrap_or(false);

        // V2 does not use expiration in the signing digest; we skip the GTD/expiration
        // validation that V1 enforces since expiration is not relevant to V2 orders.

        if post_only && !matches!(order_type, OrderType::GTC | OrderType::GTD) {
            return Err(Error::validation(
                "postOnly is only supported for GTC and GTD orders",
            ));
        }

        // Convert the optional DateTime<Utc> expiration to U256 seconds.
        // 0 = GTC (no expiration), non-zero = GTD (seconds since Unix epoch).
        // NOTE: expiration is seconds — different unit than `timestamp`, which
        // is milliseconds (see compute_timestamp). Both are intentional and
        // match py-clob-client-v2's `order_data_v2`.
        let expiration_secs = match self.expiration {
            None => U256::ZERO,
            Some(dt) => U256::from(dt.timestamp().to_u64().ok_or_else(|| {
                Error::validation(format!(
                    "Unable to represent expiration {dt} as a u64"
                ))
            })?),
        };

        assemble_signable_order_v2(
            token_id,
            side,
            price,
            size,
            minimum_tick_size,
            order_type,
            post_only,
            self.salt_generator,
            self.timestamp_ms_source,
            self.funder,
            self.signer,
            self.signature_type,
            self.metadata,
            self.builder_field,
            expiration_secs,
            None, // maker_amount_override: Limit orders always derive maker from price*size
        )
    }
}

impl<K: AuthKind> OrderBuilder<Market, K> {
    /// Sets the price for this market builder. This is an optional field.
    #[must_use]
    pub fn price(mut self, price: Decimal) -> Self {
        self.price = Some(price);
        self
    }

    /// Sets the [`Amount`] for this market order. This is a required field.
    #[must_use]
    pub fn amount(mut self, amount: Amount) -> Self {
        self.amount = Some(amount);
        self
    }

    // Attempts to calculate the market price from the top of the book for the particular token.
    // - Uses an orderbook depth search to find the cutoff price:
    //   - BUY + USDC: walk asks until notional >= USDC
    //   - BUY + Shares: walk asks until shares >= N
    //   - SELL + Shares: walk bids until shares >= N
    async fn calculate_price(&self, order_type: OrderType) -> Result<Decimal> {
        let token_id = self
            .token_id
            .expect("Token ID was already validated in `build`");
        let side = self.side.expect("Side was already validated in `build`");
        let amount = self
            .amount
            .as_ref()
            .expect("Amount was already validated in `build`");

        let book = self
            .client
            .order_book(&OrderBookSummaryRequest {
                token_id,
                side: None,
            })
            .await?;

        if !matches!(order_type, OrderType::FAK | OrderType::FOK) {
            return Err(Error::validation(
                "Cannot set an order type other than FAK/FOK for a market order",
            ));
        }

        let (levels, amount) = match side {
            Side::Buy => (book.asks, amount.0),
            Side::Sell => match amount.0 {
                a @ AmountInner::Shares(_) => (book.bids, a),
                AmountInner::Usdc(_) => {
                    return Err(Error::validation(
                        "Sell Orders must specify their `amount`s in shares",
                    ));
                }
            },

            side => return Err(Error::validation(format!("Invalid side: {side}"))),
        };

        let first = levels.first().ok_or(Error::validation(format!(
            "No opposing orders for {token_id} which means there is no market price"
        )))?;

        let mut sum = Decimal::ZERO;
        let cutoff_price = levels.iter().rev().find_map(|level| {
            match amount {
                AmountInner::Usdc(_) => sum += level.size * level.price,
                AmountInner::Shares(_) => sum += level.size,
            }
            (sum >= amount.as_inner()).then_some(level.price)
        });

        match cutoff_price {
            Some(price) => Ok(price),
            None if matches!(order_type, OrderType::FOK) => Err(Error::validation(format!(
                "Insufficient liquidity to fill order for {token_id} at {}",
                amount.as_inner()
            ))),
            None => Ok(first.price),
        }
    }

    /// Validates and transforms this market builder into a [`SignableOrder`]
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(skip(self), err(level = "warn"))
    )]
    pub async fn build(self) -> Result<SignableOrder> {
        let Some(token_id) = self.token_id else {
            return Err(Error::validation(
                "Unable to build Order due to missing token ID",
            ));
        };

        let Some(side) = self.side else {
            return Err(Error::validation(
                "Unable to build Order due to missing token side",
            ));
        };

        let amount = self
            .amount
            .ok_or_else(|| Error::validation("Unable to build Order due to missing amount"))?;

        let nonce = self.nonce.unwrap_or(0);
        let taker = self.taker.unwrap_or(Address::ZERO);

        let order_type = self.order_type.clone().unwrap_or(OrderType::FAK);
        let post_only = self.post_only;
        if post_only == Some(true) {
            return Err(Error::validation(
                "postOnly is only supported for limit orders",
            ));
        }
        let price = match self.price {
            Some(price) => price,
            None => self.calculate_price(order_type.clone()).await?,
        };

        let minimum_tick_size = self
            .client
            .tick_size(token_id)
            .await?
            .minimum_tick_size
            .as_decimal();
        let fee_rate = self.client.fee_rate_bps(token_id).await?;

        let decimals = minimum_tick_size.scale();

        // Ensure that the market price returned internally is truncated to our tick size
        let price = price.trunc_with_scale(decimals);
        if price < minimum_tick_size || price > Decimal::ONE - minimum_tick_size {
            return Err(Error::validation(format!(
                "Price {price} is too small or too large for the minimum tick size {minimum_tick_size}"
            )));
        }

        // When buying `YES` tokens, the user will "make" `USDC` dollars and "take"
        // `USDC` / `price` `YES` tokens. When selling `YES` tokens, the user will "make" `YES`
        // token shares, and "take" `YES` shares * `price`. We have to truncate the notional values
        // to the combined precision of the tick size _and_ the lot size. This is to ensure that
        // this order will "snap" to the precision of resting orders on the book. The returned
        // values are quantized to `USDC_DECIMALS`.
        //
        // e.g. User submits a market order to buy $100 worth of `YES` tokens at
        // the current `market_price` of $0.34. This means they will take/receive (100/0.34)
        // 294.1176(47) `YES` tokens, make/give up $100. This means that the `taker_amount` is
        // `294117600` and the `maker_amount` of `100000000`.
        //
        // e.g. User submits a market order to sell 100 `YES` tokens at the current
        // `market_price` of $0.34. This means that they will take/receive $34, make/give up 100
        // `YES` tokens. This means that the `taker_amount` is `34000000` and the `maker_amount` is
        // `100000000`.
        let raw_amount = amount.as_inner();

        let (taker_amount, maker_amount) = match (side, amount.0) {
            // Spend USDC to buy shares
            (Side::Buy, AmountInner::Usdc(_)) => {
                let shares = (raw_amount / price).trunc_with_scale(decimals + LOT_SIZE_SCALE);
                (shares, raw_amount)
            }

            // Buy N shares: use cutoff `price` derived from ask depth
            (Side::Buy, AmountInner::Shares(_)) => {
                let usdc = (raw_amount * price).trunc_with_scale(decimals + LOT_SIZE_SCALE);
                (raw_amount, usdc)
            }

            // Sell N shares for USDC
            (Side::Sell, AmountInner::Shares(_)) => {
                let usdc = (raw_amount * price).trunc_with_scale(decimals + LOT_SIZE_SCALE);
                (usdc, raw_amount)
            }

            (Side::Sell, AmountInner::Usdc(_)) => {
                return Err(Error::validation(
                    "Sell Orders must specify their `amount`s in shares",
                ));
            }

            (side, _) => return Err(Error::validation(format!("Invalid side: {side}"))),
        };

        let salt = to_ieee_754_int((self.salt_generator)());

        let order = Order {
            salt: U256::from(salt),
            maker: self.funder.unwrap_or(self.signer),
            taker,
            tokenId: token_id,
            makerAmount: U256::from(to_fixed_u128(maker_amount)),
            takerAmount: U256::from(to_fixed_u128(taker_amount)),
            side: side as u8,
            feeRateBps: U256::from(fee_rate.base_fee),
            nonce: U256::from(nonce),
            signer: self.signer,
            expiration: U256::ZERO,
            signatureType: self.signature_type as u8,
        };

        #[cfg(feature = "tracing")]
        tracing::debug!(token_id = %token_id, side = ?side, price = %price, amount = %amount.as_inner(), "market order built");

        Ok(SignableOrder {
            order,
            order_type,
            post_only: None,
        })
    }

    /// Build into a V2 signable market order. Produces [`SignableOrderV2`] with:
    ///   - No taker, nonce, or feeRateBps (V1-only fields)
    ///   - ms timestamp via `timestamp_ms_source` seam (default: `Utc::now().timestamp_millis()`)
    ///   - `metadata` and `builder` fields from builder state (default zero)
    ///   - `expiration = 0` (market orders fill immediately or cancel)
    ///   - `post_only = false` (market orders are never post-only)
    ///   - `order_type` must be `FOK` or `FAK`; defaults to `FAK`
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(skip(self), err(level = "warn"))
    )]
    pub async fn build_v2(self) -> Result<SignableOrderV2> {
        let Some(token_id) = self.token_id else {
            return Err(Error::validation(
                "Unable to build Order due to missing token ID",
            ));
        };

        let Some(side) = self.side else {
            return Err(Error::validation(
                "Unable to build Order due to missing token side",
            ));
        };

        let amount = self
            .amount
            .ok_or_else(|| Error::validation("Unable to build Order due to missing amount"))?;

        let order_type = self.order_type.clone().unwrap_or(OrderType::FAK);

        if !matches!(order_type, OrderType::FOK | OrderType::FAK) {
            return Err(Error::validation(
                "V2 market orders must use FOK or FAK order type",
            ));
        }

        if self.post_only == Some(true) {
            return Err(Error::validation(
                "postOnly is only supported for limit orders",
            ));
        }

        let price = match self.price {
            Some(price) => price,
            None => self.calculate_price(order_type.clone()).await?,
        };

        let minimum_tick_size = self
            .client
            .tick_size(token_id)
            .await?
            .minimum_tick_size
            .as_decimal();

        let decimals = minimum_tick_size.scale();

        // Ensure the market price is truncated to tick size precision.
        let price = price.trunc_with_scale(decimals);
        if price < minimum_tick_size || price > Decimal::ONE - minimum_tick_size {
            return Err(Error::validation(format!(
                "Price {price} is too small or too large for the minimum tick size {minimum_tick_size}"
            )));
        }

        // Derive size (in shares) from the market amount, mirroring V1 Market::build.
        //
        // For BUY+USDC we also carry the raw USDC input forward as maker_amount_override
        // so that assemble_signable_order_v2 uses the user's exact USDC value rather than
        // recomputing it from size*price. That recomputation loses up to one tick for any
        // price that does not evenly divide the USDC amount (e.g. 100 USDC / 0.33).
        //
        // py SDK reference (authoritative):
        //   BUY+USDC:   maker_amount = round_down(usdc, lot)  ← user's exact input
        //               taker_amount = maker_amount / price    ← shares derived from that
        // BUY+Shares:   taker_amount = shares, maker_amount = shares*price
        // SELL+Shares:  maker_amount = shares, taker_amount = shares*price
        let raw_amount = amount.as_inner();
        let (size, maker_amount_override) = match (side, amount.0) {
            (Side::Buy, AmountInner::Usdc(_)) => {
                let shares = (raw_amount / price).trunc_with_scale(decimals + LOT_SIZE_SCALE);
                // Round the user's USDC input down to lot precision and scale to 6 decimals.
                let raw_maker_usdc = raw_amount.trunc_with_scale(USDC_DECIMALS);
                let raw_maker_u256 = U256::from(
                    (raw_maker_usdc * Decimal::from(10u64.pow(USDC_DECIMALS)))
                        .to_u128()
                        .ok_or_else(|| Error::validation("maker_amount overflow"))?,
                );
                (shares, Some(raw_maker_u256))
            }
            (Side::Buy, AmountInner::Shares(_)) => (raw_amount, None),
            (Side::Sell, AmountInner::Shares(_)) => (raw_amount, None),
            (Side::Sell, AmountInner::Usdc(_)) => {
                return Err(Error::validation(
                    "Sell Orders must specify their `amount`s in shares",
                ));
            }
            (side, _) => return Err(Error::validation(format!("Invalid side: {side}"))),
        };

        // Reject zero or negative size — a zero-size market order can never fill.
        if size.is_zero() || size.is_sign_negative() {
            return Err(Error::validation(format!(
                "Unable to build market order due to zero/negative size {size}"
            )));
        }

        #[cfg(feature = "tracing")]
        tracing::debug!(token_id = %token_id, side = ?side, price = %price, amount = %raw_amount, "V2 market order built");

        assemble_signable_order_v2(
            token_id,
            side,
            price,
            size,
            minimum_tick_size,
            order_type,
            false, // post_only: market orders are never post-only
            self.salt_generator,
            self.timestamp_ms_source,
            self.funder,
            self.signer,
            self.signature_type,
            self.metadata,
            self.builder_field,
            U256::ZERO,           // expiration: market orders fill immediately or cancel
            maker_amount_override, // BUY+USDC: user's exact USDC input; else None
        )
    }

    /// Build into an [`AnySignableOrder`], dispatching on the builder's configured
    /// `order_version`. V1 produces the existing [`SignableOrder`]; V2 produces
    /// [`SignableOrderV2`] with the V2 field layout.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(skip(self), err(level = "warn"))
    )]
    pub async fn build_any(self) -> Result<AnySignableOrder> {
        match self.order_version {
            OrderVersion::V1 => Ok(AnySignableOrder::V1(self.build().await?)),
            OrderVersion::V2 => Ok(AnySignableOrder::V2(self.build_v2().await?)),
        }
    }
}

/// Pure synchronous core of V2 order assembly.
///
/// All inputs are fully validated by the time this is called — `build_v2`
/// handles validation and the single async I/O (tick_size fetch), then
/// delegates here.  Exposed as `pub(crate)` so the test suite can call it
/// with a mocked `tick_size` and a pinned `timestamp_ms_source` seam without
/// requiring a live network client.
///
/// `maker_amount_override`: when `Some(v)`, that value is used as the
/// `makerAmount` field of the V2 order rather than the default `size * price`
/// computation. Pass `Some` only for BUY+USDC market orders, where the
/// maker_amount must equal the user's exact USDC input (rounded down to lot
/// precision) rather than being re-derived from `size * price` (which loses
/// up to one tick for non-clean-divisor prices). All other callers pass `None`.
///
/// A revert of the `compute_timestamp(timestamp_ms_source)` call below back to
/// `Utc::now().timestamp()` will cause `build_v2_assembles_ms_timestamp` in
/// `tests/v2_timestamp_ms.rs` to fail: the pinned seam would be ignored and
/// the returned timestamp would differ from `U256::from(1_800_000_000_000u64)`.
#[allow(clippy::too_many_arguments)]
pub fn assemble_signable_order_v2(
    token_id: U256,
    side: Side,
    price: Decimal,
    size: Decimal,
    minimum_tick_size: Decimal,
    order_type: OrderType,
    post_only: bool,
    salt_generator: fn() -> u64,
    timestamp_ms_source: fn() -> i64,
    funder: Option<Address>,
    signer: Address,
    signature_type: SignatureType,
    metadata: FixedBytes<32>,
    builder_field: FixedBytes<32>,
    expiration: U256,
    maker_amount_override: Option<U256>,
) -> Result<SignableOrderV2> {
    // EIP-1271 smart-contract signing is not yet implemented. Reject early so
    // callers get a clear error rather than an order with an unusable signature.
    if matches!(signature_type, SignatureType::Poly1271) {
        return Err(Error::validation(
            "EIP-1271 smart-contract signing not implemented (SignatureType::Poly1271); \
             full implementation deferred to a follow-up spec",
        ));
    }

    let decimals = minimum_tick_size.scale();

    // Same maker/taker amount computation as V1 — only the struct shape changes.
    // When `maker_amount_override` is Some, it replaces the computed maker_amount.
    // This is used by V2 Market BUY+USDC orders to pass the user's exact USDC
    // input as maker_amount, rather than deriving it back from size*price (which
    // reintroduces a one-tick loss for non-clean-divisor prices).
    let (taker_amount_u256, maker_amount_u256) = match side {
        Side::Buy => {
            let taker_u256 = U256::from(to_fixed_u128(size));
            let maker_u256 = maker_amount_override.unwrap_or_else(|| {
                U256::from(to_fixed_u128(
                    (size * price).trunc_with_scale(decimals + LOT_SIZE_SCALE),
                ))
            });
            (taker_u256, maker_u256)
        }
        Side::Sell => {
            // maker = shares (size) — never overridden.
            // taker = USDC received = size * price.
            let maker_u256 = U256::from(to_fixed_u128(size));
            let taker_u256 = U256::from(to_fixed_u128(
                (size * price).trunc_with_scale(decimals + LOT_SIZE_SCALE),
            ));
            (taker_u256, maker_u256)
        }
        side => return Err(Error::validation(format!("Invalid side: {side}"))),
    };

    let salt = to_ieee_754_int(salt_generator());

    let timestamp = compute_timestamp(timestamp_ms_source);

    let order = OrderV2 {
        salt: U256::from(salt),
        maker: funder.unwrap_or(signer),
        signer,
        tokenId: token_id,
        makerAmount: maker_amount_u256,
        takerAmount: taker_amount_u256,
        side: side as u8,
        signatureType: signature_type as u8,
        timestamp,
        metadata,
        builder: builder_field,
    };

    #[cfg(feature = "tracing")]
    tracing::debug!(token_id = %token_id, side = ?side, price = %price, size = %size, "V2 limit order built");

    Ok(SignableOrderV2 {
        order,
        order_type,
        post_only: Some(post_only),
        expiration,
    })
}

/// Removes trailing zeros, truncates to [`USDC_DECIMALS`] decimal places, and quanitizes as an
/// integer.
fn to_fixed_u128(d: Decimal) -> u128 {
    d.normalize()
        .trunc_with_scale(USDC_DECIMALS)
        .mantissa()
        .to_u128()
        .expect("The `build` call in `OrderBuilder<S, OrderKind, K>` ensures that only positive values are being multiplied/divided")
}

/// Mask the salt to be <= 2^53 - 1, as the backend parses as an IEEE 754.
fn to_ieee_754_int(salt: u64) -> u64 {
    salt & ((1 << 53) - 1)
}

#[must_use]
#[expect(
    clippy::float_arithmetic,
    reason = "We are not concerned with precision for the seed"
)]
#[expect(
    clippy::cast_possible_truncation,
    reason = "We are not concerned with truncation for a seed"
)]
#[expect(clippy::cast_sign_loss, reason = "We only need positive integers")]
pub(crate) fn generate_seed() -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards");

    let seconds = now.as_secs_f64();
    let rand = rand::rng().random::<f64>();

    (seconds * rand).round() as u64
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;

    #[test]
    fn to_fixed_u128_should_succeed() {
        assert_eq!(to_fixed_u128(dec!(123.456)), 123_456_000);
        assert_eq!(to_fixed_u128(dec!(123.456789)), 123_456_789);
        assert_eq!(to_fixed_u128(dec!(123.456789111111111)), 123_456_789);
        assert_eq!(to_fixed_u128(dec!(3.456789111111111)), 3_456_789);
        assert_eq!(to_fixed_u128(Decimal::ZERO), 0);
    }

    #[test]
    #[should_panic(
        expected = "The `build` call in `OrderBuilder<S, OrderKind, K>` ensures that only positive values are being multiplied/divided"
    )]
    fn to_fixed_u128_panics() {
        to_fixed_u128(dec!(-123.456));
    }

    #[test]
    fn order_salt_should_be_less_than_or_equal_to_2_to_the_53_minus_1() {
        let raw_salt = u64::MAX;
        let masked_salt = to_ieee_754_int(raw_salt);

        assert!(masked_salt < (1 << 53));
    }
}
