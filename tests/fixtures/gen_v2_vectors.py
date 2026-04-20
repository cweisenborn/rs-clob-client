"""Generate golden vectors for V2 order signing.

Produces JSON files under tests/fixtures/v2/ that the Rust test suite
loads to verify the V2 signing pipeline (struct hash, signing hash,
signature bytes, JSON body) is byte-for-byte equivalent to the canonical
py-clob-client-v2.

Approach: we compute hashes manually via eth_account.messages.encode_typed_data
(the same library used internally by ExchangeOrderBuilderV2), extracting:
  - encoded.body  → struct_hash   (EIP-712 hashStruct of the Order message)
  - encoded.header → domain_sep  (EIP-712 domain separator hash)
  - keccak(\x19\x01 + domain_sep + struct_hash) → signing_hash

This gives us all four layers needed for the layered Rust tests without
relying on private methods in ExchangeOrderBuilderV2.

Run:
    source /opt/venv/bin/activate
    pip install eth-account eth-utils
    python3 tests/fixtures/gen_v2_vectors.py

Requires Python 3.9+.
"""
import json
import pathlib

from eth_account import Account
from eth_account.messages import encode_typed_data
from eth_utils import keccak as _keccak
from eth_utils import to_checksum_address

# ── Pinned throwaway private key (NOT a real wallet) ────────────────────────
TEST_KEY = "0x" + "11" * 32
TEST_ACCOUNT = Account.from_key(TEST_KEY)
TEST_ADDRESS = TEST_ACCOUNT.address  # 0x19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A

# ── Chain IDs ─────────────────────────────────────────────────────────────────
CHAIN_MAINNET = 137
CHAIN_AMOY = 80002

# ── Exchange contract addresses ───────────────────────────────────────────────
EXCHANGE_V2 = "0xE111180000d2663C0091e4f400237545B87B996B"
NEG_RISK_EXCHANGE_V2 = "0xe2222d279d744050d28e00520010520000310F59"

# ── EIP-712 type definitions ─────────────────────────────────────────────────
EIP712_DOMAIN_TYPES = [
    {"name": "name", "type": "string"},
    {"name": "version", "type": "string"},
    {"name": "chainId", "type": "uint256"},
    {"name": "verifyingContract", "type": "address"},
]

ORDER_STRUCT_TYPES = [
    {"name": "salt", "type": "uint256"},
    {"name": "maker", "type": "address"},
    {"name": "signer", "type": "address"},
    {"name": "tokenId", "type": "uint256"},
    {"name": "makerAmount", "type": "uint256"},
    {"name": "takerAmount", "type": "uint256"},
    {"name": "side", "type": "uint8"},
    {"name": "signatureType", "type": "uint8"},
    {"name": "timestamp", "type": "uint256"},
    {"name": "metadata", "type": "bytes32"},
    {"name": "builder", "type": "bytes32"},
]

# ── Pinned order values (common across most fixtures) ────────────────────────
PINNED_SALT = 7654321
PINNED_TOKEN_ID = 12345678901234567890  # fits in uint256, > u64
PINNED_MAKER_AMOUNT = 1000000
PINNED_TAKER_AMOUNT = 2000000
PINNED_TIMESTAMP = 1713600000
BYTES32_ZERO = bytes(32)
METADATA_ZERO = "0x" + "00" * 32
BUILDER_ZERO = "0x" + "00" * 32

OUT_DIR = pathlib.Path(__file__).parent / "v2"
OUT_DIR.mkdir(parents=True, exist_ok=True)


def hex32_to_bytes(hex_str: str) -> bytes:
    """Convert a 0x-prefixed 64-hex string to 32 bytes."""
    clean = hex_str.replace("0x", "").zfill(64)
    return bytes.fromhex(clean)


