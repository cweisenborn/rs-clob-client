//! CTF (Conditional Token Framework) client for interacting with the Gnosis CTF contract.
//!
//! The CTF contract is deployed at `0x4D97DCd97eC945f40cF65F87097ACe5EA0476045` on Polygon.
//!
//! # Operations
//!
//! - **ID Calculation**: Compute condition IDs, collection IDs, and position IDs
//! - **Split**: Convert USDC collateral into outcome token pairs (YES/NO)
//! - **Merge**: Combine outcome token pairs back into USDC
//! - **Redeem**: Redeem winning outcome tokens after market resolution
//!
//! # Example
//!
//! ```no_run
//! use polymarket_client_sdk::ctf::Client;
//! use alloy::providers::ProviderBuilder;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let provider = ProviderBuilder::new()
//!     .connect("https://polygon-rpc.com")
//!     .await?;
//!
//! let client = Client::new(provider, 137)?;
//! # Ok(())
//! # }
//! ```

#![allow(
    clippy::exhaustive_structs,
    clippy::exhaustive_enums,
    reason = "Alloy sol! macro generates code that triggers these lints"
)]

use alloy::primitives::ChainId;
use alloy::providers::Provider;
use alloy::sol;

use super::error::CtfError;
use super::types::{
    CollectionIdRequest, CollectionIdResponse, ConditionIdRequest, ConditionIdResponse,
    MergePositionsRequest, MergePositionsResponse, PositionIdRequest, PositionIdResponse,
    RedeemNegRiskRequest, RedeemNegRiskResponse, RedeemPositionsRequest, RedeemPositionsResponse,
    SplitPositionRequest, SplitPositionResponse,
};
use crate::{Result, contract_config};

// CTF (Conditional Token Framework) contract interface
//
// This interface is based on the Gnosis CTF contract.
//
// Source: https://github.com/gnosis/conditional-tokens-contracts
// Documentation: https://docs.polymarket.com/developers/CTF/overview
//
// Key functions implemented:
// - getConditionId, getCollectionId, getPositionId: Pure/view functions for ID calculations
// - splitPosition: Convert collateral into outcome tokens
// - mergePositions: Combine outcome tokens back into collateral
// - redeemPositions: Redeem winning tokens after resolution
// - prepareCondition: Initialize a new condition (included for completeness)
sol! {
    #[sol(rpc)]
    interface IConditionalTokens {
        /// Prepares a condition by initializing it with an oracle, question hash, and outcome slot count.
        function prepareCondition(
            address oracle,
            bytes32 questionId,
            uint256 outcomeSlotCount
        ) external;

        /// Calculates the condition ID from oracle, question hash, and outcome slot count.
        function getConditionId(
            address oracle,
            bytes32 questionId,
            uint256 outcomeSlotCount
        ) external pure returns (bytes32);

        /// Calculates the collection ID from parent collection, condition ID, and index set.
        function getCollectionId(
            bytes32 parentCollectionId,
            bytes32 conditionId,
            uint256 indexSet
        ) external view returns (bytes32);

        /// Calculates the position ID (ERC1155 token ID) from collateral token and collection ID.
        function getPositionId(
            address collateralToken,
            bytes32 collectionId
        ) external pure returns (uint256);

        /// Splits collateral into outcome tokens.
        function splitPosition(
            address collateralToken,
            bytes32 parentCollectionId,
            bytes32 conditionId,
            uint256[] calldata partition,
            uint256 amount
        ) external;

        /// Merges outcome tokens back into collateral.
        function mergePositions(
            address collateralToken,
            bytes32 parentCollectionId,
            bytes32 conditionId,
            uint256[] calldata partition,
            uint256 amount
        ) external;

        /// Redeems winning outcome tokens for collateral.
        function redeemPositions(
            address collateralToken,
            bytes32 parentCollectionId,
            bytes32 conditionId,
            uint256[] calldata indexSets
        ) external;
    }

    #[sol(rpc)]
    interface INegRiskAdapter {
        /// Simplified split — adapter handles collateral + partition internally.
        function splitPosition(bytes32 conditionId, uint256 amount) external;

        /// Simplified merge — adapter handles collateral + partition internally.
        function mergePositions(bytes32 conditionId, uint256 amount) external;

        /// Redeems positions from negative risk markets with specific amounts.
        function redeemPositions(
            bytes32 conditionId,
            uint256[] calldata amounts
        ) external;
    }
}

