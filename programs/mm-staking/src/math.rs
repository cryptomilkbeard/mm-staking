// math module — implemented in Task 3

use crate::constants::PRECISION;
use crate::errors::StakingError;
use anchor_lang::prelude::*;

/// reward_rate scaled by PRECISION: amount * PRECISION / duration
pub fn compute_reward_rate(amount: u64, _unused: u64, duration: i64) -> Result<u128> {
    require!(duration > 0, StakingError::InvalidDuration);
    (amount as u128)
        .checked_mul(PRECISION)
        .and_then(|x| x.checked_div(duration as u128))
        .ok_or_else(|| error!(StakingError::MathOverflow))
}

/// Advance reward_per_token by elapsed seconds. No-op when total_staked == 0.
pub fn accrue_rpt(
    rpt_stored: u128,
    elapsed_secs: i64,
    reward_rate: u128,
    total_staked: u64,
) -> Result<u128> {
    if total_staked == 0 || elapsed_secs <= 0 {
        return Ok(rpt_stored);
    }
    let delta = (elapsed_secs as u128)
        .checked_mul(reward_rate)
        .and_then(|x| x.checked_div(total_staked as u128))
        .ok_or_else(|| error!(StakingError::MathOverflow))?;
    rpt_stored
        .checked_add(delta)
        .ok_or_else(|| error!(StakingError::MathOverflow))
}

/// staked * (rpt - paid) / PRECISION + accrued, rounding down (vault-favoring).
pub fn earned(
    staked_amount: u64,
    reward_per_token: u128,
    reward_per_token_paid: u128,
    rewards_accrued: u64,
) -> Result<u64> {
    let delta = reward_per_token
        .checked_sub(reward_per_token_paid)
        .ok_or_else(|| error!(StakingError::MathOverflow))?;
    let new_rewards = (staked_amount as u128)
        .checked_mul(delta)
        .and_then(|x| x.checked_div(PRECISION))
        .ok_or_else(|| error!(StakingError::MathOverflow))?;
    let total = (rewards_accrued as u128)
        .checked_add(new_rewards)
        .ok_or_else(|| error!(StakingError::MathOverflow))?;
    u64::try_from(total).map_err(|_| error!(StakingError::MathOverflow))
}

/// New reward_rate after a deposit, folding any leftover of the current period.
pub fn notify_rate(
    amount: u64,
    current_rate: u128,
    now: i64,
    period_finish: i64,
    duration: i64,
) -> Result<u128> {
    require!(duration > 0, StakingError::InvalidDuration);
    let remaining = if now < period_finish {
        ((period_finish - now) as u128)
            .checked_mul(current_rate)
            .ok_or_else(|| error!(StakingError::MathOverflow))?
    } else {
        0
    };
    let scaled_amount = (amount as u128)
        .checked_mul(PRECISION)
        .ok_or_else(|| error!(StakingError::MathOverflow))?;
    scaled_amount
        .checked_add(remaining)
        .and_then(|x| x.checked_div(duration as u128))
        .ok_or_else(|| error!(StakingError::MathOverflow))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::PRECISION;

    #[test]
    fn rate_is_scaled_amount_over_duration() {
        // 3600 tokens over 3600s -> 1 token/sec, scaled by PRECISION
        let rate = compute_reward_rate(3600, PRECISION_AMOUNT(3600), 3600).unwrap();
        assert_eq!(rate, PRECISION); // 1 token/sec * PRECISION
    }

    #[test]
    fn rpt_accrues_pro_rata() {
        // rate = 1 token/sec (scaled), 10s elapsed, total_staked = 10 tokens
        // rpt += 10 * (PRECISION) / 10 = PRECISION  -> 1 token per staked token over 10s
        let rpt = accrue_rpt(0, 10, PRECISION, 10).unwrap();
        assert_eq!(rpt, PRECISION);
    }

    #[test]
    fn rpt_does_not_advance_with_zero_stake() {
        let rpt = accrue_rpt(5 * PRECISION, 10, PRECISION, 0).unwrap();
        assert_eq!(rpt, 5 * PRECISION); // unchanged
    }

    #[test]
    fn earned_rounds_down_toward_vault() {
        // staked = 3, rpt delta = PRECISION/2 -> 1.5 tokens -> floor 1
        let e = earned(3, PRECISION / 2, 0, 0).unwrap();
        assert_eq!(e, 1);
    }

    #[test]
    fn earned_includes_accrued_buffer() {
        let e = earned(0, PRECISION, 0, 7).unwrap();
        assert_eq!(e, 7);
    }

    #[test]
    fn notify_fresh_period_sets_rate() {
        // no prior period: rate = amount*PRECISION/duration
        let rate = notify_rate(1000, 0, 0, 100, 100).unwrap();
        assert_eq!(rate, 1000 * PRECISION / 100);
    }

    #[test]
    fn notify_folds_leftover() {
        // prior rate = 10*PRECISION (scaled), 50s remaining -> remaining = 50*10*PRECISION
        // new amount = 1000 over duration 100, now=50, finish=100
        let prior_rate = 10 * PRECISION;
        let remaining = (100i64 - 50i64) as u128 * prior_rate; // 500 * PRECISION
        let expected = (1000u128 * PRECISION + remaining) / 100;
        let rate = notify_rate(1000, prior_rate, 50, 100, 100).unwrap();
        assert_eq!(rate, expected);
    }

    // helper to keep tests readable
    #[allow(non_snake_case)]
    fn PRECISION_AMOUNT(t: u64) -> u64 { t }

    #[test]
    fn struct_sizes_are_stable() {
        assert_eq!(core::mem::size_of::<crate::state::RewardInfo>(), 128);
        assert_eq!(core::mem::size_of::<crate::state::RewardEntry>(), 32);
    }
}
