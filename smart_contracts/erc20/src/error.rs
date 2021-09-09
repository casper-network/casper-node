//! Error handling on the casper platform.
use casper_types::ApiError;

/// Represents error conditions of the erc20 contract.
///
/// For user errors users can use [`Error::User`] variant.
pub enum Error {
    /// ERC20 contract called from within invalid context.
    InvalidContext,
    /// Spender does not have enough balance.
    InsufficientBalance,
    /// Spender does not have enough allowance approved.
    InsufficientAllowance,
    /// Operation would cause an integer overflow.
    Overflow,
    /// User error.
    User(u16),
}

const ERROR_INVALID_CONTEXT: u16 = u16::MAX;
const ERROR_INSUFFICIENT_BALANCE: u16 = u16::MAX - 1;
const ERROR_INSUFFICIENT_ALLOWANCE: u16 = u16::MAX - 2;
const ERROR_OVERFLOW: u16 = u16::MAX - 3;

impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        let user_error = match error {
            Error::InvalidContext => ERROR_INVALID_CONTEXT,
            Error::InsufficientBalance => ERROR_INSUFFICIENT_BALANCE,
            Error::InsufficientAllowance => ERROR_INSUFFICIENT_ALLOWANCE,
            Error::Overflow => ERROR_OVERFLOW,
            Error::User(user_error) => user_error,
        };
        ApiError::User(user_error)
    }
}
