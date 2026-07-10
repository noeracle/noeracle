#!/usr/bin/env bash
# Deploy a PRODUCTION oracle_v0 build (default features — the unchecked bench
# entrypoints are excluded from the wasm) and run the init ceremony: admin +
# registered publisher set. This is the committed, reproducible version of the
# deploy that was previously done by hand.
#
# Usage:
#   ./scripts/deploy_oracle_v0.sh            # testnet (default)
#   ./scripts/deploy_oracle_v0.sh mainnet
#
# Reads (never writes) the repo-root .env: NOERACLE_ADMIN_PUBLIC,
# NOERACLE_ADMIN_SECRET, NOERACLE_PUBLISHER_PUBLIC_HEX, and the per-network
# TESTNET_RPC/TESTNET_PASSPHRASE or MAINNET_RPC/MAINNET_PASSPHRASE.
#
# After a deploy, consumers must repoint to the NEW address (there is no
# upgrade entrypoint): this repo's .env *_ORACLE_V0, and on the Noether side
# the shim (set_noeracle_oracle), router (set_noeracle), keeper env — see the
# checklist this script prints at the end.
set -euo pipefail

cd "$(dirname "$0")/.."

NETWORK="${1:-testnet}"

if [[ ! -f .env ]]; then
  echo "FATAL: .env not found at repo root" >&2
  exit 1
fi
# Passphrases contain semicolons — source with allexport, never `export $(xargs)`.
set -a
# shellcheck disable=SC1091
source .env
set +a

case "$NETWORK" in
  testnet)
    RPC="${TESTNET_RPC:?TESTNET_RPC missing from .env}"
    PASSPHRASE="${TESTNET_PASSPHRASE:?TESTNET_PASSPHRASE missing from .env}"
    ;;
  mainnet)
    RPC="${MAINNET_RPC:?MAINNET_RPC missing from .env}"
    PASSPHRASE="${MAINNET_PASSPHRASE:?MAINNET_PASSPHRASE missing from .env}"
    ;;
  *)
    echo "FATAL: unknown network '$NETWORK' (use testnet or mainnet)" >&2
    exit 1
    ;;
esac

ADMIN_PUBLIC="${NOERACLE_ADMIN_PUBLIC:?NOERACLE_ADMIN_PUBLIC missing from .env}"
ADMIN_SECRET="${NOERACLE_ADMIN_SECRET:?NOERACLE_ADMIN_SECRET missing from .env}"
PUBLISHER_HEX="${NOERACLE_PUBLISHER_PUBLIC_HEX:?NOERACLE_PUBLISHER_PUBLIC_HEX missing from .env}"

if [[ ! "$PUBLISHER_HEX" =~ ^[0-9a-fA-F]{64}$ ]]; then
  echo "FATAL: NOERACLE_PUBLISHER_PUBLIC_HEX must be 32 bytes of hex (64 chars)" >&2
  exit 1
fi

WASM="target/wasm32v1-none/release/noeracle_oracle_v0.wasm"

echo "==> Building production wasm (default features)"
cargo build -p noeracle_oracle_v0 --target wasm32v1-none --release

echo "==> Verifying the exported interface is the hardened production surface"
IFACE="$(stellar contract info interface --wasm "$WASM")"
for required in update_batch_ed25519_args update_batch_ed25519_persistent get_price get_price_pers init set_publishers; do
  if ! grep -q "fn ${required}(" <<<"$IFACE"; then
    echo "FATAL: required entrypoint '${required}' missing from wasm exports" >&2
    exit 1
  fi
done
for banned in update_ed25519_args update_ed25519_stored update_ed25519_persistent update_bls_agg update_secp256k1 update_via_auth; do
  if grep -q "fn ${banned}(" <<<"$IFACE"; then
    echo "FATAL: bench entrypoint '${banned}' present in wasm — this is NOT a production build. Never deploy it." >&2
    exit 1
  fi
done
echo "    interface OK: hardened batch entrypoints present, bench entrypoints absent"

echo
echo "About to deploy oracle_v0 with:"
echo "  network:    $NETWORK"
echo "  rpc:        $RPC"
echo "  admin:      $ADMIN_PUBLIC (must authorize init)"
echo "  publisher:  $PUBLISHER_HEX"
echo
if [[ "$NETWORK" == "mainnet" ]]; then
  read -r -p "Type MAINNET to confirm a mainnet deploy: " CONFIRM
  [[ "$CONFIRM" == "MAINNET" ]] || { echo "aborted"; exit 1; }
else
  read -r -p "Proceed? [y/N] " CONFIRM
  [[ "$CONFIRM" == "y" || "$CONFIRM" == "Y" ]] || { echo "aborted"; exit 1; }
fi

echo "==> Deploying"
CONTRACT_ID="$(stellar contract deploy \
  --wasm "$WASM" \
  --source-account "$ADMIN_SECRET" \
  --rpc-url "$RPC" \
  --network-passphrase "$PASSPHRASE")"
echo "    deployed: $CONTRACT_ID"

echo "==> Running init ceremony (admin + publisher registration)"
stellar contract invoke \
  --id "$CONTRACT_ID" \
  --source-account "$ADMIN_SECRET" \
  --rpc-url "$RPC" \
  --network-passphrase "$PASSPHRASE" \
  -- init \
  --admin "$ADMIN_PUBLIC" \
  --publishers "[\"$PUBLISHER_HEX\"]"
echo "    init OK — publisher set registered"

echo
echo "=========================================================="
echo " NEW oracle_v0 ($NETWORK): $CONTRACT_ID"
echo "=========================================================="
echo "Manual follow-ups (this script never edits .env):"
if [[ "$NETWORK" == "testnet" ]]; then
  echo "  1. .env here:            TESTNET_ORACLE_V0=$CONTRACT_ID"
else
  echo "  1. .env here:            MAINNET_ORACLE_V0=$CONTRACT_ID"
fi
echo "  2. Noether shim:         invoke set_noeracle_oracle --new-noeracle $CONTRACT_ID"
echo "  3. Noether router:       invoke set_noeracle --new-noeracle $CONTRACT_ID (or redeploy router if its refresh_price changed)"
echo "  4. Noether keeper env:   NEXT_PUBLIC_NOERACLE_ID=$CONTRACT_ID (monorepo scripts/keeper AND noetherkeeperbotv2/Railway)"
echo "  5. Noether contracts.json: \"noeracle\": \"$CONTRACT_ID\""
echo "  6. Verify end-to-end:    (Noether repo) npm run noeracle:check"
