use anchor_lang::prelude::*;

pub const MAX_REWARDS: usize = 8;
pub const PRECISION: u128 = 1_000_000_000_000; // 1e12
pub const DEFAULT_DURATION: i64 = 3600; // 1 hour

// PDA seeds
#[constant]
pub const POOL_SEED: &[u8] = b"pool";
#[constant]
pub const STAKE_VAULT_SEED: &[u8] = b"stake_vault";
#[constant]
pub const REWARD_VAULT_SEED: &[u8] = b"reward_vault";
#[constant]
pub const STAKER_SEED: &[u8] = b"staker";
