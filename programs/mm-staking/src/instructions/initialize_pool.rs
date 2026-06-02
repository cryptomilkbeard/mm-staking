use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};
use crate::constants::*;
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
    let admin = ctx.accounts.admin.key();
    let stake_mint = ctx.accounts.stake_mint.key();
    let stake_vault = ctx.accounts.stake_vault.key();
    let bump = ctx.bumps.pool;
    let mut pool = ctx.accounts.pool.load_init()?;
    crate::logic::init_pool(
        &mut pool,
        admin,
        keeper_authority,
        stake_mint,
        stake_vault,
        default_duration,
        bump,
    )
}
