#!/usr/bin/env bash
set -euo pipefail
# Copies the built IDL (+ TS types if present) to a dist dir the bot/frontends vendor in.
OUT="${1:-dist-client}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
mkdir -p "$OUT"
cp target/idl/mm_staking.json "$OUT/"
# TS types are only emitted by a full `anchor build`. Our build.sh uses `anchor build --no-idl`
# + `anchor idl build` (JSON IDL only), so target/types may not exist — that's fine: the JSON
# IDL alone is sufficient to construct a @coral-xyz/anchor `Program`. Copy types if present.
if [ -f target/types/mm_staking.ts ]; then
  cp target/types/mm_staking.ts "$OUT/"
fi
echo "Exported IDL to $OUT/ ($(ls "$OUT"))"