/// Client for interacting with the Conditional Token Framework contract.
///
/// The CTF contract handles tokenization of market outcomes as ERC1155 tokens.
#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct Client<P: Provider> {
    contract: IConditionalTokens::IConditionalTokensInstance<P>,
    neg_risk_adapter: Option<INegRiskAdapter::INegRiskAdapterInstance<P>>,
    provider: P,
}

impl<P: Provider + Clone> Client<P> {
    /// Creates a new CTF client for the specified chain.
    ///
    /// # Arguments
    ///
    /// * `provider` - An alloy provider instance
    /// * `chain_id` - The chain ID (137 for Polygon mainnet, 80002 for Amoy testnet)
    ///
    /// # Errors
    ///
    /// Returns an error if the contract configuration is not found for the given chain.
    pub fn new(provider: P, chain_id: ChainId) -> Result<Self> {
        let config = contract_config(chain_id, false).ok_or_else(|| {
            CtfError::ContractCall(format!(
                "CTF contract configuration not found for chain ID {chain_id}"
            ))
        })?;

        let contract = IConditionalTokens::new(config.conditional_tokens, provider.clone());

        Ok(Self {
            contract,
            neg_risk_adapter: None,
            provider,
        })
    }

    /// Creates a new CTF client with `NegRisk` adapter support.
    ///
    /// Use this constructor when you need to work with negative risk markets.
    ///
    /// # Arguments
    ///
    /// * `provider` - An alloy provider instance
    /// * `chain_id` - The chain ID (137 for Polygon mainnet, 80002 for Amoy testnet)
    ///
    /// # Errors
    ///
    /// Returns an error if the contract configuration is not found for the given chain,
    /// or if the `NegRisk` adapter is not configured for the chain.
    pub fn with_neg_risk(provider: P, chain_id: ChainId) -> Result<Self> {
        let config = contract_config(chain_id, true).ok_or_else(|| {
            CtfError::ContractCall(format!(
                "NegRisk contract configuration not found for chain ID {chain_id}"
            ))
        })?;

        let contract = IConditionalTokens::new(config.conditional_tokens, provider.clone());

        let neg_risk_adapter = config
            .neg_risk_adapter
            .map(|addr| INegRiskAdapter::new(addr, provider.clone()));

        Ok(Self {
            contract,
            neg_risk_adapter,
            provider,
        })
    }

