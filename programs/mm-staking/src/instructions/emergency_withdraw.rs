use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};
use crate::constants::*;
use crate::errors::StakingError;
use crate::state::{Pool, StakerAccount};

#[derive(Accounts)]
pub struct EmergencyWithdraw<'info> {
    pub owner: Signer<'info>,

    #[account(mut, seeds = [POOL_SEED, stake_mint.key().as_ref()], bump = pool.load()?.bump)]
    pub pool: AccountLoader<'info, Pool>,

    #[account(mut, seeds = [STAKER_SEED, pool.key().as_ref(), owner.key().as_ref()], bump = staker.load()?.bump,
        has_one = owner @ StakingError::Unauthorized)]
    pub staker: AccountLoader<'info, StakerAccount>,

    #[account(address = pool.load()?.stake_mint)]
    pub stake_mint: Account<'info, Mint>,

    #[account(mut, token::mint = stake_mint, token::authority = owner)]
    pub user_token_account: Account<'info, TokenAccount>,

    #[account(mut, address = pool.load()?.stake_vault)]
    pub stake_vault: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

/// Returns staked MM principal regardless of pause / reward state. Forfeits unclaimed rewards.
/// Deliberately does NOT call update_reward — must work even if reward math is broken.
pub fn handler(ctx: Context<EmergencyWithdraw>) -> Result<()> {
    let stake_mint = ctx.accounts.stake_mint.key();
    let pool_bump = ctx.accounts.pool.load()?.bump;
    let amount = {
        let mut pool = ctx.accounts.pool.load_mut()?;
        let mut staker = ctx.accounts.staker.load_mut()?;
        crate::logic::emergency_withdraw(&mut pool, &mut staker)?
    };
    require!(amount > 0, StakingError::ZeroAmount);
    let seeds: &[&[u8]] = &[POOL_SEED, stake_mint.as_ref(), &[pool_bump]];
    token::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.stake_vault.to_account_info(),
                to: ctx.accounts.user_token_account.to_account_info(),
                authority: ctx.accounts.pool.to_account_info(),
            },
            &[seeds],
        ),
        amount,
    )
}
