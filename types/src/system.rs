//! System modules, formerly known as "system contracts"
pub mod auction;
mod call_stack_element;
mod error;
pub mod handle_payment;
pub mod mint;
pub mod standard_payment;
mod system_contract_type;

pub use call_stack_element::{CallStackElement, CallStackElementTag};
pub use error::Error;
pub use system_contract_type::{
    SystemContractType, AUCTION, HANDLE_PAYMENT, MINT, STANDARD_PAYMENT,
};
