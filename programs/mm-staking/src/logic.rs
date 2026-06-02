//! Pure instruction-handler logic, extracted from `instructions/*.rs` for
//! host-side unit testing (measurable coverage). Each function operates on plain
//! `&mut Pool` / `&mut StakerAccount` + scalars — NO Anchor `Context`. The thin
//! Anchor handlers load accounts, call these, then perform the SPL token CPI.
//!
//! Behavior is byte-for-byte identical to the original inline handler bodies:
//! same `require!` conditions and ORDER, same checked arithmetic, same
//! `StakingError` variants, same `update_reward(...)` timing.

use anchor_lang::prelude::*;

use crate::constants::*;
use crate::errors::StakingError;
use crate::math::notify_rate;
use crate::state::{Pool, StakerAccount};
use crate::update::{find_slot, update_reward};

// ---------------------------------------------------------------------------
// initialize_pool.rs
// ---------------------------------------------------------------------------

/// Mirrors `initialize_pool::handler`: validate duration, then set all fields.
pub fn init_pool(
    pool: &mut Pool,
    admin: Pubkey,
    keeper: Pubkey,
    stake_mint: Pubkey,
    stake_vault: Pubkey,
    default_duration: i64,
    bump: u8,
) -> Result<()> {
    require!(default_duration > 0, StakingError::InvalidDuration);
    pool.admin = admin;
    pool.keeper_authority = keeper;
    pool.stake_mint = stake_mint;
    pool.stake_vault = stake_vault;
    pool.total_staked = 0;
    pool.default_duration = default_duration;
    pool.reward_count = 0;
    pool.paused = 0;
    pool.bump = bump;
    Ok(())
}

// ---------------------------------------------------------------------------
// add_reward.rs
// ---------------------------------------------------------------------------

/// Mirrors `add_reward::handler`: reject duplicate mint, pick a free slot, set
/// the slot fields, bump `reward_count`. `now` is the clock timestamp supplied
/// by the handler (used for `last_update_time`).
pub fn add_reward(
    pool: &mut Pool,
    mint: Pubkey,
    vault: Pubkey,
    duration_arg: i64,
    now: i64,
) -> Result<()> {
    require!(
        find_slot(pool, &mint).is_none(),
        StakingError::RewardAlreadyExists
    );
    let duration = if duration_arg > 0 {
        duration_arg
    } else {
        pool.default_duration
    };

    let free = (0..MAX_REWARDS).find(|&i| pool.rewards[i].mint == Pubkey::default());
    let idx = free.ok_or_else(|| error!(StakingError::RewardSlotsFull))?;

    pool.rewards[idx].mint = mint;
    pool.rewards[idx].vault = vault;
    pool.rewards[idx].reward_rate = 0;
    pool.rewards[idx].reward_per_token_stored = 0;
    pool.rewards[idx].period_finish = 0;
    pool.rewards[idx].last_update_time = now;
    pool.rewards[idx].duration = duration;
    pool.rewards[idx].active = 1;
    pool.reward_count = pool
        .reward_count
        .checked_add(1)
        .ok_or_else(|| error!(StakingError::MathOverflow))?;
    Ok(())
}

/// Mirrors `add_reward::set_active_handler`.
pub fn set_reward_active(pool: &mut Pool, slot: u8, active: bool) -> Result<()> {
    let i = slot as usize;
    require!(
        i < MAX_REWARDS && pool.rewards[i].mint != Pubkey::default(),
        StakingError::RewardNotFound
    );
    pool.rewards[i].active = if active { 1 } else { 0 };
    Ok(())
}

// ---------------------------------------------------------------------------
// stake.rs
// ---------------------------------------------------------------------------

/// Mirrors `stake::stake_handler` state logic. In the handler `amount > 0` is
/// checked at the very top BEFORE loading the pool, so it stays the FIRST check
/// here. `owner`/`pool_key`/`staker_bump` are used to initialize the staker on
/// first use (when `staker.owner == default`).
pub fn stake(
    pool: &mut Pool,
    staker: &mut StakerAccount,
    owner: Pubkey,
    pool_key: Pubkey,
    staker_bump: u8,
    amount: u64,
    now: i64,
) -> Result<()> {
    require!(amount > 0, StakingError::ZeroAmount);
    require!(pool.paused == 0, StakingError::Paused);

    // init staker fields on first use
    if staker.owner == Pubkey::default() {
        staker.owner = owner;
        staker.pool = pool_key;
        staker.bump = staker_bump;
    }
    update_reward(pool, Some(staker), now)?;
    staker.staked_amount = staker
        .staked_amount
        .checked_add(amount)
        .ok_or_else(|| error!(StakingError::MathOverflow))?;
    pool.total_staked = pool
        .total_staked
        .checked_add(amount)
        .ok_or_else(|| error!(StakingError::MathOverflow))?;
    Ok(())
}

