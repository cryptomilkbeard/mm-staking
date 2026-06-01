use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};
use crate::constants::*;
use crate::errors::StakingError;
use crate::state::Pool;

#[derive(Accounts)]
pub struct InitializePool<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,

    pub stake_mint: Account<'info, Mint>,

    #[account(
        init,
        payer = admin,
        space = Pool::LEN,
        seeds = [POOL_SEED, stake_mint.key().as_ref()],
        bump,
    )]
    pub pool: AccountLoader<'info, Pool>,

    #[account(
        init,
        payer = admin,
        seeds = [STAKE_VAULT_SEED, pool.key().as_ref()],
        bump,
        token::mint = stake_mint,
        token::authority = pool,
    )]
    pub stake_vault: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

pub fn handler(ctx: Context<InitializePool>, default_duration: i64, keeper_authority: Pubkey) -> Result<()> {
    require!(default_duration > 0, StakingError::InvalidDuration);
    let mut pool = ctx.accounts.pool.load_init()?;
    pool.admin = ctx.accounts.admin.key();
    pool.keeper_authority = keeper_authority;
    pool.stake_mint = ctx.accounts.stake_mint.key();
    pool.stake_vault = ctx.accounts.stake_vault.key();
    pool.total_staked = 0;
    pool.default_duration = default_duration;
    pool.reward_count = 0;
    pool.paused = 0;
    pool.bump = ctx.bumps.pool;
    Ok(())
}
