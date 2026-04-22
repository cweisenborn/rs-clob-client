//! CR-11: orderbook summary hash byte-matches py-clob-client-v2's
//! generate_orderbook_summary_hash (SHA1 over compact JSON with fixed
//! key order and hash="" placeholder).

#![cfg(feature = "clob")]
#![allow(
    clippy::unwrap_used,
    reason = "Test code does not need additional error-handling syntax"
)]

use serde_json::Value;

#[test]
fn orderbook_hash_matches_py_reference() {
    let fixture: Value = serde_json::from_str(include_str!(
        "fixtures/orderbook_summary_hash.json"
    ))
    .unwrap();
    let expected_hash = fixture["expected_hash"].as_str().unwrap();
    let ob_json = fixture["orderbook"].clone();

    let ob: polymarket_client_sdk::clob::types::response::OrderBookSummaryResponse =
        serde_json::from_value(ob_json).expect("fixture must deserialize into fork type");

    let actual_hash = ob.hash().expect("hash() must not fail on valid orderbook");
    assert_eq!(
        actual_hash,
        expected_hash,
        "fork hash must byte-match py SDK SHA1-fixed-order-compact-JSON output"
    );
}