/// Mirrors `stake::unstake_handler` state logic. Pause is deliberately NOT
/// checked — principal must stay exitable.
pub fn unstake(pool: &mut Pool, staker: &mut StakerAccount, amount: u64, now: i64) -> Result<()> {
    require!(amount > 0, StakingError::ZeroAmount);
    // unstake is allowed even when paused (principal must stay exitable)
    require!(
        staker.staked_amount >= amount,
        StakingError::InsufficientStake
    );
    update_reward(pool, Some(staker), now)?;
    staker.staked_amount -= amount;
    pool.total_staked -= amount;
    Ok(())
}

// ---------------------------------------------------------------------------
// deposit_rewards.rs
// ---------------------------------------------------------------------------

/// Mirrors `deposit_rewards::handler` state logic. The handler computes
/// `received` as the post-transfer vault balance delta and passes it in.
/// (The `amount > 0` check and the CPI/balance-delta computation stay in the
/// handler, exactly as today.)
pub fn deposit_rewards(pool: &mut Pool, mint: Pubkey, received: u64, now: i64) -> Result<()> {
    let i = find_slot(pool, &mint).ok_or_else(|| error!(StakingError::RewardNotFound))?;
    require!(pool.rewards[i].active == 1, StakingError::RewardInactive);

    // settle globals to now, then re-rate
    update_reward(pool, None, now)?;
    let duration = pool.rewards[i].duration;
    let new_rate = notify_rate(
        received,
        pool.rewards[i].reward_rate,
        now,
        pool.rewards[i].period_finish,
        duration,
    )?;
    pool.rewards[i].reward_rate = new_rate;
    pool.rewards[i].last_update_time = now;
    pool.rewards[i].period_finish = now
        .checked_add(duration)
        .ok_or_else(|| error!(StakingError::MathOverflow))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// emergency_withdraw.rs
// ---------------------------------------------------------------------------

/// Mirrors `emergency_withdraw::handler` state logic: zero the staker's
/// principal, decrement `total_staked` (saturating), forfeit unclaimed rewards,
/// and return the withdrawn amount. Deliberately does NOT call `update_reward`.
/// The handler keeps `require!(amount > 0, ZeroAmount)` AFTER calling this,
/// exactly as today.
pub fn emergency_withdraw(pool: &mut Pool, staker: &mut StakerAccount) -> Result<u64> {
    let amt = staker.staked_amount;
    staker.staked_amount = 0;
    pool.total_staked = pool.total_staked.saturating_sub(amt);
    // forfeit unclaimed rewards
    for i in 0..MAX_REWARDS {
        staker.entries[i].rewards_accrued = 0;
        staker.entries[i].reward_per_token_paid = pool.rewards[i].reward_per_token_stored;
    }
    Ok(amt)
}

// ---------------------------------------------------------------------------
// admin.rs
// ---------------------------------------------------------------------------

/// Mirrors `admin::set_paused`.
pub fn set_paused(pool: &mut Pool, paused: bool) -> Result<()> {
    pool.paused = if paused { 1 } else { 0 };
    Ok(())
}

/// Mirrors `admin::set_keeper_authority`.
pub fn set_keeper_authority(pool: &mut Pool, keeper: Pubkey) -> Result<()> {
    pool.keeper_authority = keeper;
    Ok(())
}

/// Mirrors `admin::set_admin`.
pub fn set_admin(pool: &mut Pool, new_admin: Pubkey) -> Result<()> {
    pool.admin = new_admin;
    Ok(())
}

/// Mirrors `admin::set_duration`.
pub fn set_duration(pool: &mut Pool, slot: u8, duration: i64) -> Result<()> {
    require!(duration > 0, StakingError::InvalidDuration);
    let i = slot as usize;
    require!(
        i < MAX_REWARDS && pool.rewards[i].mint != Pubkey::default(),
        StakingError::RewardNotFound
    );
    pool.rewards[i].duration = duration;
    Ok(())
}

// ---------------------------------------------------------------------------
// claim.rs
// ---------------------------------------------------------------------------

/// Mirrors the global settle at the top of `claim::handler`.
pub fn claim_settle_all(pool: &mut Pool, staker: &mut StakerAccount, now: i64) -> Result<()> {
    update_reward(pool, Some(staker), now)
}

/// Mirrors the per-vault settle inside `claim::handler`'s loop: find the slot
/// whose `vault` matches `vault_key`, read the accrued amount, zero it, and
/// return `(idx, amount)`. The handler keeps the `remaining.len()` even check,
/// the `require_keys_eq!` vault match, the `Account::try_from` + owner/mint
/// checks, the `if amount == 0 { continue }`, and the CPI.
pub fn claim_take_slot(
    pool: &Pool,
    staker: &mut StakerAccount,
    vault_key: &Pubkey,
) -> Result<(usize, u64)> {
    let idx = (0..MAX_REWARDS).find(|&i| pool.rewards[i].vault == *vault_key);
    let idx = idx.ok_or_else(|| error!(StakingError::VaultMismatch))?;
    let amt = staker.entries[idx].rewards_accrued;
    staker.entries[idx].rewards_accrued = 0;
    Ok((idx, amt))
}

// ---------------------------------------------------------------------------
// Host-side unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{MAX_REWARDS, PRECISION};
    use crate::state::{Pool, RewardEntry, RewardInfo, StakerAccount};

    // -----------------------------------------------------------------------
    // Builders
    // -----------------------------------------------------------------------

    /// Build a blank Pool with all reward slots empty.
    fn empty_pool() -> Pool {
        Pool {
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
        }
    }

    /// Build a Pool that has one reward registered at slot 0 (mint+vault unique).
    fn pool_with_one_reward(total_staked: u64) -> Pool {
        let mut p = empty_pool();
        p.total_staked = total_staked;
        p.reward_count = 1;
        p.rewards[0].mint = Pubkey::new_unique();
        p.rewards[0].vault = Pubkey::new_unique();
        p.rewards[0].active = 1;
        p.rewards[0].duration = 3600;
        p.rewards[0].last_update_time = 0;
        p.rewards[0].period_finish = 0;
        p.rewards[0].reward_rate = 0;
        p.rewards[0].reward_per_token_stored = 0;
        p
    }

    /// Build a Pool with all 8 reward slots occupied (slots full).
    fn full_pool() -> Pool {
        let mut p = empty_pool();
        for i in 0..MAX_REWARDS {
            p.rewards[i].mint = Pubkey::new_unique();
            p.rewards[i].vault = Pubkey::new_unique();
            p.rewards[i].active = 1;
            p.rewards[i].duration = 3600;
        }
        p.reward_count = MAX_REWARDS as u8;
        p
    }

    /// Build a blank StakerAccount (owner = default = uninitialised).
    fn staker() -> StakerAccount {
        StakerAccount {
            owner: Pubkey::default(),
            pool: Pubkey::default(),
            staked_amount: 0,
            bump: 255,
            _pad: [0u8; 7],
            entries: [RewardEntry::default(); MAX_REWARDS],
        }
    }

    // -----------------------------------------------------------------------
    // init_pool
    // -----------------------------------------------------------------------

    #[test]
    fn init_pool_success_sets_all_fields() {
        let mut pool = empty_pool();
        // pre-dirty a few fields so we can confirm they are overwritten
        pool.total_staked = 999;
        pool.reward_count = 7;
        pool.paused = 1;

        let admin = Pubkey::new_unique();
        let keeper = Pubkey::new_unique();
        let stake_mint = Pubkey::new_unique();
        let stake_vault = Pubkey::new_unique();

        init_pool(&mut pool, admin, keeper, stake_mint, stake_vault, 7200, 42).unwrap();

        assert_eq!(pool.admin, admin);
        assert_eq!(pool.keeper_authority, keeper);
        assert_eq!(pool.stake_mint, stake_mint);
        assert_eq!(pool.stake_vault, stake_vault);
        assert_eq!(pool.total_staked, 0);
        assert_eq!(pool.default_duration, 7200);
        assert_eq!(pool.reward_count, 0);
        assert_eq!(pool.paused, 0);
        assert_eq!(pool.bump, 42);
    }

    #[test]
    fn init_pool_rejects_zero_duration() {
        let mut pool = empty_pool();
        let err = init_pool(
            &mut pool,
            Pubkey::default(),
            Pubkey::default(),
            Pubkey::default(),
            Pubkey::default(),
            0,
            0,
        )
        .unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::InvalidDuration));
    }

    #[test]
    fn init_pool_rejects_negative_duration() {
        let mut pool = empty_pool();
        let err = init_pool(
            &mut pool,
            Pubkey::default(),
            Pubkey::default(),
            Pubkey::default(),
            Pubkey::default(),
            -1,
            0,
        )
        .unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::InvalidDuration));
    }

    // -----------------------------------------------------------------------
    // add_reward
    // -----------------------------------------------------------------------

    #[test]
    fn add_reward_success_slot0_uses_positive_duration_arg() {
        let mut pool = empty_pool();
        let mint = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let now = 500i64;

        add_reward(&mut pool, mint, vault, 1800, now).unwrap();

        assert_eq!(pool.rewards[0].mint, mint);
        assert_eq!(pool.rewards[0].vault, vault);
        assert_eq!(pool.rewards[0].reward_rate, 0);
        assert_eq!(pool.rewards[0].reward_per_token_stored, 0);
        assert_eq!(pool.rewards[0].period_finish, 0);
        assert_eq!(pool.rewards[0].last_update_time, now);
        assert_eq!(pool.rewards[0].duration, 1800);
        assert_eq!(pool.rewards[0].active, 1);
        assert_eq!(pool.reward_count, 1);
    }

    #[test]
    fn add_reward_zero_duration_arg_uses_pool_default() {
        let mut pool = empty_pool();
        pool.default_duration = 9999;
        let mint = Pubkey::new_unique();

        add_reward(&mut pool, mint, Pubkey::new_unique(), 0, 0).unwrap();

        assert_eq!(pool.rewards[0].duration, 9999);
    }

    #[test]
    fn add_reward_negative_duration_arg_uses_pool_default() {
        let mut pool = empty_pool();
        pool.default_duration = 7200;
        let mint = Pubkey::new_unique();

        add_reward(&mut pool, mint, Pubkey::new_unique(), -5, 0).unwrap();

        assert_eq!(pool.rewards[0].duration, 7200);
    }

    #[test]
    fn add_reward_duplicate_mint_returns_already_exists() {
        let mut pool = empty_pool();
        let mint = Pubkey::new_unique();
        add_reward(&mut pool, mint, Pubkey::new_unique(), 1, 0).unwrap();

        let err = add_reward(&mut pool, mint, Pubkey::new_unique(), 1, 0).unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::RewardAlreadyExists));
    }

    #[test]
    fn add_reward_all_slots_full_returns_slots_full() {
        let mut pool = full_pool();
        let err =
            add_reward(&mut pool, Pubkey::new_unique(), Pubkey::new_unique(), 1, 0).unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::RewardSlotsFull));
    }

    #[test]
    fn add_reward_fills_all_8_slots_sequentially() {
        let mut pool = empty_pool();
        for i in 0..MAX_REWARDS {
            add_reward(&mut pool, Pubkey::new_unique(), Pubkey::new_unique(), 1, 0).unwrap();
            assert_eq!(pool.reward_count as usize, i + 1);
        }
        // all slots occupied
        for i in 0..MAX_REWARDS {
            assert_ne!(pool.rewards[i].mint, Pubkey::default());
        }
    }

    #[test]
    fn add_reward_reward_count_overflow() {
        // Force reward_count to u8::MAX so checked_add overflows
        let mut pool = empty_pool();
        pool.reward_count = u8::MAX;
        // Slot 0 must be free (mint==default) so the slot-find succeeds
        let err =
            add_reward(&mut pool, Pubkey::new_unique(), Pubkey::new_unique(), 1, 0).unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::MathOverflow));
    }

    // -----------------------------------------------------------------------
    // set_reward_active
    // -----------------------------------------------------------------------

    #[test]
    fn set_reward_active_deactivates_slot() {
        let mut pool = pool_with_one_reward(0);
        assert_eq!(pool.rewards[0].active, 1);
        set_reward_active(&mut pool, 0, false).unwrap();
        assert_eq!(pool.rewards[0].active, 0);
    }

    #[test]
    fn set_reward_active_reactivates_slot() {
        let mut pool = pool_with_one_reward(0);
        pool.rewards[0].active = 0;
        set_reward_active(&mut pool, 0, true).unwrap();
        assert_eq!(pool.rewards[0].active, 1);
    }

    #[test]
    fn set_reward_active_out_of_range_slot_returns_not_found() {
        let mut pool = pool_with_one_reward(0);
        // slot 8 is >= MAX_REWARDS (8) — out of range
        let err = set_reward_active(&mut pool, 8, true).unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::RewardNotFound));
    }

    #[test]
    fn set_reward_active_empty_slot_returns_not_found() {
        let mut pool = empty_pool(); // all slots have mint==Pubkey::default()
        // Slot 0 is in-range but empty
        let err = set_reward_active(&mut pool, 0, true).unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::RewardNotFound));
    }

    #[test]
    fn set_reward_active_slot7_works() {
        let mut pool = empty_pool();
        pool.rewards[7].mint = Pubkey::new_unique();
        pool.rewards[7].active = 0;
        set_reward_active(&mut pool, 7, true).unwrap();
        assert_eq!(pool.rewards[7].active, 1);
    }

    // -----------------------------------------------------------------------
    // stake
    // -----------------------------------------------------------------------

    #[test]
    fn stake_success_first_use_initialises_staker() {
        let mut pool = pool_with_one_reward(0);
        let mut s = staker();
        let owner = Pubkey::new_unique();
        let pool_key = Pubkey::new_unique();

        stake(&mut pool, &mut s, owner, pool_key, 42, 100, 0).unwrap();

        assert_eq!(s.owner, owner);
        assert_eq!(s.pool, pool_key);
        assert_eq!(s.bump, 42);
        assert_eq!(s.staked_amount, 100);
        assert_eq!(pool.total_staked, 100);
    }

    #[test]
    fn stake_second_use_skips_init_branch() {
        let mut pool = pool_with_one_reward(100);
        let mut s = staker();
        let owner = Pubkey::new_unique();
        let pool_key = Pubkey::new_unique();
        // First stake sets owner
        stake(&mut pool, &mut s, owner, pool_key, 11, 100, 0).unwrap();
        assert_eq!(s.staked_amount, 100);

        // Second stake: owner != default → init branch is skipped
        let different_owner = Pubkey::new_unique();
        stake(&mut pool, &mut s, different_owner, pool_key, 99, 50, 0).unwrap();
        // owner should still be the original one (init branch was NOT executed)
        assert_eq!(s.owner, owner);
        assert_eq!(s.staked_amount, 150);
        assert_eq!(pool.total_staked, 250);
    }

    #[test]
    fn stake_zero_amount_returns_zero_amount() {
        let mut pool = pool_with_one_reward(0);
        let mut s = staker();
        let err = stake(
            &mut pool,
            &mut s,
            Pubkey::default(),
            Pubkey::default(),
            0,
            0,
            0,
        )
        .unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::ZeroAmount));
    }

    #[test]
    fn stake_paused_pool_returns_paused() {
        let mut pool = pool_with_one_reward(0);
        pool.paused = 1;
        let mut s = staker();
        let err = stake(
            &mut pool,
            &mut s,
            Pubkey::default(),
            Pubkey::default(),
            0,
            1,
            0,
        )
        .unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::Paused));
    }

    #[test]
    fn stake_staked_amount_overflow_returns_math_overflow() {
        let mut pool = empty_pool();
        let mut s = staker();
        s.owner = Pubkey::new_unique(); // mark as initialised to skip init branch
        s.staked_amount = u64::MAX;
        pool.total_staked = u64::MAX;

        // checked_add on staked_amount will overflow
        let err = stake(
            &mut pool,
            &mut s,
            Pubkey::default(),
            Pubkey::default(),
            0,
            1,
            0,
        )
        .unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::MathOverflow));
    }

    #[test]
    fn stake_total_staked_overflow_returns_math_overflow() {
        let mut pool = empty_pool();
        let mut s = staker();
        s.owner = Pubkey::new_unique();
        s.staked_amount = 0;
        // total_staked near max so adding any amount overflows
        pool.total_staked = u64::MAX;

        let err = stake(
            &mut pool,
            &mut s,
            Pubkey::default(),
            Pubkey::default(),
            0,
            1,
            0,
        )
        .unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::MathOverflow));
    }

    // -----------------------------------------------------------------------
    // unstake
    // -----------------------------------------------------------------------

    #[test]
    fn unstake_success_decrements_amounts() {
        let mut pool = pool_with_one_reward(200);
        let mut s = staker();
        s.owner = Pubkey::new_unique();
        s.staked_amount = 200;

        unstake(&mut pool, &mut s, 80, 0).unwrap();

        assert_eq!(s.staked_amount, 120);
        assert_eq!(pool.total_staked, 120);
    }

    #[test]
    fn unstake_full_amount_zeroes_staked() {
        let mut pool = pool_with_one_reward(50);
        let mut s = staker();
        s.owner = Pubkey::new_unique();
        s.staked_amount = 50;

        unstake(&mut pool, &mut s, 50, 0).unwrap();

        assert_eq!(s.staked_amount, 0);
        assert_eq!(pool.total_staked, 0);
    }

    #[test]
    fn unstake_zero_amount_returns_zero_amount() {
        let mut pool = pool_with_one_reward(100);
        let mut s = staker();
        s.staked_amount = 100;
        let err = unstake(&mut pool, &mut s, 0, 0).unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::ZeroAmount));
    }

    #[test]
    fn unstake_more_than_staked_returns_insufficient_stake() {
        let mut pool = pool_with_one_reward(50);
        let mut s = staker();
        s.staked_amount = 50;
        let err = unstake(&mut pool, &mut s, 51, 0).unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::InsufficientStake));
    }

    #[test]
    fn unstake_allowed_when_pool_paused() {
        // pause must NOT block unstake (principal always exitable)
        let mut pool = pool_with_one_reward(100);
        pool.paused = 1;
        let mut s = staker();
        s.owner = Pubkey::new_unique();
        s.staked_amount = 100;

        unstake(&mut pool, &mut s, 100, 0).unwrap(); // must not error
        assert_eq!(s.staked_amount, 0);
    }

    // -----------------------------------------------------------------------
    // deposit_rewards
    // -----------------------------------------------------------------------

    #[test]
    fn deposit_rewards_success_sets_rate_and_finish() {
        let mut pool = pool_with_one_reward(100);
        pool.rewards[0].duration = 3600;
        let mint = pool.rewards[0].mint;
        let now = 1000i64;

        deposit_rewards(&mut pool, mint, 3600, now).unwrap();

        // rate = 3600 * PRECISION / 3600 = PRECISION
        assert_eq!(pool.rewards[0].reward_rate, PRECISION);
        assert_eq!(pool.rewards[0].last_update_time, now);
        assert_eq!(pool.rewards[0].period_finish, now + 3600);
    }

    #[test]
    fn deposit_rewards_unregistered_mint_returns_not_found() {
        let mut pool = pool_with_one_reward(0);
        let err = deposit_rewards(&mut pool, Pubkey::new_unique(), 100, 0).unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::RewardNotFound));
    }

    #[test]
    fn deposit_rewards_inactive_slot_returns_reward_inactive() {
        let mut pool = pool_with_one_reward(0);
        pool.rewards[0].active = 0;
        let mint = pool.rewards[0].mint;
        let err = deposit_rewards(&mut pool, mint, 100, 0).unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::RewardInactive));
    }

    #[test]
    fn deposit_rewards_period_finish_overflow_returns_math_overflow() {
        let mut pool = pool_with_one_reward(0);
        pool.rewards[0].duration = i64::MAX; // now + duration overflows
        pool.rewards[0].active = 1;
        let mint = pool.rewards[0].mint;
        // notify_rate succeeds (duration > 0, amount small), but period_finish checked_add overflows
        let err = deposit_rewards(&mut pool, mint, 1, 1).unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::MathOverflow));
    }

    #[test]
    fn deposit_rewards_notify_rate_error_propagates() {
        // Make notify_rate itself fail: pass a leftover period active with current_rate=u128::MAX
        // so (period_finish - now) * current_rate overflows.
        // now=0, period_finish=2, current_rate=u128::MAX → remaining = 2 * u128::MAX overflows.
        let mut pool = pool_with_one_reward(0);
        pool.rewards[0].active = 1;
        pool.rewards[0].duration = 3600;
        pool.rewards[0].period_finish = 2;
        pool.rewards[0].reward_rate = u128::MAX;
        let mint = pool.rewards[0].mint;
        // notify_rate is called with now=0 < period_finish=2 → tries remaining = 2 * u128::MAX → overflow
        let err = deposit_rewards(&mut pool, mint, 1, 0).unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::MathOverflow));
    }

    // -----------------------------------------------------------------------
    // emergency_withdraw
    // -----------------------------------------------------------------------

    #[test]
    fn emergency_withdraw_returns_staked_amount_and_zeroes_state() {
        let mut pool = pool_with_one_reward(500);
        pool.rewards[0].reward_per_token_stored = 7 * PRECISION;
        let mut s = staker();
        s.staked_amount = 500;
        s.entries[0].rewards_accrued = 999;
        s.entries[0].reward_per_token_paid = 3 * PRECISION;

        let amt = emergency_withdraw(&mut pool, &mut s).unwrap();

        assert_eq!(amt, 500);
        assert_eq!(s.staked_amount, 0);
        assert_eq!(pool.total_staked, 0);
        // rewards forfeited
        assert_eq!(s.entries[0].rewards_accrued, 0);
        // rpt_paid synced to pool's rpt_stored
        assert_eq!(s.entries[0].reward_per_token_paid, 7 * PRECISION);
    }

    #[test]
    fn emergency_withdraw_zero_stake_returns_zero() {
        let mut pool = pool_with_one_reward(0);
        let mut s = staker();
        s.staked_amount = 0;

        let amt = emergency_withdraw(&mut pool, &mut s).unwrap();
        assert_eq!(amt, 0);
        assert_eq!(pool.total_staked, 0);
    }

    #[test]
    fn emergency_withdraw_saturating_sub_does_not_underflow() {
        // total_staked is less than staker.staked_amount (edge case)
        let mut pool = pool_with_one_reward(10);
        let mut s = staker();
        s.staked_amount = 999; // larger than pool.total_staked (10)

        let amt = emergency_withdraw(&mut pool, &mut s).unwrap();
        assert_eq!(amt, 999);
        assert_eq!(pool.total_staked, 0); // saturating_sub(999) clamped to 0
    }

    #[test]
    fn emergency_withdraw_syncs_all_reward_slots() {
        let mut pool = empty_pool();
        // Register 3 reward slots with distinct rpt values
        for i in 0..3 {
            pool.rewards[i].mint = Pubkey::new_unique();
            pool.rewards[i].reward_per_token_stored = (i as u128 + 1) * PRECISION;
        }
        pool.total_staked = 100;
        let mut s = staker();
        s.staked_amount = 100;
        for i in 0..3 {
            s.entries[i].rewards_accrued = (i as u64 + 1) * 1000;
        }

        emergency_withdraw(&mut pool, &mut s).unwrap();

        for i in 0..3 {
            assert_eq!(s.entries[i].rewards_accrued, 0);
            assert_eq!(
                s.entries[i].reward_per_token_paid,
                (i as u128 + 1) * PRECISION
            );
        }
        // Slots 3..MAX_REWARDS had mint==default and rpt_stored==0
        for i in 3..MAX_REWARDS {
            assert_eq!(s.entries[i].reward_per_token_paid, 0);
        }
    }

    // -----------------------------------------------------------------------
    // set_paused
    // -----------------------------------------------------------------------

    #[test]
    fn set_paused_true_sets_paused_to_1() {
        let mut pool = empty_pool();
        set_paused(&mut pool, true).unwrap();
        assert_eq!(pool.paused, 1);
    }

    #[test]
    fn set_paused_false_sets_paused_to_0() {
        let mut pool = empty_pool();
        pool.paused = 1;
        set_paused(&mut pool, false).unwrap();
        assert_eq!(pool.paused, 0);
    }

    // -----------------------------------------------------------------------
    // set_keeper_authority
    // -----------------------------------------------------------------------

    #[test]
    fn set_keeper_authority_updates_field() {
        let mut pool = empty_pool();
        let new_keeper = Pubkey::new_unique();
        set_keeper_authority(&mut pool, new_keeper).unwrap();
        assert_eq!(pool.keeper_authority, new_keeper);
    }

    // -----------------------------------------------------------------------
    // set_admin
    // -----------------------------------------------------------------------

    #[test]
    fn set_admin_updates_field() {
        let mut pool = empty_pool();
        let new_admin = Pubkey::new_unique();
        set_admin(&mut pool, new_admin).unwrap();
        assert_eq!(pool.admin, new_admin);
    }

    // -----------------------------------------------------------------------
    // set_duration
    // -----------------------------------------------------------------------

    #[test]
    fn set_duration_success_updates_slot_duration() {
        let mut pool = pool_with_one_reward(0);
        set_duration(&mut pool, 0, 9999).unwrap();
        assert_eq!(pool.rewards[0].duration, 9999);
    }

    #[test]
    fn set_duration_zero_duration_returns_invalid_duration() {
        let mut pool = pool_with_one_reward(0);
        let err = set_duration(&mut pool, 0, 0).unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::InvalidDuration));
    }

    #[test]
    fn set_duration_negative_duration_returns_invalid_duration() {
        let mut pool = pool_with_one_reward(0);
        let err = set_duration(&mut pool, 0, -1).unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::InvalidDuration));
    }

    #[test]
    fn set_duration_out_of_range_slot_returns_not_found() {
        let mut pool = pool_with_one_reward(0);
        // slot 8 is out of range
        let err = set_duration(&mut pool, 8, 100).unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::RewardNotFound));
    }

    #[test]
    fn set_duration_empty_slot_returns_not_found() {
        let mut pool = empty_pool(); // slot 0 has mint==default
        let err = set_duration(&mut pool, 0, 100).unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::RewardNotFound));
    }

    #[test]
    fn set_duration_slot7_works() {
        let mut pool = empty_pool();
        pool.rewards[7].mint = Pubkey::new_unique();
        set_duration(&mut pool, 7, 1234).unwrap();
        assert_eq!(pool.rewards[7].duration, 1234);
    }

    // -----------------------------------------------------------------------
    // claim_settle_all
    // -----------------------------------------------------------------------

    #[test]
    fn claim_settle_all_ok_with_no_rewards() {
        let mut pool = empty_pool();
        let mut s = staker();
        // No rewards registered — update_reward simply skips all slots
        claim_settle_all(&mut pool, &mut s, 100).unwrap();
    }

    #[test]
    fn claim_settle_all_accrues_rewards() {
        // Set up: pool with 1 reward, rate=PRECISION, 100s period, 10 staked
        let mut pool = pool_with_one_reward(10);
        pool.rewards[0].reward_rate = PRECISION;
        pool.rewards[0].period_finish = 1000;
        pool.rewards[0].last_update_time = 0;

        let mut s = staker();
        s.owner = Pubkey::new_unique();
        s.staked_amount = 10;

        // Settle at t=100: elapsed=100, rpt_delta = 100*PRECISION/10 = 10*PRECISION
        // earned = 10 * 10*PRECISION / PRECISION = 100
        claim_settle_all(&mut pool, &mut s, 100).unwrap();

        assert_eq!(s.entries[0].rewards_accrued, 100);
        assert_eq!(pool.rewards[0].reward_per_token_stored, 10 * PRECISION);
    }

    #[test]
    fn claim_settle_all_advances_rpt_even_after_period_finish() {
        let mut pool = pool_with_one_reward(10);
        pool.rewards[0].reward_rate = PRECISION;
        pool.rewards[0].period_finish = 50;
        pool.rewards[0].last_update_time = 0;

        let mut s = staker();
        s.owner = Pubkey::new_unique();
        s.staked_amount = 10;

        // now=200 > period_finish=50 → applicable=50; earned = 10*(5*PRECISION)/PRECISION = 50
        claim_settle_all(&mut pool, &mut s, 200).unwrap();
        assert_eq!(s.entries[0].rewards_accrued, 50);
    }

    // -----------------------------------------------------------------------
    // claim_take_slot
    // -----------------------------------------------------------------------

    #[test]
    fn claim_take_slot_returns_idx_and_amount_then_zeroes() {
        let pool = pool_with_one_reward(0);
        let vault = pool.rewards[0].vault;
        let mut s = staker();
        s.entries[0].rewards_accrued = 4242;

        let (idx, amt) = claim_take_slot(&pool, &mut s, &vault).unwrap();

        assert_eq!(idx, 0);
        assert_eq!(amt, 4242);
        assert_eq!(s.entries[0].rewards_accrued, 0);
    }

    #[test]
    fn claim_take_slot_non_matching_vault_returns_vault_mismatch() {
        let pool = pool_with_one_reward(0);
        let mut s = staker();
        let wrong_vault = Pubkey::new_unique();
        let err = claim_take_slot(&pool, &mut s, &wrong_vault).unwrap_err();
        assert_eq!(err, anchor_lang::error!(StakingError::VaultMismatch));
    }

    #[test]
    fn claim_take_slot_zero_accrued_returns_zero() {
        let pool = pool_with_one_reward(0);
        let vault = pool.rewards[0].vault;
        let mut s = staker();
        s.entries[0].rewards_accrued = 0;

        let (idx, amt) = claim_take_slot(&pool, &mut s, &vault).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(amt, 0);
    }

    #[test]
    fn claim_take_slot_matches_slot7() {
        let mut pool = empty_pool();
        pool.rewards[7].mint = Pubkey::new_unique();
        pool.rewards[7].vault = Pubkey::new_unique();
        let vault7 = pool.rewards[7].vault;
        let mut s = staker();
        s.entries[7].rewards_accrued = 777;

        let (idx, amt) = claim_take_slot(&pool, &mut s, &vault7).unwrap();
        assert_eq!(idx, 7);
        assert_eq!(amt, 777);
        assert_eq!(s.entries[7].rewards_accrued, 0);
    }

    // -----------------------------------------------------------------------
    // Integration: stake → deposit_rewards → claim_settle_all → claim_take_slot
    // -----------------------------------------------------------------------

    #[test]
    fn full_stake_reward_claim_flow() {
        let mut pool = empty_pool();
        let mint = Pubkey::new_unique();
        let vault = Pubkey::new_unique();

        // 1. Add reward slot
        add_reward(&mut pool, mint, vault, 3600, 0).unwrap();

        // 2. Stake 100 tokens at t=0
        let mut s = staker();
        let owner = Pubkey::new_unique();
        stake(&mut pool, &mut s, owner, Pubkey::new_unique(), 1, 100, 0).unwrap();
        assert_eq!(pool.total_staked, 100);

        // 3. Deposit 3600 reward tokens at t=0 (rate = PRECISION/s)
        deposit_rewards(&mut pool, mint, 3600, 0).unwrap();
        assert_eq!(pool.rewards[0].reward_rate, PRECISION);
        assert_eq!(pool.rewards[0].period_finish, 3600);

        // 4. Settle at t=1800 (half-period)
        claim_settle_all(&mut pool, &mut s, 1800).unwrap();
        // elapsed=1800, rpt_delta=1800*PRECISION/100=18*PRECISION
        // earned=100*18*PRECISION/PRECISION=1800
        assert_eq!(s.entries[0].rewards_accrued, 1800);

        // 5. Claim the slot
        let (idx, amt) = claim_take_slot(&pool, &mut s, &vault).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(amt, 1800);
        assert_eq!(s.entries[0].rewards_accrued, 0);

        // 6. Unstake
        unstake(&mut pool, &mut s, 100, 1800).unwrap();
        assert_eq!(pool.total_staked, 0);
        assert_eq!(s.staked_amount, 0);
    }
}
