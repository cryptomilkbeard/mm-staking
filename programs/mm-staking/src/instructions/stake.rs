use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};
use crate::constants::*;
use crate::errors::StakingError;
use crate::state::{Pool, StakerAccount};

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
    // `amount > 0` MUST be checked before the init_if_needed staker dance so a
    // zero-amount call does not initialize the staker PDA (behavior preserved).
    require!(amount > 0, StakingError::ZeroAmount);
    let now = Clock::get()?.unix_timestamp;
    let owner = ctx.accounts.owner.key();
    let pool_key = ctx.accounts.pool.key();
    let staker_bump = ctx.bumps.staker;
    {
        let mut pool = ctx.accounts.pool.load_mut()?;
        // `paused` MUST be checked before the init_if_needed staker dance so a
        // paused-pool stake does not initialize the staker PDA (behavior
        // preserved). `logic::stake` re-checks it (redundant but self-contained).
        require!(pool.paused == 0, StakingError::Paused);

        // init staker fields on first use
        let mut staker = match ctx.accounts.staker.load_mut() {
            Ok(s) => s,
            Err(_) => ctx.accounts.staker.load_init()?,
        };
        crate::logic::stake(
            &mut pool,
            &mut staker,
            owner,
            pool_key,
            staker_bump,
            amount,
            now,
        )?;
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
    let now = Clock::get()?.unix_timestamp;
    let stake_mint = ctx.accounts.stake_mint.key();
    let pool_bump = ctx.accounts.pool.load()?.bump;
    {
        let mut pool = ctx.accounts.pool.load_mut()?;
        // unstake is allowed even when paused (principal must stay exitable)
        let mut staker = ctx.accounts.staker.load_mut()?;
        crate::logic::unstake(&mut pool, &mut staker, amount, now)?;
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
