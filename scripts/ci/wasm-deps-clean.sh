#!/usr/bin/env bash
# Fails if a crate whose advisory is ignored in .cargo/audit.toml is part of
# the contracts' wasm32 dependency tree (i.e. could ship on-chain).
set -euo pipefail
cd "$(dirname "$0")/../.."
bad=0
for crate in zstd zstd-safe crypto-bigint; do
  if cargo tree -q -e normal --target wasm32-unknown-unknown -p agent-exit -p xusd -i "$crate" 2>/dev/null | grep -q .; then
    echo "FAIL: $crate is in the contract wasm dependency tree" >&2
    bad=1
  else
    echo "ok: $crate not in the contract wasm"
  fi
done
exit $bad
