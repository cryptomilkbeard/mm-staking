use anchor_lang::prelude::*;
use crate::constants::*;
use crate::errors::StakingError;
use crate::state::Pool;

#[derive(Accounts)]
pub struct AdminOnly<'info> {
    #[account(address = pool.load()?.admin @ StakingError::Unauthorized)]
    pub admin: Signer<'info>,
    #[account(mut, seeds = [POOL_SEED, pool.load()?.stake_mint.as_ref()], bump = pool.load()?.bump)]
    pub pool: AccountLoader<'info, Pool>,
}

pub fn set_paused(ctx: Context<AdminOnly>, paused: bool) -> Result<()> {
    let mut pool = ctx.accounts.pool.load_mut()?;
    crate::logic::set_paused(&mut pool, paused)
}

pub fn set_keeper_authority(ctx: Context<AdminOnly>, keeper: Pubkey) -> Result<()> {
    let mut pool = ctx.accounts.pool.load_mut()?;
    crate::logic::set_keeper_authority(&mut pool, keeper)
}

pub fn set_admin(ctx: Context<AdminOnly>, new_admin: Pubkey) -> Result<()> {
    let mut pool = ctx.accounts.pool.load_mut()?;
    crate::logic::set_admin(&mut pool, new_admin)
}

pub fn set_duration(ctx: Context<AdminOnly>, slot: u8, duration: i64) -> Result<()> {
    let mut pool = ctx.accounts.pool.load_mut()?;
    crate::logic::set_duration(&mut pool, slot, duration)
}
