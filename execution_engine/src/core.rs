//! The core of the smart contract execution logic.
pub mod engine_state;
pub mod execution;
pub mod resolvers;
pub mod runtime;
pub mod runtime_context;
pub(crate) mod tracking_copy;

use casper_types::{system::mint, CLValueError, RuntimeArgs, U512};
pub use tracking_copy::{validate_balance_proof, validate_query_proof, ValidationError};

/// The length of an address.
pub const ADDRESS_LENGTH: usize = 32;

/// Alias for an array of bytes that represents an address.
pub type Address = [u8; ADDRESS_LENGTH];

/// Returns value of `MAIN_PURSE_ALLOWANCE` from the runtime arguments or default.
/// If no `MAIN_PURSE_ALLOWANCE` present, uses `default`.
pub(crate) fn get_approved_cspr(
    runtime_args: &RuntimeArgs,
    default: U512,
) -> Result<U512, CLValueError> {
    runtime_args
        .get(mint::ARG_AMOUNT)
        .map(|cl| {
            cl.clone()
                .into_t::<U512>()
                .or_else(|_| cl.clone().into_t::<u64>().map(U512::from))
        })
        .unwrap_or_else(|| Ok(default))
}
