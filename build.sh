#!/usr/bin/env bash
set -euo pipefail
export PATH="$HOME/.local/share/solana/install/active_release/bin:$HOME/.cargo/bin:$PATH"
cd "$(dirname "$0")"
# Program .so (SBF) — force modern platform-tools so edition2024 deps compile.
anchor build --no-idl -- --tools-version v1.52
# IDL host-side (host rustc 1.96 handles edition2024; anchor idl build takes no --tools-version).
anchor idl build -o target/idl/mm_staking.json
echo "build.sh: produced target/deploy/mm_staking.so + target/idl/mm_staking.json"
