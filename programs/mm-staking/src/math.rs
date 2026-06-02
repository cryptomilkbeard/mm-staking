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

    // --- math.rs coverage gap tests ---

    #[test]
    fn compute_reward_rate_rejects_zero_duration() {
        // duration <= 0 must return Err(InvalidDuration)
        let err = compute_reward_rate(1000, 0, 0).unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::InvalidDuration));
    }

    #[test]
    fn compute_reward_rate_rejects_negative_duration() {
        let err = compute_reward_rate(1000, 0, -1).unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::InvalidDuration));
    }

    #[test]
    fn notify_rate_past_period_finish_no_leftover() {
        // now >= period_finish  =>  remaining = 0  (the else { 0 } branch, line 69)
        // rate = amount*PRECISION / duration
        let rate = notify_rate(1000, 9999 * PRECISION, 200, 100, 100).unwrap();
        assert_eq!(rate, 1000 * PRECISION / 100);
    }

    #[test]
    fn notify_rate_exactly_at_period_finish() {
        // now == period_finish: still hits the else branch (now < period_finish is false)
        let rate = notify_rate(500, 5 * PRECISION, 100, 100, 50).unwrap();
        assert_eq!(rate, 500 * PRECISION / 50);
    }

    #[test]
    fn notify_rate_rejects_zero_duration() {
        let err = notify_rate(1000, 0, 0, 0, 0).unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::InvalidDuration));
    }

    #[test]
    fn notify_rate_rejects_negative_duration() {
        let err = notify_rate(1000, 0, 0, 0, -5).unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::InvalidDuration));
    }

    #[test]
    fn accrue_rpt_no_op_when_elapsed_zero() {
        // elapsed_secs == 0 => returns rpt_stored unchanged
        let rpt = accrue_rpt(42 * PRECISION, 0, PRECISION, 100).unwrap();
        assert_eq!(rpt, 42 * PRECISION);
    }

    #[test]
    fn accrue_rpt_no_op_when_elapsed_negative() {
        // elapsed_secs < 0 => returns rpt_stored unchanged
        let rpt = accrue_rpt(7 * PRECISION, -5, PRECISION, 100).unwrap();
        assert_eq!(rpt, 7 * PRECISION);
    }

    // --- Overflow-guard closures (ok_or_else bodies) ---

    #[test]
    fn accrue_rpt_errors_on_mul_overflow() {
        // elapsed_secs * reward_rate overflows u128:
        // elapsed=2, reward_rate = u128::MAX/1 + 1 → checked_mul returns None
        let err = accrue_rpt(0, 2, u128::MAX, 1).unwrap_err();
        // Note: rpt_stored=0 but total_staked=1 and elapsed=2, so the mul fires.
        // Actually elapsed=2, reward_rate=u128::MAX: 2 * u128::MAX overflows.
        assert_eq!(err, anchor_lang::error!(StakingError::MathOverflow));
    }

    #[test]
    fn accrue_rpt_errors_on_add_overflow() {
        // rpt_stored + delta overflows u128:
        // elapsed=1, reward_rate=1, total_staked=1 → delta=1; rpt_stored=u128::MAX → overflow
        let err = accrue_rpt(u128::MAX, 1, 1, 1).unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::MathOverflow));
    }

    #[test]
    fn earned_errors_on_mul_overflow() {
        // staked_amount * delta overflows u128:
        // delta = u128::MAX (rpt - paid), staked = 2 → 2 * u128::MAX overflows
        let err = earned(2, u128::MAX, 0, 0).unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::MathOverflow));
    }

    #[test]
    fn earned_errors_on_u64_cast_overflow() {
        // total > u64::MAX: staked=1, delta=PRECISION*(u64::MAX as u128 + 1)/PRECISION
        // Use rpt delta = (u64::MAX as u128 + 1) * PRECISION so new_rewards = u64::MAX + 1
        let big_rpt = (u64::MAX as u128 + 1) * PRECISION;
        let err = earned(1, big_rpt, 0, 0).unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::MathOverflow));
    }

    #[test]
    fn earned_errors_when_add_rewards_overflows() {
        // rewards_accrued + new_rewards overflows u128:
        // staked=1, delta = u128::MAX/1 (rpt=u128::MAX, paid=0), rewards_accrued non-zero
        // But staked*delta already overflows. Use rewards_accrued = u64::MAX, new_rewards > 0
        // that causes their u128 sum to overflow: not possible since u64 + u64 fits u128.
        // The checked_add on line 50 is between two u128 values; u128::MAX + 1 to overflow:
        // new_rewards = (staked * delta) / PRECISION must be very large.
        // Use staked = u64::MAX, delta just above 0 yielding new_rewards near u128::MAX,
        // then rewards_accrued large enough to overflow the sum.
        // Easier: call earned with staked=u64::MAX, rpt-paid = u128::MAX (triggers mul overflow first).
        // The only way to get u128 add overflow on line 50 is if new_rewards alone is near u128::MAX.
        // Since new_rewards = (staked*delta)/PRECISION and staked <= u64::MAX (1.8e19),
        // max new_rewards ≈ 1.8e19 * u128::MAX / 1e12 — mul overflows before we get there.
        // This path is UNREACHABLE with valid types (covered by the mul overflow guard above).
        // Test skipped — see coverage notes.
    }

    #[test]
    fn notify_rate_errors_on_remaining_mul_overflow() {
        // (period_finish - now) * current_rate overflows: use large current_rate
        let err = notify_rate(0, u128::MAX, 0, 2, 1).unwrap_err();
        // remaining = (2 - 0) as u128 * u128::MAX overflows
        assert_eq!(err, anchor_lang::error!(StakingError::MathOverflow));
    }

    #[test]
    fn notify_rate_errors_on_final_add_overflow() {
        // scaled_amount + remaining overflows u128.
        // scaled = u64::MAX * PRECISION (~1.8e31).
        // remaining = 1 * current_rate; choose current_rate so scaled + remaining > u128::MAX.
        // current_rate > u128::MAX - (u64::MAX * PRECISION) → at least u128::MAX - 1.8e31 + 1.
        let current_rate = u128::MAX - (u64::MAX as u128 * PRECISION) + 1;
        let err = notify_rate(u64::MAX, current_rate, 0, 1, 1).unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::MathOverflow));
    }
}
