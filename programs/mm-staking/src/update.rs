use anchor_lang::prelude::*;
use crate::math::{accrue_rpt, earned};
use crate::state::{Pool, StakerAccount};

/// Settle one reward slot's global rpt up to `now`, rolling forward on zero stake.
fn settle_slot(slot: &mut crate::state::RewardInfo, total_staked: u64, now: i64) -> Result<()> {
    let applicable = core::cmp::min(now, slot.period_finish);
    if total_staked > 0 {
        if applicable > slot.last_update_time {
            slot.reward_per_token_stored = accrue_rpt(
                slot.reward_per_token_stored,
                applicable - slot.last_update_time,
                slot.reward_rate,
                total_staked,
            )?;
        }
        slot.last_update_time = now;
    } else {
        // zero stake: extend the finish by the idle gap so the remainder rolls forward
        if now < slot.period_finish && slot.last_update_time < now {
            slot.period_finish = slot
                .period_finish
                .checked_add(now - slot.last_update_time)
                .ok_or_else(|| error!(crate::errors::StakingError::MathOverflow))?;
        }
        slot.last_update_time = now;
    }
    Ok(())
}

/// Settle all added slots globally, and (optionally) the staker's per-slot entries.
pub fn update_reward(
    pool: &mut Pool,
    staker: Option<&mut StakerAccount>,
    now: i64,
) -> Result<()> {
    let total_staked = pool.total_staked;
    // settle globals first
    for i in 0..crate::constants::MAX_REWARDS {
        if pool.rewards[i].mint == Pubkey::default() {
            continue;
        }
        settle_slot(&mut pool.rewards[i], total_staked, now)?;
    }
    if let Some(staker) = staker {
        let staked = staker.staked_amount;
        for i in 0..crate::constants::MAX_REWARDS {
            if pool.rewards[i].mint == Pubkey::default() {
                continue;
            }
            let rpt = pool.rewards[i].reward_per_token_stored;
            staker.entries[i].rewards_accrued = earned(
                staked,
                rpt,
                staker.entries[i].reward_per_token_paid,
                staker.entries[i].rewards_accrued,
            )?;
            staker.entries[i].reward_per_token_paid = rpt;
        }
    }
    Ok(())
}

