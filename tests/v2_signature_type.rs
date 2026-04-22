#![cfg(feature = "clob")]
#![allow(
    clippy::unwrap_used,
    reason = "Test code — unwrap is fine for assertions"
)]

//! CR-7 (variant-only): SignatureType::Poly1271 = 3 API-parity tests.
//!
//! Validates:
//!   1. The variant exists and encodes as `3`.
//!   2. The integer `3` round-trips through JSON deserialization.
//!   3. Attempting to assemble a V2 order with `Poly1271` returns a
//!      clear "not implemented" error rather than silently encoding garbage.

use alloy::primitives::{Address, FixedBytes, U256};
use polymarket_client_sdk::clob::order_builder::assemble_signable_order_v2;
use polymarket_client_sdk::clob::types::{OrderType, Side, SignatureType};
use rust_decimal_macros::dec;

// ── Test 1: variant exists with discriminant = 3 ────────────────────────────

#[test]
fn poly_1271_variant_exists_with_value_3() {
    assert_eq!(
        SignatureType::Poly1271 as u8,
        3,
        "SignatureType::Poly1271 must encode as 3 to match \
         py-clob-client-v2 SignatureTypeV2.POLY_1271"
    );
}

// ── Test 2: JSON round-trip (Serialize_repr encodes as integer) ───────────

#[test]
fn poly_1271_round_trips_via_json_u8() {
    // Serialize_repr → serializes as integer 3.
    let serialized = serde_json::to_string(&SignatureType::Poly1271)
        .expect("SignatureType::Poly1271 must serialize");
    assert_eq!(serialized, "3", "must serialize as integer 3");

    // Deserialize_repr → accepts integer 3 (matches Serialize_repr output).
    let deserialized: SignatureType = serde_json::from_str("3")
        .expect("integer 3 must deserialize into SignatureType::Poly1271");
    assert!(
        matches!(deserialized, SignatureType::Poly1271),
        "deserialized variant must be Poly1271, got {deserialized:?}"
    );

    // Pre-existing variants must still round-trip under Deserialize_repr —
    // the trait switch from Deserialize (string-name) to Deserialize_repr
    // (integer) is covered for all four variants, not just the new one.
    let eoa: SignatureType =
        serde_json::from_str("0").expect("integer 0 must deserialize to Eoa");
    assert!(matches!(eoa, SignatureType::Eoa));
    let proxy: SignatureType =
        serde_json::from_str("1").expect("integer 1 must deserialize to Proxy");
    assert!(matches!(proxy, SignatureType::Proxy));
    let safe: SignatureType =
        serde_json::from_str("2").expect("integer 2 must deserialize to GnosisSafe");
    assert!(matches!(safe, SignatureType::GnosisSafe));
}

// ── Test 3: signing attempt returns NotImplemented error ──────────────────

#[test]
fn poly_1271_signing_returns_not_implemented() {
    // assemble_signable_order_v2 is the synchronous core that build_v2 delegates
    // to after fetching tick_size. It is the earliest point where SignatureType
    // is still an enum (not yet cast to u8), so the guard lives here.
    let result = assemble_signable_order_v2(
        U256::from(1u64),        // token_id (arbitrary)
        Side::Buy,
        dec!(0.50),              // price
        dec!(100.00),            // size
        dec!(0.01),              // minimum_tick_size
        OrderType::GTC,
        false,                   // post_only
        || 1u64,                 // salt_generator
        || 1_800_000_000_000i64, // timestamp_ms_source
        None,                    // funder
        Address::ZERO,           // signer
        SignatureType::Poly1271, // ← the variant under test
        FixedBytes::<32>::ZERO,  // metadata
        FixedBytes::<32>::ZERO,  // builder
        U256::ZERO,              // expiration (GTC — not relevant for this test)
        None,                    // maker_amount_override: limit orders use default computation
    );

    let err = result.expect_err("Poly1271 must return Err, not a signable order");
    let err_string = err.to_string();
    assert!(
        err_string.contains("not implemented") || err_string.contains("NotImplemented"),
        "error message must mention 'not implemented'; got: {err_string}"
    );
}
