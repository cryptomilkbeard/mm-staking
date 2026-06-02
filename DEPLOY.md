# Deploy runbook

## Toolchain

- Agave/Solana CLI 3.1.14, anchor-cli 0.31.1, Solana platform-tools v1.52 (rustc 1.89).
- Always build via `./build.sh` (forces `--tools-version v1.52` for the SBF build; anchor 0.31.1's
  pinned platform-tools v1.43/rustc 1.79 is too old for current `edition2024` dependencies, and the
  IDL is built host-side).
- Shell PATH (the default shell does not source the profile):
  `export PATH="$HOME/.local/share/solana/install/active_release/bin:$HOME/.cargo/bin:$PATH"`

## Devnet

1. `solana config set --url devnet`
2. Fund the deployer: `solana airdrop 5`
3. `./build.sh && anchor deploy --provider.cluster devnet`
4. Record the program ID; if it changed, run `anchor keys sync` then `./build.sh` again.
5. Export the client IDL for the bot/frontends: `./scripts/export-idl.sh`
6. Publish the IDL on-chain so explorers decode instructions (see "Publish the on-chain IDL" below).
7. Initialize the pool + add reward mints (ETH/BTC/HYPE) — handled by the keeper plan's ops scripts.

## Publish the on-chain IDL (explorer decoding)

Explorers decode a program's instructions only when its IDL is published on-chain. There are TWO
standards and Solana Explorer's "Program IDL" panel reads the **Program Metadata** one:

1. **Program Metadata standard (what Explorer's "Program IDL" panel reads — DO THIS):**
   ```
   npx -y @solana-program/program-metadata@latest write idl <PROGRAM_ID> target/idl/mm_staking.json \
     --keypair <UPGRADE_AUTHORITY> --rpc <RPC_URL>
   ```
   Creates a metadata account (PDA seed `idl`) owned by the program's upgrade authority.
2. **Classic Anchor IDL (older mechanism, some Anchor tooling reads it):** optional, also fine to have both:
   ```
   anchor idl init <PROGRAM_ID> --filepath target/idl/mm_staking.json --provider.cluster <CLUSTER>
   ```

Re-run after any program change that alters the IDL: `... write idl ...` again (metadata) /
`anchor idl upgrade ...` (classic).

**Devnet (done 2026-06-02):** metadata account `Bz6wWcFg5Xga34VPuEEogRxw6waRc31Ph6KEr9JvUxzf`; classic IDL
account `3gstv6PipLqCKkbPRCyP7oGdr6KMNZ2CaJgYoyaawrC8`.

**MAINNET — REQUIRED:** publish the Program Metadata IDL right after the mainnet deploy. Because the
upgrade authority is the Squads multisig, the metadata write must be authorized by the multisig —
use `@solana-program/program-metadata`'s multisig flow (it can emit the instruction for the Squads
transaction) rather than a single-key `--keypair`. Verify the "Program IDL" panel renders on
explorer.solana.com (mainnet) before announcing.

## Capped mainnet beta

- Deploy with the upgrade authority TEMPORARILY set to the deployer key.
- After smoke tests, transfer upgrade authority to the Squads multisig:
  `solana program set-upgrade-authority <PROGRAM_ID> --new-upgrade-authority <SQUADS_VAULT>`
- Enforce the TVL/deposit cap OFF-CHAIN in the keeper/UI until confidence builds. The program itself
  has no cap; do NOT add one without an audit.

## Verifiable build (solana-verify)

Public repo: https://github.com/cryptomilkbeard/mm-staking (branch `main`).

**Toolchain pin (REQUIRED):** this program depends on crates needing `edition2024` (rustc ≥1.85),
but Anchor 0.31.1's default tools are too old. The reproducible build MUST use the
`solanafoundation/solana-verifiable-build:3.1.14` base image (platform-tools v1.52 / rustc 1.89) —
the same tools `build.sh` forces. Other base images fail with `feature edition2024 is required`.

Needs Docker. Install: `cargo install solana-verify`.

1. **Reproducible build** (must match the deployed bytecode):
   ```
   solana-verify build --library-name mm_staking \
     --base-image solanafoundation/solana-verifiable-build:3.1.14
   solana-verify get-executable-hash target/deploy/mm_staking.so
   ```
2. **Deploy that exact .so** so the on-chain hash equals the reproducible-build hash. Confirm:
   ```
   solana-verify get-program-hash -u <CLUSTER> <PROGRAM_ID>   # must equal step 1's hash
   ```
3. **Upload the on-chain verify PDA** (signed by the upgrade authority — on mainnet that's the
   Squads multisig, so use the multisig flow):
   ```
   solana-verify verify-from-repo https://github.com/cryptomilkbeard/mm-staking \
     --program-id <PROGRAM_ID> --library-name mm_staking \
     --base-image solanafoundation/solana-verifiable-build:3.1.14 \
     --commit-hash <COMMIT> -u <CLUSTER> -k <UPGRADE_AUTHORITY> --skip-prompt
   ```
   This rebuilds from the repo, asserts the hash matches on-chain, and writes the otter-verify PDA.
4. **Queue the OtterSec registry** (this is what flips explorer.solana.com's "Verified" badge —
   **MAINNET ONLY**; the remote service rejects devnet/testnet):
   ```
   solana-verify remote submit-job --program-id <PROGRAM_ID> --uploader <UPGRADE_AUTHORITY>
   ```

**Devnet status (done 2026-06-02):** on-chain verify PDA written; reproducible build hash
`2836032f6bded5be9ecb8e013d8e83ffd852f025644fd300d16e70d7b35dfefd` matches the deployed program. The
remote registry badge is mainnet-only, so the devnet explorer badge may not render — the on-chain
verification is still real and recorded.

## Upgrade authority end state

Multisig (Squads) + timelock, published. Do NOT renounce. Do NOT leave a single-key upgrade authority.

## Audit & rollout order (from the design spec)

Reuse audited Synthetix pattern → full Anchor + fuzz/solvency tests → devnet → capped mainnet beta →
professional audit (OtterSec / Neodyme / Sec3 / Zellic / Halborn) → solana-verify publish →
multisig + timelock → bug bounty.
