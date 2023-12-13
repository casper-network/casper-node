use casper_macros::CasperABI;
use casper_sdk::abi::CasperABI;

/// While the code consuming this contract needs to define further error variants, it can
/// return those via the [`Error::User`] variant or equivalently via the [`ApiError::User`]
/// variant.
#[repr(u16)]
#[derive(Clone, Copy, CasperABI)]
pub enum Cep18Error {
    /// CEP-18 contract called from within an invalid context.
    InvalidContext = 60000,
    /// Spender does not have enough balance.
    InsufficientBalance = 60001,
    /// Spender does not have enough allowance approved.
    InsufficientAllowance = 60002,
    /// Operation would cause an integer overflow.
    Overflow = 60003,
    /// A required package hash was not specified.
    PackageHashMissing = 60004,
    /// The package hash specified does not represent a package.
    PackageHashNotPackage = 60005,
    /// An invalid event mode was specified.
    InvalidEventsMode = 60006,
    /// The event mode required was not specified.
    MissingEventsMode = 60007,
    /// An unknown error occurred.
    Phantom = 60008,
    /// Failed to read the runtime arguments provided.
    FailedToGetArgBytes = 60009,
    /// The caller does not have sufficient security access.
    InsufficientRights = 60010,
    /// The list of Admin accounts provided is invalid.
    InvalidAdminList = 60011,
    /// The list of accounts that can mint tokens is invalid.
    InvalidMinterList = 60012,
    /// The list of accounts with no access rights is invalid.
    InvalidNoneList = 60013,
    /// The flag to enable the mint and burn mode is invalid.
    InvalidEnableMBFlag = 60014,
    /// This contract instance cannot be initialized again.
    AlreadyInitialized = 60015,
    ///  The mint and burn mode is disabled.
    MintBurnDisabled = 60016,
    CannotTargetSelfUser = 60017,
    InvalidBurnTarget = 60018,
}
