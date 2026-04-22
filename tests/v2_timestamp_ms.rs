#![cfg(feature = "clob")]
#![allow(
    clippy::unwrap_used,
    reason = "Test code — unwrap is fine for fixture loading and parsing"
)]

//! CR-6: V2 order timestamp must be in milliseconds, not seconds.
//!
//! Two test strategies:
//!
//! **Test 1 — Fixture consistency**
//! The golden-vector JSON in `tests/fixtures/order_v2_eoa_ms.json` was produced
//! by the py-clob-client-v2 with time.time_ns monkey-patched to
//! `1_800_000_000_000 * 1_000_000`.  We confirm:
//!   a. The JSON's `pinned_ms_timestamp` field equals `1_800_000_000_000`.
//!   b. The JSON's `signed_order.timestamp` field (a string) equals
//!      `"1800000000000"` — proving the py SDK did in fact emit ms.
//!   c. `signed_order.timestamp` parsed as u64 equals `pinned_ms_timestamp`.
//!
//! **Test 2 — Assembly call-site test**
//! Calls `assemble_signable_order_v2` — the pure synchronous core that
//! `build_v2` delegates to after fetching `tick_size` — with a pinned
//! `timestamp_ms_source` seam (`|| 1_800_000_000_000i64`) and a mocked
//! `tick_size`.  Asserts that the returned `SignableOrderV2.order.timestamp`
//! equals `U256::from(1_800_000_000_000u64)`.
//!
//! A revert of `order_builder.rs` line 409 from
//! `compute_timestamp(timestamp_ms_source)` back to `Utc::now().timestamp()`
//! causes this test to fail: the pinned seam is ignored and the timestamp
//! diverges from `1_800_000_000_000`.

use alloy::primitives::{Address, FixedBytes, U256};
use polymarket_client_sdk::clob::order_builder::assemble_signable_order_v2;
use polymarket_client_sdk::clob::types::{OrderType, Side, SignatureType};
use rust_decimal_macros::dec;

// ── Fixture schema ────────────────────────────────────────────────────────────

const FIXTURE: &str = include_str!("fixtures/order_v2_eoa_ms.json");

/// Minimum plausible ms timestamp: 2017-07-14 in Unix ms.
/// A seconds-valued timestamp for the current era (~1_745_000_000) is well
/// below this bound, so the assertion catches the wrong units.
const MIN_MS_2017: u64 = 1_500_000_000_000;

// ── Test 1: fixture internal consistency ─────────────────────────────────────

#[test]
fn fixture_timestamp_is_milliseconds() {
    let v: serde_json::Value = serde_json::from_str(FIXTURE)
        .expect("order_v2_eoa_ms.json must be valid JSON");

    // a. pinned_ms_timestamp is exactly what we set.
    let pinned_ms: u64 = v["pinned_ms_timestamp"]
        .as_u64()
        .expect("pinned_ms_timestamp must be a u64");
    assert_eq!(
        pinned_ms, 1_800_000_000_000u64,
        "pinned_ms_timestamp should be 1_800_000_000_000 (milliseconds)"
    );

    // b. signed_order.timestamp (string) equals the pinned ms value as a string.
    let ts_str = v["signed_order"]["timestamp"]
        .as_str()
        .expect("signed_order.timestamp must be a JSON string");
    assert_eq!(
        ts_str, "1800000000000",
        "py SDK signed_order.timestamp must equal pinned ms timestamp as string"
    );

    // c. Parsed numeric value matches pinned_ms.
    let ts_u64: u64 = ts_str.parse().expect("timestamp must be numeric string");
    assert_eq!(
        ts_u64, pinned_ms,
        "signed_order.timestamp parsed == pinned_ms_timestamp"
    );

    // d. The timestamp is clearly in milliseconds (well above any seconds-era value).
    assert!(
        ts_u64 > MIN_MS_2017,
        "signed_order.timestamp {ts_u64} must be > {MIN_MS_2017} (ms range); \
         got a value that looks like seconds"
    );
}

// ── Test 2: assembly call-site traverses timestamp_ms_source ─────────────────

#[test]
fn build_v2_assembles_ms_timestamp() {
    // Call the pure synchronous core of build_v2 directly, bypassing the async
    // tick_size fetch.  The pinned seam is threaded through assemble_signable_order_v2
    // → compute_timestamp, which is exactly the path that build_v2 exercises at
    // order_builder.rs:409.  A revert of that line to Utc::now().timestamp() (seconds)
    // causes this assertion to fail: the seam is ignored and the timestamp no longer
    // equals 1_800_000_000_000.
    const PINNED_MS: i64 = 1_800_000_000_000;

    let result = assemble_signable_order_v2(
        U256::from(1u64),          // token_id (arbitrary)
        Side::Buy,
        dec!(0.50),                // price
        dec!(100.00),              // size
        dec!(0.01),                // minimum_tick_size (mocked; 2 decimal places)
        OrderType::GTC,
        false,                     // post_only
        || 1u64,                   // salt_generator (deterministic)
        || PINNED_MS,              // timestamp_ms_source — the seam under test
        None,                      // funder
        Address::ZERO,             // signer
        SignatureType::Eoa,
        FixedBytes::<32>::ZERO,    // metadata
        FixedBytes::<32>::ZERO,    // builder
        U256::ZERO,                // expiration (GTC — not relevant for this test)
        None,                      // maker_amount_override: limit orders use default computation
    );

    let signable = result.expect("assemble_signable_order_v2 must succeed with valid inputs");
    assert_eq!(
        signable.order.timestamp,
        U256::from(1_800_000_000_000u64),
        "order.timestamp must equal the pinned ms seam value; \
         a revert to Utc::now().timestamp() (seconds) would produce a different value"
    );
}
