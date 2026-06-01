use anchor_lang::prelude::*;

#[error_code]
pub enum StakingError {
    #[msg("Math overflow")]
    MathOverflow,
    #[msg("Reward slots are full")]
    RewardSlotsFull,
    #[msg("Reward mint not registered")]
    RewardNotFound,
    #[msg("Reward slot is not accepting deposits")]
    RewardInactive,
    #[msg("Reward mint already registered")]
    RewardAlreadyExists,
    #[msg("Caller is not the keeper authority")]
    Unauthorized,
    #[msg("Pool is paused")]
    Paused,
    #[msg("Amount must be greater than zero")]
    ZeroAmount,
    #[msg("Insufficient staked balance")]
    InsufficientStake,
    #[msg("Duration must be greater than zero")]
    InvalidDuration,
    #[msg("Provided reward vault does not match any slot")]
    VaultMismatch,
    #[msg("Token account mint mismatch")]
    MintMismatch,
}
