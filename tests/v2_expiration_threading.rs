#![cfg(feature = "clob")]
#![allow(
    clippy::unwrap_used,
    reason = "Test code — unwrap is fine for assertions"
)]

//! CR-8: V2 GTD order expiration flows through SignableOrderV2 →
//! SignedOrderV2 POST body serializer, producing a non-zero expiration
//! in the JSON output. OrderV2 sol! struct (EIP-712 typed data) is
//! NOT changed — expiration is not part of the signed digest on V2.

use alloy::primitives::{Address, FixedBytes, Signature, U256};
use polymarket_client_sdk::clob::order_builder::assemble_signable_order_v2;
use polymarket_client_sdk::clob::types::{OrderType, OrderV2, Side, SignatureType, SignedOrderV2};
use rust_decimal_macros::dec;
use serde_json::Value;
use uuid::Uuid;

const FIXTURE: &str = include_str!("fixtures/signed_order_v2_gtd.json");
const EXPIRATION_SECS: u64 = 1_800_003_600;
const PINNED_MS: i64 = 1_800_000_000_000;

// ── Test 1: fixture sanity + Rust serializer agreement ───────────────────────

#[test]
fn v2_post_body_expiration_matches_fixture() {
    // — Fixture sanity check —
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();

    // The fixture stores expiration as a string (py SDK order_to_json_v2).
    let fixture_expiration = fixture["post_body"]["order"]["expiration"]
        .as_str()
        .map(|s| s.to_owned())
        .or_else(|| {
            fixture["post_body"]["order"]["expiration"]
                .as_u64()
                .map(|n| n.to_string())
        })
        .expect("fixture post_body.order.expiration must be a string or integer");

    assert_eq!(
        fixture_expiration,
        EXPIRATION_SECS.to_string(),
        "py SDK fixture must emit expiration = {EXPIRATION_SECS}"
    );

    // — Rust threading check: assemble_signable_order_v2 preserves expiration —
    let signable = assemble_signable_order_v2(
        U256::from(12345678901234567890u128), // token_id
        Side::Buy,
        dec!(0.50),                           // price
        dec!(10.00),                          // size
        dec!(0.01),                           // minimum_tick_size
        OrderType::GTD,
        false,                                // post_only
        || 9_999_999u64,                      // salt_generator
        || PINNED_MS,                         // timestamp_ms_source
        None,                                 // funder
        Address::ZERO,                        // signer
        SignatureType::Eoa,
        FixedBytes::<32>::ZERO,               // metadata
        FixedBytes::<32>::ZERO,               // builder
        U256::from(EXPIRATION_SECS),          // expiration (GTD: 1h past signing)
    )
    .expect("assemble_signable_order_v2 must succeed");

    assert_eq!(
        signable.expiration,
        U256::from(EXPIRATION_SECS),
        "SignableOrderV2.expiration must equal the supplied value"
    );

    // — Rust serializer check: SignedOrderV2 emits expiration as decimal string —
    // Use stub signature (r=0, s=0, parity=false) — we're testing the serializer.
    let stub_sig = Signature::new(U256::ZERO, U256::ZERO, false);

    let signed = SignedOrderV2::builder()
        .order(signable.order)
        .signature(stub_sig)
        .order_type(signable.order_type)
        .owner(Uuid::nil())
        .expiration(signable.expiration)
        .build();

    let json = serde_json::to_value(&signed).unwrap();

    // The CLOB POST body nests the order fields inside `order`.
    let json_expiration = json["order"]["expiration"]
        .as_str()
        .expect("serialized expiration must be a JSON string");

    assert_eq!(
        json_expiration,
        EXPIRATION_SECS.to_string(),
        "SignedOrderV2 serializer must emit expiration = {EXPIRATION_SECS} as a string"
    );
}

// ── Test 2: GTC orders serialize expiration as "0" ────────────────────────────

#[test]
fn v2_post_body_expiration_zero_for_gtc() {
    // GTC orders have expiration = 0. Assert that serializes as "0".
    let signable = assemble_signable_order_v2(
        U256::from(1u64),              // token_id
        Side::Buy,
        dec!(0.50),                    // price
        dec!(100.00),                  // size
        dec!(0.01),                    // minimum_tick_size
        OrderType::GTC,
        false,                         // post_only
        || 1u64,                       // salt_generator
        || PINNED_MS,                  // timestamp_ms_source
        None,                          // funder
        Address::ZERO,                 // signer
        SignatureType::Eoa,
        FixedBytes::<32>::ZERO,        // metadata
        FixedBytes::<32>::ZERO,        // builder
        U256::ZERO,                    // expiration = 0 (GTC)
    )
    .expect("assemble_signable_order_v2 must succeed");

    assert_eq!(
        signable.expiration,
        U256::ZERO,
        "GTC SignableOrderV2.expiration must be U256::ZERO"
    );

    let stub_sig = Signature::new(U256::ZERO, U256::ZERO, false);

    // Build using .expiration(U256::ZERO) explicitly to confirm round-trip.
    let signed = SignedOrderV2::builder()
        .order(signable.order)
        .signature(stub_sig)
        .order_type(signable.order_type)
        .owner(Uuid::nil())
        .expiration(U256::ZERO)
        .build();

    let json = serde_json::to_value(&signed).unwrap();

    let json_expiration = json["order"]["expiration"]
        .as_str()
        .expect("serialized expiration must be a JSON string");

    assert_eq!(
        json_expiration, "0",
        "GTC SignedOrderV2 serializer must emit expiration = \"0\""
    );
}

// ── Test 3: builder default for expiration is "0" (GTC) ──────────────────────

#[test]
fn v2_post_body_expiration_default_is_zero() {
    // When no expiration is set via builder, it defaults to U256::ZERO (GTC).
    let stub_sig = Signature::new(U256::ZERO, U256::ZERO, false);

    let signed = SignedOrderV2::builder()
        .order(OrderV2::default())
        .signature(stub_sig)
        .order_type(OrderType::GTC)
        .owner(Uuid::nil())
        // .expiration() not called — defaults to U256::ZERO
        .build();

    let json = serde_json::to_value(&signed).unwrap();

    assert_eq!(
        json["order"]["expiration"].as_str(),
        Some("0"),
        "default (no .expiration() call) must emit expiration = \"0\""
    );
}