def build_fixture(
    chain_id: int,
    verifying_contract: str,
    maker: str,
    signer: str,
    token_id: int,
    maker_amount: int,
    taker_amount: int,
    side: int,           # 0=BUY, 1=SELL
    signature_type: int, # 0=EOA, 1=POLY_PROXY, 2=POLY_GNOSIS_SAFE
    timestamp: int,
    metadata_hex: str,   # 0x-prefixed 64-hex
    builder_hex: str,    # 0x-prefixed 64-hex
) -> dict:
    """Build one complete fixture dict with all four layers."""

    # ── Build the typed data dict ────────────────────────────────────────────
    typed_data = {
        "primaryType": "Order",
        "types": {
            "EIP712Domain": EIP712_DOMAIN_TYPES,
            "Order": ORDER_STRUCT_TYPES,
        },
        "domain": {
            "name": "Polymarket CTF Exchange",
            "version": "2",
            "chainId": chain_id,
            "verifyingContract": to_checksum_address(verifying_contract),
        },
        "message": {
            "salt": PINNED_SALT,
            "maker": to_checksum_address(maker),
            "signer": to_checksum_address(signer),
            "tokenId": token_id,
            "makerAmount": maker_amount,
            "takerAmount": taker_amount,
            "side": side,
            "signatureType": signature_type,
            "timestamp": timestamp,
            "metadata": hex32_to_bytes(metadata_hex),
            "builder": hex32_to_bytes(builder_hex),
        },
    }

    # ── Layer 1: struct hash (EIP-712 hashStruct) ────────────────────────────
    encoded = encode_typed_data(full_message=typed_data)
    # encoded.body is the struct hash (32 bytes)
    struct_hash_bytes = encoded.body
    struct_hash = "0x" + struct_hash_bytes.hex()

    # ── Layer 2: signing hash (\x19\x01 + domain_sep + struct_hash) ──────────
    signing_hash_bytes = _keccak(
        primitive=b"\x19" + encoded.version + encoded.header + encoded.body
    )
    signing_hash = "0x" + signing_hash_bytes.hex()

    # ── Layer 3: signature ───────────────────────────────────────────────────
    signed = Account.sign_message(encoded, private_key=TEST_KEY)
    signature_full = "0x" + signed.signature.hex()

    # Sanity: recover signer
    recovered = Account.recover_message(encoded, signature=signed.signature)
    assert recovered.lower() == TEST_ADDRESS.lower(), (
        f"Recovery mismatch: {recovered} != {TEST_ADDRESS}"
    )

    # ── Layer 4: JSON body (matches Rust SignedOrderV2::Serialize output) ──────
    # Addresses are stored as lowercase hex to match alloy's Address serializer.
    # The CLOB API accepts both checksum and lowercase addresses.
    side_str = "BUY" if side == 0 else "SELL"
    json_body_order = {
        "salt": PINNED_SALT,
        "maker": maker.lower(),
        "signer": signer.lower(),
        "tokenId": str(token_id),
        "makerAmount": str(maker_amount),
        "takerAmount": str(taker_amount),
        "side": side_str,
        "expiration": "0",
        "signatureType": signature_type,
        "timestamp": str(timestamp),
        "metadata": metadata_hex,
        "builder": builder_hex,
        "signature": signature_full,
    }

    return {
        "input": {
            "chain_id": chain_id,
            "verifying_contract": to_checksum_address(verifying_contract),
            "test_key": TEST_KEY,
            "order": {
                "salt": str(PINNED_SALT),
                "maker": to_checksum_address(maker),
                "signer": to_checksum_address(signer),
                "tokenId": str(token_id),
                "makerAmount": str(maker_amount),
                "takerAmount": str(taker_amount),
                "side": side_str,
                "signatureType": signature_type,
                "timestamp": str(timestamp),
                "metadata": metadata_hex,
                "builder": builder_hex,
            },
        },
        "eip712": {
            "struct_hash": struct_hash,
            "signing_hash": signing_hash,
        },
        "signature": {
            "full": signature_full,
        },
        "json_body": json_body_order,
    }


