use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};
use crate::constants::*;
use crate::errors::StakingError;
use crate::state::{Pool, StakerAccount};
use crate::update::update_reward;

#[derive(Accounts)]
pub struct Claim<'info> {
    pub owner: Signer<'info>,

    #[account(mut, seeds = [POOL_SEED, pool.load()?.stake_mint.as_ref()], bump = pool.load()?.bump)]
    pub pool: AccountLoader<'info, Pool>,

    #[account(mut, seeds = [STAKER_SEED, pool.key().as_ref(), owner.key().as_ref()], bump = staker.load()?.bump,
        has_one = owner @ StakingError::Unauthorized)]
    pub staker: AccountLoader<'info, StakerAccount>,

    pub token_program: Program<'info, Token>,
    // remaining_accounts: repeating (reward_vault, user_token_account) pairs
}

pub fn handler<'info>(ctx: Context<'_, '_, 'info, 'info, Claim<'info>>) -> Result<()> {
    let now = Clock::get()?.unix_timestamp;
    let stake_mint = ctx.accounts.pool.load()?.stake_mint;
    let pool_bump = ctx.accounts.pool.load()?.bump;

    {
        let mut pool = ctx.accounts.pool.load_mut()?;
        let mut staker = ctx.accounts.staker.load_mut()?;
        update_reward(&mut pool, Some(&mut staker), now)?;
    }

    let remaining = ctx.remaining_accounts;
    require!(remaining.len().is_multiple_of(2), StakingError::VaultMismatch);
    let seeds: &[&[u8]] = &[POOL_SEED, stake_mint.as_ref(), &[pool_bump]];

    for pair in remaining.chunks(2) {
        let vault_ai = &pair[0];
        let user_ai = &pair[1];

        // resolve slot by vault, read accrued, zero it
        let (amount, vault_key) = {
            let pool = ctx.accounts.pool.load_mut()?;
            let idx = (0..MAX_REWARDS).find(|&i| pool.rewards[i].vault == *vault_ai.key);
            let idx = idx.ok_or_else(|| error!(StakingError::VaultMismatch))?;
            let mut staker = ctx.accounts.staker.load_mut()?;
            let amt = staker.entries[idx].rewards_accrued;
            staker.entries[idx].rewards_accrued = 0;
            (amt, pool.rewards[idx].vault)
        };
        if amount == 0 {
            continue;
        }
        require_keys_eq!(*vault_ai.key, vault_key, StakingError::VaultMismatch);

        let vault_acc: Account<TokenAccount> = Account::try_from(vault_ai)?;
        let user_acc: Account<TokenAccount> = Account::try_from(user_ai)?;
        require_keys_eq!(user_acc.owner, ctx.accounts.owner.key(), StakingError::Unauthorized);
        require_keys_eq!(user_acc.mint, vault_acc.mint, StakingError::MintMismatch);

        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: vault_ai.clone(),
                    to: user_ai.clone(),
                    authority: ctx.accounts.pool.to_account_info(),
                },
                &[seeds],
            ),
            amount,
        )?;
    }
    Ok(())
}
