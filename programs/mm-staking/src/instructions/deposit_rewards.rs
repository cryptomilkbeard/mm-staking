use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};
use crate::constants::*;
use crate::errors::StakingError;
use crate::math::notify_rate;
use crate::state::Pool;
use crate::update::{find_slot, update_reward};

#[derive(Accounts)]
pub struct DepositRewards<'info> {
    #[account(address = pool.load()?.keeper_authority @ StakingError::Unauthorized)]
    pub keeper: Signer<'info>,

    #[account(mut, seeds = [POOL_SEED, pool.load()?.stake_mint.as_ref()], bump = pool.load()?.bump)]
    pub pool: AccountLoader<'info, Pool>,

    pub reward_mint: Account<'info, Mint>,

    #[account(mut, token::mint = reward_mint, token::authority = keeper)]
    pub keeper_token_account: Account<'info, TokenAccount>,

    #[account(mut, token::mint = reward_mint, address = find_reward_vault(&pool, &reward_mint.key())?)]
    pub reward_vault: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

fn find_reward_vault(pool: &AccountLoader<Pool>, mint: &Pubkey) -> Result<Pubkey> {
    let p = pool.load()?;
    let i = find_slot(&p, mint).ok_or_else(|| error!(StakingError::RewardNotFound))?;
    Ok(p.rewards[i].vault)
}

pub fn handler(ctx: Context<DepositRewards>, amount: u64) -> Result<()> {
    require!(amount > 0, StakingError::ZeroAmount);
    let now = Clock::get()?.unix_timestamp;
    let mint = ctx.accounts.reward_mint.key();

    let before = ctx.accounts.reward_vault.amount;
    token::transfer(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.keeper_token_account.to_account_info(),
                to: ctx.accounts.reward_vault.to_account_info(),
                authority: ctx.accounts.keeper.to_account_info(),
            },
        ),
        amount,
    )?;
    ctx.accounts.reward_vault.reload()?;
    let received = ctx.accounts.reward_vault.amount.checked_sub(before).ok_or_else(|| error!(StakingError::MathOverflow))?;

    let mut pool = ctx.accounts.pool.load_mut()?;
    let i = find_slot(&pool, &mint).ok_or_else(|| error!(StakingError::RewardNotFound))?;
    require!(pool.rewards[i].active == 1, StakingError::RewardInactive);

    // settle globals to now, then re-rate
    update_reward(&mut pool, None, now)?;
    let duration = pool.rewards[i].duration;
    let new_rate = notify_rate(received, pool.rewards[i].reward_rate, now, pool.rewards[i].period_finish, duration)?;
    pool.rewards[i].reward_rate = new_rate;
    pool.rewards[i].last_update_time = now;
    pool.rewards[i].period_finish = now.checked_add(duration).ok_or_else(|| error!(StakingError::MathOverflow))?;
    Ok(())
}
