use casper_macros::casper;

/// While the code consuming this contract needs to define further error variants, it can
/// return those via the [`Error::User`] variant or equivalently via the [`ApiError::User`]
/// variant.
#[derive(Debug, PartialEq, Eq)]
#[casper]
pub enum Cep18Error {
    /// CEP-18 contract called from within an invalid context.
    InvalidContext,
    /// Spender does not have enough balance.
    InsufficientBalance,
    /// Spender does not have enough allowance approved.
    InsufficientAllowance,
    /// Operation would cause an integer overflow.
    Overflow,
    /// A required package hash was not specified.
    PackageHashMissing,
    /// The package hash specified does not represent a package.
    PackageHashNotPackage,
    /// An invalid event mode was specified.
    InvalidEventsMode,
    /// The event mode required was not specified.
    MissingEventsMode,
    /// An unknown error occurred.
    Phantom,
    /// Failed to read the runtime arguments provided.
    FailedToGetArgBytes,
    /// The caller does not have sufficient security access.
    InsufficientRights,
    /// The list of Admin accounts provided is invalid.
    InvalidAdminList,
    /// The list of accounts that can mint tokens is invalid.
    InvalidMinterList,
    /// The list of accounts with no access rights is invalid.
    InvalidNoneList,
    /// The flag to enable the mint and burn mode is invalid.
    InvalidEnableMBFlag,
    /// This contract instance cannot be initialized again.
    AlreadyInitialized,
    ///  The mint and burn mode is disabled.
    MintBurnDisabled,
    CannotTargetSelfUser,
    InvalidBurnTarget,
}
