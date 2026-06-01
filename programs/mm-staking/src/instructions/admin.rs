use anchor_lang::prelude::*;
use crate::constants::*;
use crate::errors::StakingError;
use crate::state::Pool;

#[derive(Accounts)]
pub struct AdminOnly<'info> {
    #[account(address = pool.load()?.admin @ StakingError::Unauthorized)]
    pub admin: Signer<'info>,
    #[account(mut)]
    pub pool: AccountLoader<'info, Pool>,
}

pub fn set_paused(ctx: Context<AdminOnly>, paused: bool) -> Result<()> {
    ctx.accounts.pool.load_mut()?.paused = if paused { 1 } else { 0 };
    Ok(())
}

pub fn set_keeper_authority(ctx: Context<AdminOnly>, keeper: Pubkey) -> Result<()> {
    ctx.accounts.pool.load_mut()?.keeper_authority = keeper;
    Ok(())
}

pub fn set_admin(ctx: Context<AdminOnly>, new_admin: Pubkey) -> Result<()> {
    ctx.accounts.pool.load_mut()?.admin = new_admin;
    Ok(())
}

pub fn set_duration(ctx: Context<AdminOnly>, slot: u8, duration: i64) -> Result<()> {
    require!(duration > 0, StakingError::InvalidDuration);
    let mut pool = ctx.accounts.pool.load_mut()?;
    let i = slot as usize;
    require!(i < MAX_REWARDS && pool.rewards[i].mint != Pubkey::default(), StakingError::RewardNotFound);
    pool.rewards[i].duration = duration;
    Ok(())
}