/// Find the slot index for a registered reward mint.
pub fn find_slot(pool: &Pool, mint: &Pubkey) -> Option<usize> {
    (0..crate::constants::MAX_REWARDS).find(|&i| pool.rewards[i].mint == *mint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{MAX_REWARDS, PRECISION};
    use crate::state::{Pool, RewardEntry, RewardInfo, StakerAccount};

    // Helper: build a zeroed Pool with one registered reward slot at index 0.
    fn make_pool(
        total_staked: u64,
        reward_rate: u128,
        period_finish: i64,
        last_update_time: i64,
    ) -> Pool {
        let mut pool = Pool {
            admin: Pubkey::default(),
            keeper_authority: Pubkey::default(),
            stake_mint: Pubkey::default(),
            stake_vault: Pubkey::default(),
            total_staked,
            default_duration: 3600,
            reward_count: 1,
            paused: 0,
            bump: 255,
            _pad: [0u8; 13],
            rewards: [RewardInfo::default(); MAX_REWARDS],
        };
        pool.rewards[0].mint = Pubkey::new_unique(); // mark slot 0 as registered
        pool.rewards[0].reward_rate = reward_rate;
        pool.rewards[0].period_finish = period_finish;
        pool.rewards[0].last_update_time = last_update_time;
        pool.rewards[0].reward_per_token_stored = 0;
        pool
    }

    // Helper: build a zeroed StakerAccount.
    fn make_staker(staked_amount: u64) -> StakerAccount {
        StakerAccount {
            owner: Pubkey::default(),
            pool: Pubkey::default(),
            staked_amount,
            bump: 255,
            _pad: [0u8; 7],
            entries: [RewardEntry::default(); MAX_REWARDS],
        }
    }

    // --- find_slot ---

    #[test]
    fn find_slot_returns_index_for_registered_mint() {
        let pool = make_pool(100, PRECISION, 1000, 0);
        let registered_mint = pool.rewards[0].mint;
        assert_eq!(find_slot(&pool, &registered_mint), Some(0));
    }

    #[test]
    fn find_slot_returns_none_for_unregistered_mint() {
        let pool = make_pool(100, PRECISION, 1000, 0);
        let unknown = Pubkey::new_unique();
        assert_eq!(find_slot(&pool, &unknown), None);
    }

    #[test]
    fn find_slot_returns_none_on_empty_pool() {
        let pool = Pool {
            admin: Pubkey::default(),
            keeper_authority: Pubkey::default(),
            stake_mint: Pubkey::default(),
            stake_vault: Pubkey::default(),
            total_staked: 0,
            default_duration: 3600,
            reward_count: 0,
            paused: 0,
            bump: 255,
            _pad: [0u8; 13],
            rewards: [RewardInfo::default(); MAX_REWARDS],
        };
        assert_eq!(find_slot(&pool, &Pubkey::new_unique()), None);
    }

    // --- settle_slot (via update_reward with no staker) ---

    #[test]
    fn settle_advances_rpt_when_total_staked_positive() {
        // rate = PRECISION (1 scaled token/sec), 10 staked, 100s elapsed  -> rpt += 100*PRECISION/10
        let mut pool = make_pool(10, PRECISION, 1000, 0);
        let now = 100i64;
        update_reward(&mut pool, None, now).unwrap();
        // applicable = min(100, 1000) = 100; elapsed = 100-0 = 100
        // delta = 100 * PRECISION / 10 = 10 * PRECISION
        assert_eq!(pool.rewards[0].reward_per_token_stored, 10 * PRECISION);
        assert_eq!(pool.rewards[0].last_update_time, now);
    }

    #[test]
    fn settle_caps_applicable_at_period_finish() {
        // period_finish = 50, now = 200 → applicable = 50, elapsed = 50
        let mut pool = make_pool(10, PRECISION, 50, 0);
        update_reward(&mut pool, None, 200).unwrap();
        // delta = 50 * PRECISION / 10 = 5 * PRECISION
        assert_eq!(pool.rewards[0].reward_per_token_stored, 5 * PRECISION);
        // last_update_time is set to now (200), not applicable (50)
        assert_eq!(pool.rewards[0].last_update_time, 200);
    }

    #[test]
    fn settle_no_op_when_applicable_lte_last_update_time() {
        // now <= last_update_time: applicable = min(now, period_finish).
        // Set period_finish = 50, last_update_time = 100, now = 100 → applicable=50 < last=100 → no advance.
        let mut pool = make_pool(10, PRECISION, 50, 100);
        pool.rewards[0].reward_per_token_stored = 77 * PRECISION;
        update_reward(&mut pool, None, 100).unwrap();
        // rpt unchanged
        assert_eq!(pool.rewards[0].reward_per_token_stored, 77 * PRECISION);
        assert_eq!(pool.rewards[0].last_update_time, 100);
    }

    #[test]
    fn settle_extends_period_finish_on_zero_stake() {
        // total_staked=0, now < period_finish, last_update_time < now → period_finish rolls forward
        let mut pool = make_pool(0, PRECISION, 1000, 0);
        let now = 200i64;
        update_reward(&mut pool, None, now).unwrap();
        // idle gap = now - last_update_time = 200 - 0 = 200
        // new period_finish = 1000 + 200 = 1200
        assert_eq!(pool.rewards[0].period_finish, 1200);
        assert_eq!(pool.rewards[0].last_update_time, now);
        // rpt unchanged (was 0)
        assert_eq!(pool.rewards[0].reward_per_token_stored, 0);
    }

    #[test]
    fn settle_zero_stake_no_roll_when_past_period_finish() {
        // total_staked=0, now >= period_finish → no roll (zero-stake else-branch,
        // inner if `now < slot.period_finish` is false)
        let mut pool = make_pool(0, PRECISION, 100, 50);
        update_reward(&mut pool, None, 200).unwrap();
        // period_finish unchanged (200 >= 100)
        assert_eq!(pool.rewards[0].period_finish, 100);
        assert_eq!(pool.rewards[0].last_update_time, 200);
    }

    #[test]
    fn settle_zero_stake_no_roll_when_last_update_gte_now() {
        // total_staked=0, now < period_finish but last_update_time >= now → no roll
        let mut pool = make_pool(0, PRECISION, 1000, 300);
        pool.rewards[0].period_finish = 1000;
        update_reward(&mut pool, None, 300).unwrap();
        // period_finish unchanged (last_update_time == now, inner condition false)
        assert_eq!(pool.rewards[0].period_finish, 1000);
        assert_eq!(pool.rewards[0].last_update_time, 300);
    }

    // --- update_reward with staker ---

    #[test]
    fn update_reward_with_staker_accrues_entries() {
        // rate=PRECISION, 10 staked, 100s elapsed → rpt=10*PRECISION
        // staker staked_amount=10, rewards_accrued=0, rpt_paid=0
        // earned = 10 * 10*PRECISION / PRECISION = 100
        let mut pool = make_pool(10, PRECISION, 1000, 0);
        let mut staker = make_staker(10);
        update_reward(&mut pool, Some(&mut staker), 100).unwrap();

        assert_eq!(pool.rewards[0].reward_per_token_stored, 10 * PRECISION);
        assert_eq!(staker.entries[0].rewards_accrued, 100);
        assert_eq!(staker.entries[0].reward_per_token_paid, 10 * PRECISION);
    }

    #[test]
    fn update_reward_with_staker_accumulates_accrued() {
        // Pre-populate rpt_stored and staker's paid to simulate second call
        let mut pool = make_pool(10, PRECISION, 1000, 100);
        pool.rewards[0].reward_per_token_stored = 10 * PRECISION;
        let mut staker = make_staker(10);
        staker.entries[0].reward_per_token_paid = 10 * PRECISION;
        staker.entries[0].rewards_accrued = 100; // already accrued from before

        // advance another 50s: delta = 50*PRECISION/10 = 5*PRECISION
        // earned = 10 * (15-10)*PRECISION / PRECISION + 100 = 50 + 100 = 150
        update_reward(&mut pool, Some(&mut staker), 150).unwrap();

        assert_eq!(pool.rewards[0].reward_per_token_stored, 15 * PRECISION);
        assert_eq!(staker.entries[0].rewards_accrued, 150);
        assert_eq!(staker.entries[0].reward_per_token_paid, 15 * PRECISION);
    }

    #[test]
    fn update_reward_skips_empty_slots_for_staker() {
        // Slots 1..MAX_REWARDS all have mint==Pubkey::default() → skipped
        // Only slot 0 (registered) is processed.
        let mut pool = make_pool(10, PRECISION, 1000, 0);
        let mut staker = make_staker(10);
        update_reward(&mut pool, Some(&mut staker), 100).unwrap();

        // Slots 1..7 entries must be zero (untouched)
        for i in 1..MAX_REWARDS {
            assert_eq!(staker.entries[i].rewards_accrued, 0);
            assert_eq!(staker.entries[i].reward_per_token_paid, 0);
        }
    }

    #[test]
    fn update_reward_skips_empty_slots_global() {
        // All slots default (mint==Pubkey::default()) → loop body skipped for all
        let mut pool = Pool {
            admin: Pubkey::default(),
            keeper_authority: Pubkey::default(),
            stake_mint: Pubkey::default(),
            stake_vault: Pubkey::default(),
            total_staked: 10,
            default_duration: 3600,
            reward_count: 0,
            paused: 0,
            bump: 255,
            _pad: [0u8; 13],
            rewards: [RewardInfo::default(); MAX_REWARDS],
        };
        // Should complete without error; no slots touched
        update_reward(&mut pool, None, 100).unwrap();
        for i in 0..MAX_REWARDS {
            assert_eq!(pool.rewards[i].reward_per_token_stored, 0);
        }
    }

    #[test]
    fn update_reward_no_staker_is_ok() {
        let mut pool = make_pool(10, PRECISION, 1000, 0);
        update_reward(&mut pool, None, 50).unwrap();
        // Just verify it doesn't panic and rpt advanced
        assert!(pool.rewards[0].reward_per_token_stored > 0);
    }

    // --- Error-path coverage: lines 15 and 57 (the `?` propagation arms) ---

    #[test]
    fn settle_slot_propagates_accrue_rpt_overflow_error() {
        // Trigger the `?` on line 15: make accrue_rpt return Err.
        // accrue_rpt overflows when elapsed_secs * reward_rate overflows u128.
        // Use reward_rate = u128::MAX and total_staked = 1 (so delta = elapsed * u128::MAX / 1).
        // elapsed = applicable - last_update_time.  With total_staked = 1, elapsed = 1, reward_rate = u128::MAX:
        //   checked_mul(u128::MAX) on 1 = u128::MAX, then checked_add on rpt_stored=u128::MAX overflows.
        let mut pool = make_pool(1, u128::MAX, 1000, 0);
        pool.rewards[0].reward_per_token_stored = u128::MAX;
        // elapsed = min(1, 1000) - 0 = 1; delta = 1 * u128::MAX / 1 = u128::MAX; u128::MAX + u128::MAX overflows
        let err = update_reward(&mut pool, None, 1).unwrap_err();
        assert_eq!(
            err,
            anchor_lang::error!(crate::errors::StakingError::MathOverflow)
        );
    }

    #[test]
    fn update_reward_with_staker_propagates_earned_error() {
        // Trigger the `?` on line 57: make earned() return Err.
        // earned() errors when reward_per_token < reward_per_token_paid (underflow on checked_sub).
        // Set pool rpt_stored = 5 but staker paid = 10 → delta = underflow → Err.
        let mut pool = make_pool(10, PRECISION, 1000, 100);
        // Fix rpt so it doesn't advance (applicable <= last_update_time)
        pool.rewards[0].reward_per_token_stored = 5 * PRECISION;
        let mut staker = make_staker(10);
        // staker paid more than the current rpt → earned will underflow
        staker.entries[0].reward_per_token_paid = 10 * PRECISION;
        let err = update_reward(&mut pool, Some(&mut staker), 100).unwrap_err();
        assert_eq!(
            err,
            anchor_lang::error!(crate::errors::StakingError::MathOverflow)
        );
    }

    #[test]
    fn settle_slot_period_finish_overflow_propagates() {
        // Trigger update.rs line 24: period_finish.checked_add(gap) overflows.
        // total_staked=0, now < period_finish, last_update_time < now,
        // and period_finish near i64::MAX so adding the gap overflows.
        let mut pool = make_pool(0, PRECISION, i64::MAX, 0);
        // gap = now - last_update_time = 1 - 0 = 1; period_finish + 1 overflows i64::MAX
        let err = update_reward(&mut pool, None, 1).unwrap_err();
        assert_eq!(
            err,
            anchor_lang::error!(crate::errors::StakingError::MathOverflow)
        );
    }
}
