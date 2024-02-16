//! System modules, formerly known as "system contracts"
pub mod auction;
mod caller;
mod error;
pub mod handle_payment;
pub mod mint;
pub mod standard_payment;
mod system_contract_type;

pub use caller::{Caller, CallerTag};
pub use error::Error;
pub use system_contract_type::{SystemEntityType, AUCTION, HANDLE_PAYMENT, MINT, STANDARD_PAYMENT};
