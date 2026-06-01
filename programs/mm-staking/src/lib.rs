use anchor_lang::prelude::*;

pub mod constants;
pub mod errors;
pub mod state;
pub mod math;
pub mod update;

declare_id!("1Zx9vyjZLMJqsFyZxraPBww4SrSPXwHt7HFbtwpfCmA");

#[program]
pub mod mm_staking {
    use super::*;
    // instruction handlers added in later tasks
}
