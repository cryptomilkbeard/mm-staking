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
