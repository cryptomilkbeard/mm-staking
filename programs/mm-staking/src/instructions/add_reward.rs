use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};
use crate::constants::*;
use crate::errors::StakingError;
use crate::state::Pool;

#[derive(Accounts)]
pub struct AddReward<'info> {
    #[account(mut, address = pool.load()?.admin @ StakingError::Unauthorized)]
    pub admin: Signer<'info>,

    #[account(mut, seeds = [POOL_SEED, pool.load()?.stake_mint.as_ref()], bump = pool.load()?.bump)]
    pub pool: AccountLoader<'info, Pool>,

    pub reward_mint: Account<'info, Mint>,

    #[account(
        init,
        payer = admin,
        seeds = [REWARD_VAULT_SEED, pool.key().as_ref(), reward_mint.key().as_ref()],
        bump,
        token::mint = reward_mint,
        token::authority = pool,
    )]
    pub reward_vault: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

pub fn handler(ctx: Context<AddReward>, duration: i64) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let mint = ctx.accounts.reward_mint.key();
    let vault = ctx.accounts.reward_vault.key();
    let mut pool = ctx.accounts.pool.load_mut()?;
    crate::logic::add_reward(&mut pool, mint, vault, duration, now)
}

#[derive(Accounts)]
pub struct SetRewardActive<'info> {
    #[account(address = pool.load()?.admin @ StakingError::Unauthorized)]
    pub admin: Signer<'info>,
    #[account(mut, seeds = [POOL_SEED, pool.load()?.stake_mint.as_ref()], bump = pool.load()?.bump)]
    pub pool: AccountLoader<'info, Pool>,
}

pub fn set_active_handler(ctx: Context<SetRewardActive>, slot: u8, active: bool) -> Result<()> {
    let mut pool = ctx.accounts.pool.load_mut()?;
    crate::logic::set_reward_active(&mut pool, slot, active)
}
