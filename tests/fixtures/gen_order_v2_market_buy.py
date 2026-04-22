"""
V2 market-buy signed order reference output for CR-9.

Generates a golden fixture for cross-checking Rust's build_v2 on
OrderBuilder<Market, _> against py-clob-client-v2.

Scenario:
  - BUY $100 USDC worth of YES tokens at price $0.33 (FOK, V2, EOA)
  - tick_size = 0.01 (2 decimal price precision, 2 decimal lot precision)
  - amount (USDC) = 100.0 → shares = 100 / 0.33 = 303.0303... (truncated to 303.03)
  - makerAmount = 100_000_000 (100 USDC at 6 decimals — user's exact input)
  - takerAmount = 303_030_300 (303.0303 shares at 6 decimals)
  - expiration = 0 (market orders fill immediately or cancel)
  - post_only = absent / false

Price 0.33 is a non-clean-divisor of 100 USDC, which exercises the C-1 bug:
  WITHOUT the fix: maker_amount = size * price = 303.0303 * 0.33 = 99.999999 → 99_999_900
  WITH the fix:    maker_amount = round_down(amount, lot) = 100.0 → 100_000_000

The py SDK's get_market_order_amounts for BUY:
  raw_maker_amt = round_down(amount, size_precision) = 100.0   ← user's exact input
  raw_taker_amt = raw_maker_amt / round_down(price, price_precision) = 100.0 / 0.33 = 303.030...
                  truncated to lot precision (4 decimal places) → 303.0303
  makerAmount = to_token_decimals(100.0) = 100_000_000
  takerAmount = to_token_decimals(303.0303) = 303_030_300

Usage:
    source /opt/venv/bin/activate
    python tests/fixtures/gen_order_v2_market_buy.py > tests/fixtures/order_v2_market_buy.json
"""
import dataclasses
import json

import py_clob_client_v2.order_utils.exchange_order_builder_v2 as builder_mod
from py_clob_client_v2.order_builder.builder import OrderBuilder
from py_clob_client_v2.clob_types import MarketOrderArgsV2, CreateOrderOptions, OrderType
from py_clob_client_v2.order_utils.model.side import Side
from py_clob_client_v2.order_utils.model.signature_type_v2 import SignatureTypeV2
from py_clob_client_v2.signer import Signer

# ── Pinned constants ──────────────────────────────────────────────────────────

PRIVATE_KEY = "0x" + "11" * 32
CHAIN_ID = 137
PINNED_MS = 1_800_000_000_000

TOKEN_ID = "12345678901234567890"
TICK_SIZE = "0.01"

# Market BUY: spend 100 USDC at price 0.33 → receive 303.03 shares (non-clean-divisor)
AMOUNT_USDC = 100.0
PRICE = 0.33

# Pinned salt so the fixture is fully deterministic.
PINNED_SALT = "1234567890"

# ── Monkey-patch time.time_ns so the builder emits the pinned ms timestamp ────

builder_mod.time.time_ns = lambda: PINNED_MS * 1_000_000

# ── Build the V2 market order ─────────────────────────────────────────────────

signer = Signer(PRIVATE_KEY, CHAIN_ID)
ob = OrderBuilder(signer, SignatureTypeV2.EOA)

# Patch salt generator on the ExchangeOrderBuilderV2 that build_market_order
# creates internally. We do this by patching the module-level generate_salt
# default so the builder picks it up.
from py_clob_client_v2.order_utils.exchange_order_builder_v2 import ExchangeOrderBuilderV2

_orig_init = ExchangeOrderBuilderV2.__init__

def _patched_init(self, contract_address, chain_id, signer, generate_salt=None):
    _orig_init(self, contract_address, chain_id, signer,
               generate_salt=lambda: PINNED_SALT)

ExchangeOrderBuilderV2.__init__ = _patched_init

args = MarketOrderArgsV2(
    token_id=TOKEN_ID,
    amount=AMOUNT_USDC,
    side=Side.BUY,
    price=PRICE,
    order_type=OrderType.FOK,
)
opts = CreateOrderOptions(tick_size=TICK_SIZE, neg_risk=False)

signed_order = ob.build_market_order(args, opts, version=2)
signed_order_dict = dataclasses.asdict(signed_order)

# ── Sanity-check the fixture before writing ───────────────────────────────────

assert signed_order_dict["timestamp"] == str(PINNED_MS), (
    f"timestamp mismatch: expected '{PINNED_MS}', got '{signed_order_dict['timestamp']}'"
)
assert signed_order_dict["side"] == 0, f"expected BUY=0, got {signed_order_dict['side']}"
assert signed_order_dict["makerAmount"] == "100000000", (
    f"makerAmount mismatch: {signed_order_dict['makerAmount']}"
)
# takerAmount: 100 / 0.33 = 303.0303... truncated to lot precision (4 decimals) → 303.0303
# at 6 decimals → 303_030_300
assert signed_order_dict["takerAmount"] == "303030300", (
    f"takerAmount mismatch: {signed_order_dict['takerAmount']}"
)
assert signed_order_dict["expiration"] == "0", (
    f"expiration should be 0 for market orders, got {signed_order_dict['expiration']}"
)

# ── Output ────────────────────────────────────────────────────────────────────

out = {
    "pinned_ms_timestamp": PINNED_MS,
    "token_id": TOKEN_ID,
    "tick_size": TICK_SIZE,
    "amount_usdc": AMOUNT_USDC,
    "price": PRICE,
    "signer_eoa": signer.address(),
    "signed_order": signed_order_dict,
}

print(json.dumps(out, indent=2, sort_keys=True, default=str))
