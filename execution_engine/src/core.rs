//! The core of the smart contract execution logic.
pub mod engine_state;
pub mod execution;
pub mod resolvers;
pub mod runtime;
pub mod runtime_context;
pub(crate) mod tracking_copy;

use casper_types::{system::mint, CLType, CLValueError, RuntimeArgs, U512};
pub use tracking_copy::{validate_balance_proof, validate_query_proof, ValidationError};

/// The length of an address.
pub const ADDRESS_LENGTH: usize = 32;

/// Alias for an array of bytes that represents an address.
pub type Address = [u8; ADDRESS_LENGTH];

/// Returns value of `mint::ARG_AMOUNT` from the runtime arguments or defaults to 0 if that arg
/// doesn't exist or is not an integer type.
///
/// Returns an error if parsing the arg fails.
pub(crate) fn get_approved_amount(runtime_args: &RuntimeArgs) -> Result<U512, CLValueError> {
    let amount_arg = match runtime_args.get(mint::ARG_AMOUNT) {
        None => return Ok(U512::zero()),
        Some(arg) => arg,
    };
    match amount_arg.cl_type() {
        CLType::U512 => amount_arg.clone().into_t::<U512>(),
        CLType::U64 => amount_arg.clone().into_t::<u64>().map(U512::from),
        _ => Ok(U512::zero()),
    }
}

#[cfg(test)]
mod test {
    use casper_types::{system::mint, RuntimeArgs, U512};

    use crate::core::get_approved_amount;

    #[test]
    fn get_approved_amount_test() {
        let mut args = RuntimeArgs::new();
        args.insert(mint::ARG_AMOUNT, 0u64).expect("is ok");
        assert_eq!(get_approved_amount(&args).unwrap(), U512::zero());

        let mut args = RuntimeArgs::new();
        args.insert(mint::ARG_AMOUNT, U512::zero()).expect("is ok");
        assert_eq!(get_approved_amount(&args).unwrap(), U512::zero());

        let args = RuntimeArgs::new();
        assert_eq!(get_approved_amount(&args).unwrap(), U512::zero());

        let hundred = 100u64;

        let mut args = RuntimeArgs::new();
        let input = U512::from(hundred);
        args.insert(mint::ARG_AMOUNT, input).expect("is ok");
        assert_eq!(get_approved_amount(&args).unwrap(), input);

        let mut args = RuntimeArgs::new();
        args.insert(mint::ARG_AMOUNT, hundred).expect("is ok");
        assert_eq!(get_approved_amount(&args).unwrap(), U512::from(hundred));
    }
}
