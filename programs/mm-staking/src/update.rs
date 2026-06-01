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