def main() -> None:
    common_kwargs = dict(
        maker=TEST_ADDRESS,
        signer=TEST_ADDRESS,
        token_id=PINNED_TOKEN_ID,
        maker_amount=PINNED_MAKER_AMOUNT,
        taker_amount=PINNED_TAKER_AMOUNT,
        side=0,  # BUY
        timestamp=PINNED_TIMESTAMP,
        metadata_hex=METADATA_ZERO,
        builder_hex=BUILDER_ZERO,
    )

    fixtures = {
        # 1. EOA, mainnet, standard exchange
        "eoa_mainnet.json": build_fixture(
            chain_id=CHAIN_MAINNET,
            verifying_contract=EXCHANGE_V2,
            signature_type=0,  # EOA
            **common_kwargs,
        ),
        # 2. EOA, mainnet, neg-risk exchange
        "eoa_neg_risk_mainnet.json": build_fixture(
            chain_id=CHAIN_MAINNET,
            verifying_contract=NEG_RISK_EXCHANGE_V2,
            signature_type=0,  # EOA
            **common_kwargs,
        ),
        # 3. POLY_PROXY, mainnet, standard exchange
        "poly_proxy_mainnet.json": build_fixture(
            chain_id=CHAIN_MAINNET,
            verifying_contract=EXCHANGE_V2,
            signature_type=1,  # POLY_PROXY
            **common_kwargs,
        ),
        # 4. POLY_GNOSIS_SAFE, mainnet, standard exchange
        "poly_gnosis_safe_mainnet.json": build_fixture(
            chain_id=CHAIN_MAINNET,
            verifying_contract=EXCHANGE_V2,
            signature_type=2,  # POLY_GNOSIS_SAFE
            **common_kwargs,
        ),
        # 5. EOA, Amoy (chainId=80002), same V2 contract address
        "eoa_amoy.json": build_fixture(
            chain_id=CHAIN_AMOY,
            verifying_contract=EXCHANGE_V2,
            signature_type=0,  # EOA
            **common_kwargs,
        ),
        # 6. EOA, mainnet, non-zero metadata + builder bytes32
        "eoa_nonzero_metadata.json": build_fixture(
            chain_id=CHAIN_MAINNET,
            verifying_contract=EXCHANGE_V2,
            signature_type=0,  # EOA
            **{
                **common_kwargs,
                "metadata_hex": "0x" + "ab" * 32,
                "builder_hex": "0x" + "cd" * 32,
            },
        ),
        # 7. EOA, mainnet, SELL side — catches Side enum mapping bugs in struct
        #    hash and Serialize output (side=1 vs side=0 in the BUY fixtures).
        "eoa_sell_mainnet.json": build_fixture(
            chain_id=CHAIN_MAINNET,
            verifying_contract=EXCHANGE_V2,
            signature_type=0,  # EOA
            **{
                **common_kwargs,
                "side": 1,  # SELL
            },
        ),
    }

    for filename, fixture in fixtures.items():
        path = OUT_DIR / filename
        path.write_text(json.dumps(fixture, indent=2, sort_keys=True) + "\n")
        print(f"wrote {path}")

    print("\nSanity check — first fixture:")
    fx = fixtures["eoa_mainnet.json"]
    print(f"  struct_hash  = {fx['eip712']['struct_hash']}")
    print(f"  signing_hash = {fx['eip712']['signing_hash']}")
    print(f"  signature    = {fx['signature']['full'][:20]}...")
    assert len(fx["signature"]["full"]) == 132, "signature should be 0x + 130 hex chars (65 bytes)"
    assert fx["eip712"]["struct_hash"].startswith("0x"), "struct_hash should start with 0x"
    assert fx["eip712"]["signing_hash"].startswith("0x"), "signing_hash should start with 0x"
    # All 7 should have distinct signing hashes
    hashes = [f["eip712"]["signing_hash"] for f in fixtures.values()]
    assert len(set(hashes)) == len(hashes), f"signing hashes must all be distinct: {hashes}"
    print("  All sanity checks pass.")


if __name__ == "__main__":
    main()
