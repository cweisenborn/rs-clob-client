"""
Generate V2 GTD order POST body with expiration = 1_800_003_600 (1h past
pinned ms-ts of 1_800_000_000_000). Cross-checks py SDK's expiration
threading — POST body should contain the non-zero expiration.

Usage:
  source /opt/venv/bin/activate
  python tests/fixtures/gen_signed_order_v2_gtd.py > tests/fixtures/signed_order_v2_gtd.json
"""
import json

import py_clob_client_v2.order_utils.exchange_order_builder_v2 as builder_mod
from py_clob_client_v2.order_utils.exchange_order_builder_v2 import ExchangeOrderBuilderV2
from py_clob_client_v2.order_utils.model.order_data_v2 import OrderDataV2, order_to_json_v2
from py_clob_client_v2.order_utils.model.side import Side
from py_clob_client_v2.order_utils.model.signature_type_v2 import SignatureTypeV2
from py_clob_client_v2.signer import Signer

PRIVATE_KEY = "0x" + "11" * 32
EXCHANGE = "0xE111180000d2663C0091e4f400237545B87B996B"
CHAIN_ID = 137
PINNED_MS = 1_800_000_000_000
# PINNED_MS / 1000 = 1_800_000_000 seconds; +3600 = 1 hour past signing
EXPIRATION_SECS = 1_800_003_600

# Pin clock so the fixture is deterministic.
builder_mod.time.time_ns = lambda: PINNED_MS * 1_000_000

signer = Signer(PRIVATE_KEY, CHAIN_ID)

builder = ExchangeOrderBuilderV2(
    contract_address=EXCHANGE,
    chain_id=CHAIN_ID,
    signer=signer,
    generate_salt=lambda: "9999999",
)

order_data = OrderDataV2(
    maker=signer.address(),
    signer=signer.address(),
    tokenId="12345678901234567890",
    makerAmount="5000000",
    takerAmount="10000000",
    side=Side.BUY,
    signatureType=SignatureTypeV2.EOA,
    expiration=str(EXPIRATION_SECS),
)

signed_order = builder.build_signed_order(order_data)

# Sanity checks.
assert signed_order.timestamp == str(PINNED_MS), (
    f"timestamp mismatch: expected '{PINNED_MS}', got '{signed_order.timestamp}'"
)
assert signed_order.expiration == str(EXPIRATION_SECS), (
    f"expiration mismatch on signed_order: expected '{EXPIRATION_SECS}', got '{signed_order.expiration}'"
)

post_body = order_to_json_v2(
    signed_order,
    owner=signer.address(),
    order_type="GTD",
)

assert str(post_body["order"]["expiration"]) == str(EXPIRATION_SECS), (
    f"post_body expiration mismatch: got '{post_body['order']['expiration']}'"
)

out = {
    "pinned_ms_timestamp": PINNED_MS,
    "expiration_secs": EXPIRATION_SECS,
    "post_body": post_body,
}

print(json.dumps(out, indent=2, sort_keys=True, default=str))