    /// Creates a CTF client that dispatches NEG-RISK V2 split/merge/redeem
    /// through the V2 wrapper adapter at `config.neg_risk_ctf_collateral_adapter`
    /// (pUSD-denominated — `0xAdA200001000ef00D07553cEE7006808F895c6F1` on
    /// Polygon).
    ///
    /// # Role — the neg-risk V2 dispatch
    ///
    /// Polymarket's V2 rollout introduces **two** collateral-wrapper adapters
    /// that replace V1's direct-to-CTF / direct-to-`NegRiskAdapter` calling
    /// conventions:
    ///
    /// | V2 adapter field                         | Address (Polygon)                            | Routes for           |
    /// |------------------------------------------|----------------------------------------------|----------------------|
    /// | `config.ctf_collateral_adapter`          | `0xADa100874d00e3331D00F2007a9c336a65009718` | STANDARD (negRisk=f) |
    /// | `config.neg_risk_ctf_collateral_adapter` | `0xAdA200001000ef00D07553cEE7006808F895c6F1` | NEG-RISK (negRisk=t) |
    ///
    /// `with_standard_v2` covers the STANDARD path; this constructor covers the
    /// parallel NEG-RISK path so callers can dispatch per-market on Gamma's
    /// `negRisk` flag without falling back to V1 (which will be deprecated
    /// 2026-04-28).
    ///
    /// # The wrapper exposes 5-arg `IConditionalTokens` selectors (via inheritance)
    ///
    /// The `NegRiskCtfCollateralAdapter` contract at
    /// `config.neg_risk_ctf_collateral_adapter` **inherits from
    /// `CtfCollateralAdapter`** (see
    /// [Polymarket/ctf-exchange-v2 `src/adapters/NegRiskCtfCollateralAdapter.sol`](https://github.com/Polymarket/ctf-exchange-v2/blob/main/src/adapters/NegRiskCtfCollateralAdapter.sol)).
    /// Its public external surface is therefore the canonical
    /// `IConditionalTokens` 5-arg shape:
    ///
    /// - `splitPosition(address, bytes32, bytes32, uint256[], uint256)`
    /// - `mergePositions(address, bytes32, bytes32, uint256[], uint256)`
    /// - `redeemPositions(address, bytes32, bytes32, uint256[])`
    ///
    /// It accepts pUSD as its collateral input (so the EOA approves/holds pUSD,
    /// not USDC.e) and internally converts pUSD → USDC.e before forwarding to
    /// the real CTF contract at `config.conditional_tokens`. From the calldata
    /// perspective the wrapper is indistinguishable from the raw CTF contract —
    /// just at a different address.
    ///
    /// Critically, the wrapper **does NOT expose** the 2-arg simplified
    /// `splitPosition(bytes32, uint256)` / `mergePositions(bytes32, uint256)`
    /// that the OLD V1 `NegRiskAdapter` (at `0xd91E80cF…`) exposes. That
    /// interface is V1-only. Attempting to call the 2-arg selectors against
    /// the V2 wrapper reverts with `data: "0x"` (unknown selector) — diagnosed
    /// on-chain 2026-04-23 via `eth_call` prior to this fix.
    ///
    /// # Construction — symmetric with `with_standard_v2`
    ///
    /// We therefore bind the `IConditionalTokens` interface directly to the
    /// wrapper address (not to `config.conditional_tokens`) and leave
    /// `neg_risk_adapter` as `None`. Callers use the **same 5-arg methods** as
    /// `with_standard_v2` — `split_position(&request)`,
    /// `merge_positions(&request)`, `redeem_positions(&request)` — with pUSD as
    /// the `collateral_token` field. The only structural difference between
    /// the two V2 constructors is which wrapper address backs the `contract`
    /// field.
    ///
    /// The legacy 2-arg methods on this client
    /// (`split_position_neg_risk` / `merge_positions_neg_risk` / `redeem_neg_risk`)
    /// will return `Err` because `neg_risk_adapter` is `None`; they are
    /// intentionally V1-only post this fix. See their rustdoc for migration
    /// guidance.
    ///
    /// # Arguments
    ///
    /// * `provider` - An alloy provider instance
    /// * `chain_id` - The chain ID (137 for Polygon mainnet). Amoy (80002) has
    ///   no V2 adapter configured and will return `Err`.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The contract configuration is not found for the given chain (e.g. an
    ///   unknown chain ID).
    /// - The chain has no V2 neg-risk CTF collateral adapter configured
    ///   (`config.neg_risk_ctf_collateral_adapter` is `None`) — e.g., Amoy
    ///   pre-V2-deployment.
    pub fn with_neg_risk_v2(provider: P, chain_id: ChainId) -> Result<Self> {
        let config = contract_config(chain_id, true).ok_or_else(|| {
            CtfError::ContractCall(format!(
                "NegRisk contract configuration not found for chain ID {chain_id}"
            ))
        })?;

        let adapter_addr = config.neg_risk_ctf_collateral_adapter.ok_or_else(|| {
            CtfError::ContractCall(format!(
                "V2 neg-risk CTF collateral adapter not configured for chain ID {chain_id}"
            ))
        })?;

        // Wrapper exposes the IConditionalTokens-compatible 5-arg selectors via
        // inheritance from CtfCollateralAdapter (see Polymarket/ctf-exchange-v2
        // src/adapters/NegRiskCtfCollateralAdapter.sol). Bind the IConditionalTokens
        // interface to the wrapper address — symmetric with `with_standard_v2`.
        // Callers use the 5-arg `split_position(&request)` / `merge_positions(&request)` /
        // `redeem_positions(&request)` methods with pUSD as collateral.
        let contract = IConditionalTokens::new(adapter_addr, provider.clone());

        Ok(Self {
            contract,
            // The V2 neg-risk wrapper does NOT expose the 2-arg simplified
            // interface (splitPosition(bytes32, uint256) etc.) — that is V1-only,
            // implemented by the OLD NegRiskAdapter at 0xd91E80cF…. Calling
            // the 2-arg selectors against this wrapper reverts with
            // `data: "0x"` (unknown selector). Leaving this `None` forces
            // the legacy 2-arg methods (split_position_neg_risk, etc.) to
            // return Err — callers must migrate to the 5-arg path above.
            neg_risk_adapter: None,
            provider,
        })
    }

