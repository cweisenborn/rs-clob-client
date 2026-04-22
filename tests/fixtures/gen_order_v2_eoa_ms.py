"""
Generate golden-vector reference output for CR-6: V2 EOA signed order
with timestamp in MILLISECONDS.

The py-clob-client-v2 ExchangeOrderBuilderV2 calls:
    str(time.time_ns() // 1_000_000)
inside build_order() when no timestamp is provided in the OrderDataV2.

We monkey-patch time.time_ns in the builder module so the fixed millisecond
timestamp (PINNED_MS) flows through deterministically.

Usage:
    source /opt/venv/bin/activate
    python tests/fixtures/gen_order_v2_eoa_ms.py > tests/fixtures/order_v2_eoa_ms.json
"""
import dataclasses
import json

import py_clob_client_v2.order_utils.exchange_order_builder_v2 as builder_mod
from py_clob_client_v2.order_utils.exchange_order_builder_v2 import ExchangeOrderBuilderV2
from py_clob_client_v2.order_utils.model.order_data_v2 import OrderDataV2
from py_clob_client_v2.order_utils.model.side import Side
from py_clob_client_v2.order_utils.model.signature_type_v2 import SignatureTypeV2
from py_clob_client_v2.signer import Signer

PRIVATE_KEY = "0x" + "11" * 32
EXCHANGE = "0xE111180000d2663C0091e4f400237545B87B996B"
CHAIN_ID = 137
PINNED_MS = 1_800_000_000_000

# Pinned order inputs — deterministic salt so the fixture is reproducible.
PINNED_SALT = "9999999"
PINNED_TOKEN_ID = "12345678901234567890"
# BUY: price=0.50, size=10 shares
# makerAmount = 5_000_000 (5 USDC at 6 decimals)
# takerAmount = 10_000_000 (10 shares at 6 decimals)
PINNED_MAKER_AMOUNT = "5000000"
PINNED_TAKER_AMOUNT = "10000000"

# Monkey-patch time.time_ns so builder uses the pinned ms timestamp.
builder_mod.time.time_ns = lambda: PINNED_MS * 1_000_000

# Build a signed V2 EOA limit order.
signer = Signer(PRIVATE_KEY, CHAIN_ID)

builder = ExchangeOrderBuilderV2(
    contract_address=EXCHANGE,
    chain_id=CHAIN_ID,
    signer=signer,
    generate_salt=lambda: PINNED_SALT,
)

order_data = OrderDataV2(
    maker=signer.address(),
    signer=signer.address(),
    tokenId=PINNED_TOKEN_ID,
    makerAmount=PINNED_MAKER_AMOUNT,
    takerAmount=PINNED_TAKER_AMOUNT,
    side=Side.BUY,
    signatureType=SignatureTypeV2.EOA,
    # timestamp=None => builder calls time.time_ns() // 1_000_000 (monkey-patched)
)

signed_order = builder.build_signed_order(order_data)
signed_order_dict = dataclasses.asdict(signed_order)

# Verify the monkey-patch worked.
assert signed_order.timestamp == str(PINNED_MS), (
    f"timestamp mismatch: expected '{PINNED_MS}', got '{signed_order.timestamp}'"
)

out = {
    "pinned_ms_timestamp": PINNED_MS,
    "signer_eoa": signer.address(),
    "signed_order": signed_order_dict,
}

print(json.dumps(out, indent=2, sort_keys=True, default=str))
