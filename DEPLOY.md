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
6. Initialize the pool + add reward mints (ETH/BTC/HYPE) — handled by the keeper plan's ops scripts.

## Capped mainnet beta

- Deploy with the upgrade authority TEMPORARILY set to the deployer key.
- After smoke tests, transfer upgrade authority to the Squads multisig:
  `solana program set-upgrade-authority <PROGRAM_ID> --new-upgrade-authority <SQUADS_VAULT>`
- Enforce the TVL/deposit cap OFF-CHAIN in the keeper/UI until confidence builds. The program itself
  has no cap; do NOT add one without an audit.

## Verifiable build (solana-verify)

1. `cargo install solana-verify`
2. `solana-verify build` (reproducible container build)
3. After mainnet deploy: `solana-verify verify-from-repo <REPO_URL> --program-id <PROGRAM_ID>`
4. Submit verification (e.g. to the OtterSec API per solana-verify docs) so explorers show "verified".

## Upgrade authority end state

Multisig (Squads) + timelock, published. Do NOT renounce. Do NOT leave a single-key upgrade authority.

## Audit & rollout order (from the design spec)

Reuse audited Synthetix pattern → full Anchor + fuzz/solvency tests → devnet → capped mainnet beta →
professional audit (OtterSec / Neodyme / Sec3 / Zellic / Halborn) → solana-verify publish →
multisig + timelock → bug bounty.
