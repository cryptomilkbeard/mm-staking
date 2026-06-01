use anchor_lang::prelude::*;

pub mod constants;
pub mod errors;
pub mod state;
pub mod math;
pub mod update;
pub mod instructions;

use instructions::*;

declare_id!("1Zx9vyjZLMJqsFyZxraPBww4SrSPXwHt7HFbtwpfCmA");

#[program]
pub mod mm_staking {
    use super::*;

    pub fn initialize_pool(
        ctx: Context<InitializePool>,
        default_duration: i64,
        keeper_authority: Pubkey,
    ) -> Result<()> {
        instructions::initialize_pool::handler(ctx, default_duration, keeper_authority)
    }

    pub fn add_reward(ctx: Context<AddReward>, duration: i64) -> Result<()> {
        instructions::add_reward::handler(ctx, duration)
    }

    pub fn set_reward_active(ctx: Context<SetRewardActive>, slot: u8, active: bool) -> Result<()> {
        instructions::add_reward::set_active_handler(ctx, slot, active)
    }

    pub fn stake(ctx: Context<Stake>, amount: u64) -> Result<()> {
        instructions::stake::stake_handler(ctx, amount)
    }

    pub fn unstake(ctx: Context<Unstake>, amount: u64) -> Result<()> {
        instructions::stake::unstake_handler(ctx, amount)
    }

    pub fn deposit_rewards(ctx: Context<DepositRewards>, amount: u64) -> Result<()> {
        instructions::deposit_rewards::handler(ctx, amount)
    }

    pub fn claim<'info>(ctx: Context<'_, '_, 'info, 'info, Claim<'info>>) -> Result<()> {
        instructions::claim::handler(ctx)
    }

    pub fn emergency_withdraw(ctx: Context<EmergencyWithdraw>) -> Result<()> {
        instructions::emergency_withdraw::handler(ctx)
    }
}