    /// Creates a CTF client that dispatches STANDARD (non-neg-risk) V2
    /// split/merge/redeem through the V2 wrapper adapter at
    /// `config.ctf_collateral_adapter` (pUSD-denominated — `0xADa1…9718` on Polygon).
    ///
    /// # Role — the missing V2 dispatch for non-neg-risk markets
    ///
    /// Polymarket's V2 rollout introduces **two** collateral-wrapper adapters
    /// that replace V1's direct-to-CTF / direct-to-NegRiskAdapter calling
    /// conventions:
    ///
    /// | V2 adapter field                       | Address (Polygon)                              | Routes for           |
    /// |----------------------------------------|------------------------------------------------|----------------------|
    /// | `config.ctf_collateral_adapter`        | `0xADa100874d00e3331D00F2007a9c336a65009718`   | STANDARD (negRisk=f) |
    /// | `config.neg_risk_ctf_collateral_adapter` | `0xAdA200001000ef00D07553cEE7006808F895c6F1` | NEG-RISK (negRisk=t) |
    ///
    /// `with_neg_risk_v2` already covers the neg-risk path. This constructor
    /// covers the parallel STANDARD path so callers can dispatch per-market on
    /// Gamma's `negRisk` flag without falling back to the V1 CTF contract
    /// (which will be deprecated 2026-04-28).
    ///
    /// # The pUSD → USDC.e wrapping trick
    ///
    /// The wrapper contract at `config.ctf_collateral_adapter` accepts pUSD as
    /// its collateral input (so the EOA approves/holds pUSD, not USDC.e) and
    /// internally converts pUSD → USDC.e before forwarding to the real CTF
    /// contract at `config.conditional_tokens`. Critically, the wrapper exposes
    /// **canonical `IConditionalTokens` 5-arg selectors** —
    /// `splitPosition(address,bytes32,bytes32,uint256[],uint256)`,
    /// `mergePositions(...)`, `redeemPositions(...)` — so from the calldata
    /// perspective it is indistinguishable from the raw CTF contract.
    ///
    /// We therefore deliberately place the wrapper's address in the
    /// `contract: IConditionalTokens::…Instance` field (typed as
    /// `IConditionalTokens`, not a new wrapper type) and let the existing
    /// `split_position` / `merge_positions` / `redeem_positions` methods route
    /// through it unchanged. `neg_risk_adapter` is left `None` — this client
    /// is non-neg-risk.
    ///
    /// This mirrors `with_neg_risk_v2`'s design where the V2 neg-risk wrapper
    /// is typed as `INegRiskAdapter` because its selectors are compatible; the
    /// only structural difference between the two V2 constructors is which
    /// field holds the wrapper address (here: `contract`, there:
    /// `neg_risk_adapter`) and which interface the wrapper conforms to (here:
    /// 5-arg CTF, there: 2-arg NegRiskAdapter).
    ///
    /// Verified against a live unresolved standard BTC market via eth_call
    /// (pre-cutover integration test):
    ///
    /// ```text
    /// STD.splitPosition(pUSD, 0x0, cid, [1, 2], 1e6) from=EOA → 0x (success)
    /// ```
    ///
    /// # Arguments
    ///
    /// * `provider` - An alloy provider instance
    /// * `chain_id` - The chain ID (137 for Polygon mainnet). Amoy (80002) has
    ///   no V2 adapter configured and will return `Err`.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The contract configuration is not found for the given chain (e.g. an
    ///   unknown chain ID).
    /// - The chain has no V2 standard CTF collateral adapter configured
    ///   (`config.ctf_collateral_adapter` is `None`) — e.g., Amoy
    ///   pre-V2-deployment.
    pub fn with_standard_v2(provider: P, chain_id: ChainId) -> Result<Self> {
        let config = contract_config(chain_id, false).ok_or_else(|| {
            CtfError::ContractCall(format!(
                "CTF contract configuration not found for chain ID {chain_id}"
            ))
        })?;

        let adapter_addr = config.ctf_collateral_adapter.ok_or_else(|| {
            CtfError::ContractCall(format!(
                "V2 standard CTF collateral adapter not configured for chain ID {chain_id}"
            ))
        })?;

        // Intentionally type the wrapper as IConditionalTokens: it exposes
        // canonical 5-arg CTF selectors (splitPosition / mergePositions /
        // redeemPositions) and internally handles pUSD → USDC.e conversion
        // before forwarding to the real CTF contract. This lets the existing
        // split_position / merge_positions / redeem_positions methods on this
        // client route through the wrapper unchanged.
        let contract = IConditionalTokens::new(adapter_addr, provider.clone());

        Ok(Self {
            contract,
            neg_risk_adapter: None,
            provider,
        })
    }

