use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};
use crate::constants::*;
use crate::errors::StakingError;
use crate::state::Pool;
use crate::update::find_slot;

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
    let mut pool = ctx.accounts.pool.load_mut()?;
    let mint = ctx.accounts.reward_mint.key();
    require!(find_slot(&pool, &mint).is_none(), StakingError::RewardAlreadyExists);
    let duration = if duration > 0 { duration } else { pool.default_duration };

    let free = (0..MAX_REWARDS).find(|&i| pool.rewards[i].mint == Pubkey::default());
    let idx = free.ok_or_else(|| error!(StakingError::RewardSlotsFull))?;

    pool.rewards[idx].mint = mint;
    pool.rewards[idx].vault = ctx.accounts.reward_vault.key();
    pool.rewards[idx].reward_rate = 0;
    pool.rewards[idx].reward_per_token_stored = 0;
    pool.rewards[idx].period_finish = 0;
    pool.rewards[idx].last_update_time = Clock::get()?.unix_timestamp;
    pool.rewards[idx].duration = duration;
    pool.rewards[idx].active = 1;
    pool.reward_count = pool.reward_count.checked_add(1).ok_or_else(|| error!(StakingError::MathOverflow))?;
    Ok(())
}

#[derive(Accounts)]
pub struct SetRewardActive<'info> {
    #[account(address = pool.load()?.admin @ StakingError::Unauthorized)]
    pub admin: Signer<'info>,
    #[account(mut)]
    pub pool: AccountLoader<'info, Pool>,
}

pub fn set_active_handler(ctx: Context<SetRewardActive>, slot: u8, active: bool) -> Result<()> {
    let mut pool = ctx.accounts.pool.load_mut()?;
    let i = slot as usize;
    require!(i < MAX_REWARDS && pool.rewards[i].mint != Pubkey::default(), StakingError::RewardNotFound);
    pool.rewards[i].active = if active { 1 } else { 0 };
    Ok(())
}
