// Each instruction module exports a `handler` fn with the same name — this is
// the standard Anchor pattern. The glob re-exports are required by the
// `#[program]` macro's generated __client_accounts_* items. The `handler` name
// collision is intentional and harmless because lib.rs always calls handlers
// fully-qualified (e.g. `instructions::claim::handler`).
#![allow(ambiguous_glob_reexports)]

pub mod initialize_pool;
pub use initialize_pool::*;

pub mod add_reward;
pub use add_reward::*;

pub mod stake;
pub use stake::*;

pub mod deposit_rewards;
pub use deposit_rewards::*;

pub mod claim;
pub use claim::*;

pub mod emergency_withdraw;
pub use emergency_withdraw::*;

pub mod admin;
pub use admin::*;
