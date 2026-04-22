"""CR-11: orderbook summary hash. SHA1 over compact JSON with fixed key order."""
import json, sys
sys.path.insert(0, '/opt/venv/lib/python3.12/site-packages')

from py_clob_client_v2.clob_types import OrderBookSummary, OrderSummary

# Pinned input — mirrors OrderBookSummaryResponse's fields.
# Uses OrderBookSummary namedtuple so generate_orderbook_summary_hash
# receives the correct type.
bids = [OrderSummary(price="0.49", size="100.00")]
asks = [OrderSummary(price="0.51", size="100.00")]

orderbook = OrderBookSummary(
    market="0xabcd1234000000000000000000000000000000000000000000000000000000aa",
    asset_id="52114319501245915516055106046884209969926127482827954674443846427813813222426",
    timestamp="1800000000000",
    hash="",
    bids=bids,
    asks=asks,
    min_order_size="0.01",
    tick_size="0.01",
    neg_risk=False,
    last_trade_price="0.50",
)

from py_clob_client_v2.utilities import generate_orderbook_summary_hash
h = generate_orderbook_summary_hash(orderbook)

# Reconstruct the plain-dict form of the orderbook for the fixture
orderbook_dict = {
    "market": orderbook.market,
    "asset_id": orderbook.asset_id,
    "timestamp": orderbook.timestamp,
    "hash": "",
    "bids": [{"price": b.price, "size": b.size} for b in orderbook.bids],
    "asks": [{"price": a.price, "size": a.size} for a in orderbook.asks],
    "min_order_size": orderbook.min_order_size,
    "tick_size": orderbook.tick_size,
    "neg_risk": orderbook.neg_risk,
    "last_trade_price": orderbook.last_trade_price,
}

print(json.dumps({
    "orderbook": orderbook_dict,
    "expected_hash": h,
}, indent=2))
