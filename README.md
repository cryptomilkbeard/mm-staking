# MM Single-Sided Staking

Single-sided staking for **MM** with **multi-bluechip streaming rewards**. Stakers deposit MM and
earn real yield paid in up to **8 different reward mints** (e.g. wrapped ETH / BTC / HYPE),
distributed pro-rata using the Synthetix `StakingRewards` streaming model.

- **Program ID:** `1Zx9vyjZLMJqsFyZxraPBww4SrSPXwHt7HFbtwpfCmA`
- **Framework:** Anchor 0.31 (Rust), `#[zero_copy]` accounts
- **Security contact:** hello@milkbot.org · https://milkbot.org

## How it works

Each `Pool` (keyed by the stake mint) holds up to 8 `RewardInfo` slots. A keeper streams rewards by
calling `deposit_rewards`, which sets a per-slot rate over a rolling window. Stakers accrue each
reward pro-rata to their share of `total_staked`; rewards are claimable any time. All accounting is
checked integer math with `u128` intermediates scaled by `1e12`; rounding always favors the vault.

### Instructions

| Instruction | Who | Purpose |
| --- | --- | --- |
| `initialize_pool` | admin | Create a pool for a stake mint + set keeper authority + default duration |
| `add_reward` / `set_reward_active` | admin | Register a reward mint (≤8) / toggle it |
| `stake` / `unstake` | holder | Deposit / withdraw MM (no cooldown) |
| `deposit_rewards` | keeper | Stream a reward amount into a slot (rate folds leftover) |
| `claim` | holder | Claim accrued rewards across all slots (one tx) |
| `emergency_withdraw` | holder | Always-available principal exit (forfeits unclaimed rewards) |
| `set_paused` / `set_keeper_authority` / `set_admin` / `set_duration` | admin | Guarded admin setters |

Principal is always exitable (even when paused); pause never blocks withdrawal.

## Build

The build forces a modern platform-tools version because Anchor 0.31.1 pins one too old for current
`edition2024` dependencies. Always build via:

```bash
export PATH="$HOME/.local/share/solana/install/active_release/bin:$HOME/.cargo/bin:$PATH"
./build.sh        # anchor build --no-idl -- --tools-version v1.52, then host-side IDL
```

Produces `target/deploy/mm_staking.so` + `target/idl/mm_staking.json`.

## Test

```bash
cargo test -p mm-staking --lib      # Rust unit tests (pure math/accrual)
npm install && npm test             # LiteSVM TypeScript integration tests
```

Coverage of the pure accounting modules: `update.rs` 100%, `math.rs` 98% (remaining lines are
unreachable overflow guards). See `coverage-summary.txt`.

## Deploy & verify

See [`DEPLOY.md`](./DEPLOY.md) for the full runbook: devnet → capped mainnet beta → professional
audit → `solana-verify` reproducible build → multisig + timelock upgrade authority → bug bounty.
The on-chain IDL is published via both the Program Metadata standard and the classic Anchor IDL so
explorers can decode instructions, and a `security.txt` is embedded in the binary.

## Security

This program custodies user principal and a reward balance. Report vulnerabilities to
**hello@milkbot.org** — good-faith disclosure is welcomed and acknowledged. Not yet audited; do not
use with significant value until the audit in `DEPLOY.md` is complete.

## License

[MIT](./LICENSE) © 2026 Milkbot