    /// Calculates a condition ID.
    ///
    /// The condition ID is derived from the oracle address, question hash, and number of outcome slots.
    ///
    /// # Errors
    ///
    /// Returns an error if the contract call fails.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(level = "debug", skip(self), fields(
            oracle = %request.oracle,
            question_id = %request.question_id,
            outcome_slot_count = %request.outcome_slot_count
        ))
    )]
    pub async fn condition_id(&self, request: &ConditionIdRequest) -> Result<ConditionIdResponse> {
        let condition_id = self
            .contract
            .getConditionId(
                request.oracle,
                request.question_id,
                request.outcome_slot_count,
            )
            .call()
            .await
            .map_err(|e| CtfError::ContractCall(format!("Failed to get condition ID: {e}")))?;

        Ok(ConditionIdResponse { condition_id })
    }

    /// Calculates a collection ID.
    ///
    /// Creates collection identifiers using parent collection, condition ID, and index set.
    ///
    /// # Errors
    ///
    /// Returns an error if the contract call fails.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(level = "debug", skip(self), fields(
            parent_collection_id = %request.parent_collection_id,
            condition_id = %request.condition_id,
            index_set = %request.index_set
        ))
    )]
    pub async fn collection_id(
        &self,
        request: &CollectionIdRequest,
    ) -> Result<CollectionIdResponse> {
        let collection_id = self
            .contract
            .getCollectionId(
                request.parent_collection_id,
                request.condition_id,
                request.index_set,
            )
            .call()
            .await
            .map_err(|e| CtfError::ContractCall(format!("Failed to get collection ID: {e}")))?;

        Ok(CollectionIdResponse { collection_id })
    }

    /// Calculates a position ID (ERC1155 token ID).
    ///
    /// Generates final token IDs from collateral token and collection ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the contract call fails.
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(level = "debug", skip(self), fields(
            collateral_token = %request.collateral_token,
            collection_id = %request.collection_id
        ))
    )]
    pub async fn position_id(&self, request: &PositionIdRequest) -> Result<PositionIdResponse> {
        let position_id = self
            .contract
            .getPositionId(request.collateral_token, request.collection_id)
            .call()
            .await
            .map_err(|e| CtfError::ContractCall(format!("Failed to get position ID: {e}")))?;

        Ok(PositionIdResponse { position_id })
    }

    /// Splits collateral into outcome tokens.
    ///
    /// Converts USDC collateral into matched outcome token pairs (YES/NO).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The transaction fails to send
    /// - The transaction fails to be mined
    /// - The wallet doesn't have sufficient collateral
    /// - The condition hasn't been prepared
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(level = "debug", skip(self), fields(
            collateral_token = %request.collateral_token,
            condition_id = %request.condition_id,
            amount = %request.amount
        ))
    )]
    pub async fn split_position(
        &self,
        request: &SplitPositionRequest,
    ) -> Result<SplitPositionResponse> {
        let pending_tx = self
            .contract
            .splitPosition(
                request.collateral_token,
                request.parent_collection_id,
                request.condition_id,
                request.partition.clone(),
                request.amount,
            )
            .send()
            .await
            .map_err(|e| {
                CtfError::ContractCall(format!("Failed to send split transaction: {e}"))
            })?;

        let transaction_hash = *pending_tx.tx_hash();

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| CtfError::ContractCall(format!("Failed to get split receipt: {e}")))?;

        if !receipt.status() {
            return Err(CtfError::ContractCall(format!(
                "split tx reverted on-chain: tx={transaction_hash:#x} block={}",
                receipt.block_number.unwrap_or_default()
            ))
            .into());
        }

        Ok(SplitPositionResponse {
            transaction_hash,
            block_number: receipt.block_number.ok_or_else(|| {
                CtfError::ContractCall("Block number not available in receipt".to_owned())
            })?,
        })
    }

    /// Merges outcome tokens back into collateral.
    ///
    /// Combines matched outcome token pairs back into USDC.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The transaction fails to send
    /// - The transaction fails to be mined
    /// - The wallet doesn't have sufficient outcome tokens
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(level = "debug", skip(self), fields(
            collateral_token = %request.collateral_token,
            condition_id = %request.condition_id,
            amount = %request.amount
        ))
    )]
    pub async fn merge_positions(
        &self,
        request: &MergePositionsRequest,
    ) -> Result<MergePositionsResponse> {
        let pending_tx = self
            .contract
            .mergePositions(
                request.collateral_token,
                request.parent_collection_id,
                request.condition_id,
                request.partition.clone(),
                request.amount,
            )
            .send()
            .await
            .map_err(|e| {
                CtfError::ContractCall(format!("Failed to send merge transaction: {e}"))
            })?;

        let transaction_hash = *pending_tx.tx_hash();

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| CtfError::ContractCall(format!("Failed to get merge receipt: {e}")))?;

        if !receipt.status() {
            return Err(CtfError::ContractCall(format!(
                "merge tx reverted on-chain: tx={transaction_hash:#x} block={}",
                receipt.block_number.unwrap_or_default()
            ))
            .into());
        }

        Ok(MergePositionsResponse {
            transaction_hash,
            block_number: receipt.block_number.ok_or_else(|| {
                CtfError::ContractCall("Block number not available in receipt".to_owned())
            })?,
        })
    }

    /// Redeems winning outcome tokens for collateral.
    ///
    /// After a condition is resolved, burns winning tokens to recover USDC.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The transaction fails to send
    /// - The transaction fails to be mined
    /// - The condition hasn't been resolved
    /// - The wallet doesn't have the specified outcome tokens
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(level = "debug", skip(self), fields(
            collateral_token = %request.collateral_token,
            condition_id = %request.condition_id
        ))
    )]
    pub async fn redeem_positions(
        &self,
        request: &RedeemPositionsRequest,
    ) -> Result<RedeemPositionsResponse> {
        let pending_tx = self
            .contract
            .redeemPositions(
                request.collateral_token,
                request.parent_collection_id,
                request.condition_id,
                request.index_sets.clone(),
            )
            .send()
            .await
            .map_err(|e| {
                CtfError::ContractCall(format!("Failed to send redeem transaction: {e}"))
            })?;

        let transaction_hash = *pending_tx.tx_hash();

        let receipt = pending_tx
            .get_receipt()
            .await
            .map_err(|e| CtfError::ContractCall(format!("Failed to get redeem receipt: {e}")))?;

        if !receipt.status() {
            return Err(CtfError::ContractCall(format!(
                "redeem tx reverted on-chain: tx={transaction_hash:#x} block={}",
                receipt.block_number.unwrap_or_default()
            ))
            .into());
        }

        Ok(RedeemPositionsResponse {
            transaction_hash,
            block_number: receipt.block_number.ok_or_else(|| {
                CtfError::ContractCall("Block number not available in receipt".to_owned())
            })?,
        })
    }

    /// Splits via the 2-arg simplified signature exposed by the OLD V1
    /// `NegRiskAdapter`.
    ///
    /// **V1-only.** Intended for V1 neg-risk (OLD adapter via `with_neg_risk`,
    /// USDC.e-denominated at `0xd91E80cF…`), which exposes the 2-arg
    /// simplified interface `splitPosition(bytes32, uint256)` — the adapter
    /// handles collateral + partition internally.
    ///
    /// Under **V2**, `with_neg_risk_v2` binds to a wrapper that does NOT
    /// expose this 2-arg interface (its external surface is the canonical
    /// 5-arg `IConditionalTokens` shape via inheritance from
    /// `CtfCollateralAdapter`). V2 callers must use the 5-arg
    /// `split_position(&request)` / `merge_positions(&request)` /
    /// `redeem_positions(&request)` methods with pUSD as collateral instead.
    /// This method will return `Err` on a `with_neg_risk_v2`-constructed
    /// client because `neg_risk_adapter` is `None` under that path.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - This client was constructed without a V1 NegRisk adapter (i.e., plain
    ///   `Client::new`, or `with_neg_risk_v2` — V2 does not expose this selector)
    /// - The transaction fails to send
    /// - The transaction fails to be mined
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(level = "debug", skip(self), fields(
            condition_id = %condition_id,
            amount = %amount
        ))
    )]
    pub async fn split_position_neg_risk(
        &self,
        condition_id: alloy::primitives::B256,
        amount: alloy::primitives::U256,
    ) -> Result<SplitPositionResponse> {
        let adapter = self.neg_risk_adapter.as_ref().ok_or_else(|| {
            CtfError::ContractCall(
                "NegRisk adapter not configured — V1-only path, use Client::with_neg_risk. Under V2 (with_neg_risk_v2), use the 5-arg split_position / merge_positions instead (wrapper exposes IConditionalTokens)".to_owned(),
            )
        })?;

        let pending_tx = adapter
            .splitPosition(condition_id, amount)
            .send()
            .await
            .map_err(|e| {
                CtfError::ContractCall(format!("Failed to send NegRisk split transaction: {e}"))
            })?;

        let transaction_hash = *pending_tx.tx_hash();

        let receipt = pending_tx.get_receipt().await.map_err(|e| {
            CtfError::ContractCall(format!("Failed to get NegRisk split receipt: {e}"))
        })?;

        if !receipt.status() {
            return Err(CtfError::ContractCall(format!(
                "NegRisk split tx reverted on-chain: tx={transaction_hash:#x} block={}",
                receipt.block_number.unwrap_or_default()
            ))
            .into());
        }

        Ok(SplitPositionResponse {
            transaction_hash,
            block_number: receipt.block_number.ok_or_else(|| {
                CtfError::ContractCall("Block number not available in receipt".to_owned())
            })?,
        })
    }

    /// Merges via the 2-arg simplified signature exposed by the OLD V1
    /// `NegRiskAdapter`.
    ///
    /// **V1-only.** See `split_position_neg_risk` for full dispatch
    /// semantics. Under V2, `with_neg_risk_v2` binds to a wrapper that does
    /// NOT expose this 2-arg interface — callers must use the 5-arg
    /// `merge_positions(&request)` with pUSD collateral instead.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - This client was constructed without a V1 NegRisk adapter (i.e., plain
    ///   `Client::new`, or `with_neg_risk_v2` — V2 does not expose this selector)
    /// - The transaction fails to send
    /// - The transaction fails to be mined
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(level = "debug", skip(self), fields(
            condition_id = %condition_id,
            amount = %amount
        ))
    )]
    pub async fn merge_positions_neg_risk(
        &self,
        condition_id: alloy::primitives::B256,
        amount: alloy::primitives::U256,
    ) -> Result<MergePositionsResponse> {
        let adapter = self.neg_risk_adapter.as_ref().ok_or_else(|| {
            CtfError::ContractCall(
                "NegRisk adapter not configured — V1-only path, use Client::with_neg_risk. Under V2 (with_neg_risk_v2), use the 5-arg split_position / merge_positions instead (wrapper exposes IConditionalTokens)".to_owned(),
            )
        })?;

        let pending_tx = adapter
            .mergePositions(condition_id, amount)
            .send()
            .await
            .map_err(|e| {
                CtfError::ContractCall(format!("Failed to send NegRisk merge transaction: {e}"))
            })?;

        let transaction_hash = *pending_tx.tx_hash();

        let receipt = pending_tx.get_receipt().await.map_err(|e| {
            CtfError::ContractCall(format!("Failed to get NegRisk merge receipt: {e}"))
        })?;

        if !receipt.status() {
            return Err(CtfError::ContractCall(format!(
                "NegRisk merge tx reverted on-chain: tx={transaction_hash:#x} block={}",
                receipt.block_number.unwrap_or_default()
            ))
            .into());
        }

        Ok(MergePositionsResponse {
            transaction_hash,
            block_number: receipt.block_number.ok_or_else(|| {
                CtfError::ContractCall("Block number not available in receipt".to_owned())
            })?,
        })
    }

    /// Redeems positions from V1 negative risk markets via the OLD V1
    /// `NegRiskAdapter`'s amount-keyed `redeemPositions` signature.
    ///
    /// **V1-only.** Intended for V1 neg-risk (OLD adapter via `with_neg_risk`),
    /// which exposes `redeemPositions(bytes32 conditionId, uint256[] amounts)`
    /// — keyed by exact per-outcome amounts rather than index sets.
    ///
    /// Under **V2**, `with_neg_risk_v2` binds to a wrapper that does NOT
    /// expose this signature (its surface is the canonical 5-arg
    /// `IConditionalTokens` `redeemPositions(address, bytes32, bytes32, uint256[])`
    /// via inheritance from `CtfCollateralAdapter`). V2 callers must use the
    /// 5-arg `redeem_positions(&request)` with pUSD collateral and index sets
    /// instead. This method returns `Err` on a `with_neg_risk_v2`-constructed
    /// client because `neg_risk_adapter` is `None` under that path.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The client was not created with `with_neg_risk()` (adapter not
    ///   available — includes `Client::new` and `with_neg_risk_v2`)
    /// - The transaction fails to send
    /// - The transaction fails to be mined
    /// - The condition hasn't been resolved
    /// - The wallet doesn't have the specified outcome token amounts
    #[cfg_attr(
        feature = "tracing",
        tracing::instrument(level = "debug", skip(self), fields(
            condition_id = %request.condition_id,
            amounts_len = request.amounts.len()
        ))
    )]
    pub async fn redeem_neg_risk(
        &self,
        request: &RedeemNegRiskRequest,
    ) -> Result<RedeemNegRiskResponse> {
        let adapter = self.neg_risk_adapter.as_ref().ok_or_else(|| {
            CtfError::ContractCall(
                "NegRisk adapter not available — V1-only path, use Client::with_neg_risk() to enable. Under V2 (with_neg_risk_v2), use the 5-arg redeem_positions with pUSD + index sets instead (wrapper exposes IConditionalTokens)".to_owned()
            )
        })?;

        let pending_tx = adapter
            .redeemPositions(request.condition_id, request.amounts.clone())
            .send()
            .await
            .map_err(|e| {
                CtfError::ContractCall(format!("Failed to send NegRisk redeem transaction: {e}"))
            })?;

        let transaction_hash = *pending_tx.tx_hash();

        let receipt = pending_tx.get_receipt().await.map_err(|e| {
            CtfError::ContractCall(format!("Failed to get NegRisk redeem receipt: {e}"))
        })?;

        if !receipt.status() {
            return Err(CtfError::ContractCall(format!(
                "NegRisk redeem tx reverted on-chain: tx={transaction_hash:#x} block={}",
                receipt.block_number.unwrap_or_default()
            ))
            .into());
        }

        Ok(RedeemNegRiskResponse {
            transaction_hash,
            block_number: receipt.block_number.ok_or_else(|| {
                CtfError::ContractCall("Block number not available in receipt".to_owned())
            })?,
        })
    }

    /// Returns a reference to the underlying provider.
    #[must_use]
    pub const fn provider(&self) -> &P {
        &self.provider
    }
}
