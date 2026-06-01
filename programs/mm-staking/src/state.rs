use anchor_lang::prelude::*;
use crate::constants::MAX_REWARDS;

#[zero_copy]
#[derive(Default)]
#[repr(C)]
pub struct RewardInfo {
    pub mint: Pubkey,                  // 32 — Pubkey::default() means empty slot
    pub vault: Pubkey,                 // 32
    pub reward_rate: u128,             // 16 — scaled by PRECISION (scaled-tokens/sec)
    pub reward_per_token_stored: u128, // 16 — scaled by PRECISION
    pub period_finish: i64,            // 8
    pub last_update_time: i64,         // 8
    pub duration: i64,                 // 8
    pub active: u8,                    // 1 — accepts deposits (accrual still settles after deactivation)
    pub _pad: [u8; 7],                 // 7
}

#[zero_copy]
#[derive(Default)]
#[repr(C)]
pub struct RewardEntry {
    pub reward_per_token_paid: u128, // 16
    pub rewards_accrued: u64,        // 8
    pub _pad: [u8; 8],               // 8
}

#[account(zero_copy)]
#[repr(C)]
pub struct Pool {
    pub admin: Pubkey,                       // 32
    pub keeper_authority: Pubkey,            // 32
    pub stake_mint: Pubkey,                  // 32
    pub stake_vault: Pubkey,                 // 32
    pub total_staked: u64,                   // 8
    pub default_duration: i64,               // 8
    pub reward_count: u8,                    // 1
    pub paused: u8,                          // 1
    pub bump: u8,                            // 1
    pub _pad: [u8; 13],                      // 13 — align rewards to 16-byte boundary (offset 160)
    pub rewards: [RewardInfo; MAX_REWARDS],  // 8 * 128 = 1024
}

#[account(zero_copy)]
#[repr(C)]
pub struct StakerAccount {
    pub owner: Pubkey,                      // 32
    pub pool: Pubkey,                       // 32
    pub staked_amount: u64,                 // 8
    pub bump: u8,                           // 1
    pub _pad: [u8; 7],                      // 7
    pub entries: [RewardEntry; MAX_REWARDS],// 8 * 32 = 256
}

impl Pool {
    pub const LEN: usize = 8 + 32 + 32 + 32 + 32 + 8 + 8 + 1 + 1 + 1 + 13 + (128 * MAX_REWARDS);
}

impl StakerAccount {
    pub const LEN: usize = 8 + 32 + 32 + 8 + 1 + 7 + (32 * MAX_REWARDS);
}
