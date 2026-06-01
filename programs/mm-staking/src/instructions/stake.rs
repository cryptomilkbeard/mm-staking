use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};
use crate::constants::*;
use crate::errors::StakingError;
use crate::state::{Pool, StakerAccount};
use crate::update::update_reward;

#[derive(Accounts)]
pub struct Stake<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,

    #[account(mut, seeds = [POOL_SEED, stake_mint.key().as_ref()], bump = pool.load()?.bump)]
    pub pool: AccountLoader<'info, Pool>,

    #[account(
        init_if_needed,
        payer = owner,
        space = StakerAccount::LEN,
        seeds = [STAKER_SEED, pool.key().as_ref(), owner.key().as_ref()],
        bump,
    )]
    pub staker: AccountLoader<'info, StakerAccount>,

    #[account(address = pool.load()?.stake_mint)]
    pub stake_mint: Account<'info, Mint>,

    #[account(mut, token::mint = stake_mint, token::authority = owner)]
    pub user_token_account: Account<'info, TokenAccount>,

    #[account(mut, address = pool.load()?.stake_vault)]
    pub stake_vault: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

pub fn stake_handler(ctx: Context<Stake>, amount: u64) -> Result<()> {
    require!(amount > 0, StakingError::ZeroAmount);
    let now = Clock::get()?.unix_timestamp;
    {
        let mut pool = ctx.accounts.pool.load_mut()?;
        require!(pool.paused == 0, StakingError::Paused);

        // init staker fields on first use
        let mut staker = match ctx.accounts.staker.load_mut() {
            Ok(s) => s,
            Err(_) => ctx.accounts.staker.load_init()?,
        };
        if staker.owner == Pubkey::default() {
            staker.owner = ctx.accounts.owner.key();
            staker.pool = ctx.accounts.pool.key();
            staker.bump = ctx.bumps.staker;
        }
        update_reward(&mut pool, Some(&mut staker), now)?;
        staker.staked_amount = staker
            .staked_amount
            .checked_add(amount)
            .ok_or_else(|| error!(StakingError::MathOverflow))?;
        pool.total_staked = pool
            .total_staked
            .checked_add(amount)
            .ok_or_else(|| error!(StakingError::MathOverflow))?;
    }
    token::transfer(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.user_token_account.to_account_info(),
                to: ctx.accounts.stake_vault.to_account_info(),
                authority: ctx.accounts.owner.to_account_info(),
            },
        ),
        amount,
    )
}

#[derive(Accounts)]
pub struct Unstake<'info> {
    pub owner: Signer<'info>,

    #[account(mut, seeds = [POOL_SEED, stake_mint.key().as_ref()], bump = pool.load()?.bump)]
    pub pool: AccountLoader<'info, Pool>,

    #[account(
        mut,
        seeds = [STAKER_SEED, pool.key().as_ref(), owner.key().as_ref()],
        bump = staker.load()?.bump,
        has_one = owner @ StakingError::Unauthorized,
    )]
    pub staker: AccountLoader<'info, StakerAccount>,

    #[account(address = pool.load()?.stake_mint)]
    pub stake_mint: Account<'info, Mint>,

    #[account(mut, token::mint = stake_mint, token::authority = owner)]
    pub user_token_account: Account<'info, TokenAccount>,

    #[account(mut, address = pool.load()?.stake_vault)]
    pub stake_vault: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

pub fn unstake_handler(ctx: Context<Unstake>, amount: u64) -> Result<()> {
    require!(amount > 0, StakingError::ZeroAmount);
    let now = Clock::get()?.unix_timestamp;
    let stake_mint = ctx.accounts.stake_mint.key();
    let pool_bump = ctx.accounts.pool.load()?.bump;
    {
        let mut pool = ctx.accounts.pool.load_mut()?;
        // unstake is allowed even when paused (principal must stay exitable)
        let mut staker = ctx.accounts.staker.load_mut()?;
        require!(staker.staked_amount >= amount, StakingError::InsufficientStake);
        update_reward(&mut pool, Some(&mut staker), now)?;
        staker.staked_amount -= amount;
        pool.total_staked -= amount;
    }
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
