//! Contains [`ApiError`] and associated helper functions.

use core::{
    convert::TryFrom,
    fmt::{self, Debug, Formatter},
};

use crate::{
    account::{
        AddKeyFailure, RemoveKeyFailure, SetThresholdFailure, TryFromIntError,
        TryFromSliceForAccountHashError, UpdateKeyFailure,
    },
    bytesrepr, contracts,
    system::{auction, handle_payment, mint},
    CLValueError,
};

/// All `Error` variants defined in this library other than `Error::User` will convert to a `u32`
/// value less than or equal to `RESERVED_ERROR_MAX`.
const RESERVED_ERROR_MAX: u32 = u16::MAX as u32; // 0..=65535

/// Handle Payment errors will have this value added to them when being converted to a `u32`.
const POS_ERROR_OFFSET: u32 = RESERVED_ERROR_MAX - u8::MAX as u32; // 65280..=65535

/// Mint errors will have this value added to them when being converted to a `u32`.
const MINT_ERROR_OFFSET: u32 = (POS_ERROR_OFFSET - 1) - u8::MAX as u32; // 65024..=65279

/// Contract header errors will have this value added to them when being converted to a `u32`.
const HEADER_ERROR_OFFSET: u32 = (MINT_ERROR_OFFSET - 1) - u8::MAX as u32; // 64768..=65023

/// Contract header errors will have this value added to them when being converted to a `u32`.
const AUCTION_ERROR_OFFSET: u32 = (HEADER_ERROR_OFFSET - 1) - u8::MAX as u32; // 64512..=64767

/// Minimum value of user error's inclusive range.
const USER_ERROR_MIN: u32 = RESERVED_ERROR_MAX + 1;

/// Maximum value of user error's inclusive range.
const USER_ERROR_MAX: u32 = 2 * RESERVED_ERROR_MAX + 1;

/// Minimum value of Mint error's inclusive range.
const MINT_ERROR_MIN: u32 = MINT_ERROR_OFFSET;

/// Maximum value of Mint error's inclusive range.
const MINT_ERROR_MAX: u32 = POS_ERROR_OFFSET - 1;

/// Minimum value of Handle Payment error's inclusive range.
const HP_ERROR_MIN: u32 = POS_ERROR_OFFSET;

/// Maximum value of Handle Payment error's inclusive range.
const HP_ERROR_MAX: u32 = RESERVED_ERROR_MAX;

/// Minimum value of contract header error's inclusive range.
const HEADER_ERROR_MIN: u32 = HEADER_ERROR_OFFSET;

/// Maximum value of contract header error's inclusive range.
const HEADER_ERROR_MAX: u32 = HEADER_ERROR_OFFSET + u8::MAX as u32;

/// Minimum value of an auction contract error's inclusive range.
const AUCTION_ERROR_MIN: u32 = AUCTION_ERROR_OFFSET;

/// Maximum value of an auction contract error's inclusive range.
const AUCTION_ERROR_MAX: u32 = AUCTION_ERROR_OFFSET + u8::MAX as u32;

/// Errors which can be encountered while running a smart contract.
///
/// An `ApiError` can be converted to a `u32` in order to be passed via the execution engine's
/// `ext_ffi::casper_revert()` function.  This means the information each variant can convey is
/// limited.
///
/// The variants are split into numeric ranges as follows:
///
/// | Inclusive range | Variant(s)                                                      |
/// | ----------------| ----------------------------------------------------------------|
/// | [1, 64511]      | all except reserved system contract error ranges defined below. |
/// | [64512, 64767]  | `Auction`                                                       |
/// | [64768, 65023]  | `ContractHeader`                                                |
/// | [65024, 65279]  | `Mint`                                                          |
/// | [65280, 65535]  | `HandlePayment`                                                 |
/// | [65536, 131071] | `User`                                                          |
///
/// Users can specify a C-style enum and implement `From` to ease usage of
/// `casper_contract::runtime::revert()`, e.g.
/// ```
/// use casper_types::ApiError;
///
/// #[repr(u16)]
/// enum FailureCode {
///     Zero = 0,  // 65,536 as an ApiError::User
///     One,       // 65,537 as an ApiError::User
///     Two        // 65,538 as an ApiError::User
/// }
///
/// impl From<FailureCode> for ApiError {
///     fn from(code: FailureCode) -> Self {
///         ApiError::User(code as u16)
///     }
/// }
///
/// assert_eq!(ApiError::User(1), FailureCode::One.into());
/// assert_eq!(65_536, u32::from(ApiError::from(FailureCode::Zero)));
/// assert_eq!(65_538, u32::from(ApiError::from(FailureCode::Two)));
/// ```
#[derive(Copy, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ApiError {
    /// Optional data was unexpectedly `None`.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(1), ApiError::None);
    /// ```
    None,
    /// Specified argument not provided.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(2), ApiError::MissingArgument);
    /// ```
    MissingArgument,
    /// Argument not of correct type.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(3), ApiError::InvalidArgument);
    /// ```
    InvalidArgument,
    /// Failed to deserialize a value.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(4), ApiError::Deserialize);
    /// ```
    Deserialize,
    /// `casper_contract::storage::read()` returned an error.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(5), ApiError::Read);
    /// ```
    Read,
    /// The given key returned a `None` value.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(6), ApiError::ValueNotFound);
    /// ```
    ValueNotFound,
    /// Failed to find a specified contract.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(7), ApiError::ContractNotFound);
    /// ```
    ContractNotFound,
    /// A call to `casper_contract::runtime::get_key()` returned a failure.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(8), ApiError::GetKey);
    /// ```
    GetKey,
    /// The [`Key`](crate::Key) variant was not as expected.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(9), ApiError::UnexpectedKeyVariant);
    /// ```
    UnexpectedKeyVariant,
    /// Obsolete error variant (we no longer have ContractRef).
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(10), ApiError::UnexpectedContractRefVariant);
    /// ```
    UnexpectedContractRefVariant, // TODO: this variant is not used any longer and can be removed
    /// Invalid purse name given.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(11), ApiError::InvalidPurseName);
    /// ```
    InvalidPurseName,
    /// Invalid purse retrieved.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(12), ApiError::InvalidPurse);
    /// ```
    InvalidPurse,
    /// Failed to upgrade contract at [`URef`](crate::URef).
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(13), ApiError::UpgradeContractAtURef);
    /// ```
    UpgradeContractAtURef,
    /// Failed to transfer motes.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(14), ApiError::Transfer);
    /// ```
    Transfer,
    /// The given [`URef`](crate::URef) has no access rights.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(15), ApiError::NoAccessRights);
    /// ```
    NoAccessRights,
    /// A given type could not be constructed from a [`CLValue`](crate::CLValue).
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(16), ApiError::CLTypeMismatch);
    /// ```
    CLTypeMismatch,
    /// Early end of stream while deserializing.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(17), ApiError::EarlyEndOfStream);
    /// ```
    EarlyEndOfStream,
    /// Formatting error while deserializing.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(18), ApiError::Formatting);
    /// ```
    Formatting,
    /// Not all input bytes were consumed in [`deserialize`](crate::bytesrepr::deserialize).
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(19), ApiError::LeftOverBytes);
    /// ```
    LeftOverBytes,
    /// Out of memory error.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(20), ApiError::OutOfMemory);
    /// ```
    OutOfMemory,
    /// There are already maximum [`AccountHash`](crate::account::AccountHash)s associated with the
    /// given account.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(21), ApiError::MaxKeysLimit);
    /// ```
    MaxKeysLimit,
    /// The given [`AccountHash`](crate::account::AccountHash) is already associated with the given
    /// account.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(22), ApiError::DuplicateKey);
    /// ```
    DuplicateKey,
    /// Caller doesn't have sufficient permissions to perform the given action.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(23), ApiError::PermissionDenied);
    /// ```
    PermissionDenied,
    /// The given [`AccountHash`](crate::account::AccountHash) is not associated with the given
    /// account.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(24), ApiError::MissingKey);
    /// ```
    MissingKey,
    /// Removing/updating the given associated [`AccountHash`](crate::account::AccountHash) would
    /// cause the total [`Weight`](crate::account::Weight) of all remaining `AccountHash`s to
    /// fall below one of the action thresholds for the given account.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(25), ApiError::ThresholdViolation);
    /// ```
    ThresholdViolation,
    /// Setting the key-management threshold to a value lower than the deployment threshold is
    /// disallowed.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(26), ApiError::KeyManagementThreshold);
    /// ```
    KeyManagementThreshold,
    /// Setting the deployment threshold to a value greater than any other threshold is disallowed.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(27), ApiError::DeploymentThreshold);
    /// ```
    DeploymentThreshold,
    /// Setting a threshold to a value greater than the total weight of associated keys is
    /// disallowed.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(28), ApiError::InsufficientTotalWeight);
    /// ```
    InsufficientTotalWeight,
    /// The given `u32` doesn't map to a [`SystemContractType`](crate::system::SystemContractType).
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(29), ApiError::InvalidSystemContract);
    /// ```
    InvalidSystemContract,
    /// Failed to create a new purse.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(30), ApiError::PurseNotCreated);
    /// ```
    PurseNotCreated,
    /// An unhandled value, likely representing a bug in the code.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(31), ApiError::Unhandled);
    /// ```
    Unhandled,
    /// The provided buffer is too small to complete an operation.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(32), ApiError::BufferTooSmall);
    /// ```
    BufferTooSmall,
    /// No data available in the host buffer.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(33), ApiError::HostBufferEmpty);
    /// ```
    HostBufferEmpty,
    /// The host buffer has been set to a value and should be consumed first by a read operation.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(34), ApiError::HostBufferFull);
    /// ```
    HostBufferFull,
    /// Could not lay out an array in memory
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(35), ApiError::AllocLayout);
    /// ```
    AllocLayout,
    /// The `dictionary_item_key` length exceeds the maximum length.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(36), ApiError::DictionaryItemKeyExceedsLength);
    /// ```
    DictionaryItemKeyExceedsLength,
    /// The `dictionary_item_key` is invalid.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(37), ApiError::InvalidDictionaryItemKey);
    /// ```
    InvalidDictionaryItemKey,
    /// Unable to retrieve the requested system contract hash.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(38), ApiError::MissingSystemContractHash);
    /// ```
    MissingSystemContractHash,
    /// Exceeded a recursion depth limit.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(39), ApiError::ExceededRecursionDepth);
    /// ```
    ExceededRecursionDepth,
    /// Attempt to serialize a value that does not have a serialized representation.
    /// ```
    /// # use casper_types::ApiError;
    /// assert_eq!(ApiError::from(40), ApiError::NonRepresentableSerialization);
    /// ```
    NonRepresentableSerialization,
    /// Error specific to Auction contract. See
    /// [casper_types::system::auction::Error](crate::system::auction::Error).
    /// ```
    /// # use casper_types::ApiError;
    /// for code in 64512..=64767 {
    ///     assert!(matches!(ApiError::from(code), ApiError::AuctionError(_auction_error)));
    /// }
    /// ```
    AuctionError(u8),
    /// Contract header errors. See [casper_types::contracts::Error](crate::contracts::Error).
    ///
    /// ```
    /// # use casper_types::ApiError;
    /// for code in 64768..=65023 {
    ///     assert!(matches!(ApiError::from(code), ApiError::ContractHeader(_contract_header_error)));
    /// }
    /// ```
    ContractHeader(u8),
    /// Error specific to Mint contract. See
    /// [casper_types::system::mint::Error](crate::system::mint::Error).
    /// ```
    /// # use casper_types::ApiError;
    /// for code in 65024..=65279 {
    ///     assert!(matches!(ApiError::from(code), ApiError::Mint(_mint_error)));
    /// }
    /// ```
    Mint(u8),
    /// Error specific to Handle Payment contract. See
    /// [casper_types::system::handle_payment](crate::system::handle_payment::Error).
    /// ```
    /// # use casper_types::ApiError;
    /// for code in 65280..=65535 {
    ///     assert!(matches!(ApiError::from(code), ApiError::HandlePayment(_handle_payment_error)));
    /// }
    /// ```
    HandlePayment(u8),
    /// User-specified error code.  The internal `u16` value is added to `u16::MAX as u32 + 1` when
    /// an `Error::User` is converted to a `u32`.
    /// ```
    /// # use casper_types::ApiError;
    /// for code in 65536..131071 {
    ///     assert!(matches!(ApiError::from(code), ApiError::User(_)));
    /// }
    /// ```
    User(u16),
}

impl From<bytesrepr::Error> for ApiError {
    fn from(error: bytesrepr::Error) -> Self {
        match error {
            bytesrepr::Error::EarlyEndOfStream => ApiError::EarlyEndOfStream,
            bytesrepr::Error::Formatting => ApiError::Formatting,
            bytesrepr::Error::LeftOverBytes => ApiError::LeftOverBytes,
            bytesrepr::Error::OutOfMemory => ApiError::OutOfMemory,
            bytesrepr::Error::NotRepresentable => ApiError::NonRepresentableSerialization,
            bytesrepr::Error::ExceededRecursionDepth => ApiError::ExceededRecursionDepth,
        }
    }
}

impl From<AddKeyFailure> for ApiError {
    fn from(error: AddKeyFailure) -> Self {
        match error {
            AddKeyFailure::MaxKeysLimit => ApiError::MaxKeysLimit,
            AddKeyFailure::DuplicateKey => ApiError::DuplicateKey,
            AddKeyFailure::PermissionDenied => ApiError::PermissionDenied,
        }
    }
}

impl From<UpdateKeyFailure> for ApiError {
    fn from(error: UpdateKeyFailure) -> Self {
        match error {
            UpdateKeyFailure::MissingKey => ApiError::MissingKey,
            UpdateKeyFailure::PermissionDenied => ApiError::PermissionDenied,
            UpdateKeyFailure::ThresholdViolation => ApiError::ThresholdViolation,
        }
    }
}

impl From<RemoveKeyFailure> for ApiError {
    fn from(error: RemoveKeyFailure) -> Self {
        match error {
            RemoveKeyFailure::MissingKey => ApiError::MissingKey,
            RemoveKeyFailure::PermissionDenied => ApiError::PermissionDenied,
            RemoveKeyFailure::ThresholdViolation => ApiError::ThresholdViolation,
        }
    }
}

impl From<SetThresholdFailure> for ApiError {
    fn from(error: SetThresholdFailure) -> Self {
        match error {
            SetThresholdFailure::KeyManagementThreshold => ApiError::KeyManagementThreshold,
            SetThresholdFailure::DeploymentThreshold => ApiError::DeploymentThreshold,
            SetThresholdFailure::PermissionDeniedError => ApiError::PermissionDenied,
            SetThresholdFailure::InsufficientTotalWeight => ApiError::InsufficientTotalWeight,
        }
    }
}

impl From<CLValueError> for ApiError {
    fn from(error: CLValueError) -> Self {
        match error {
            CLValueError::Serialization(bytesrepr_error) => bytesrepr_error.into(),
            CLValueError::Type(_) => ApiError::CLTypeMismatch,
        }
    }
}

impl From<contracts::Error> for ApiError {
    fn from(error: contracts::Error) -> Self {
        ApiError::ContractHeader(error as u8)
    }
}

impl From<auction::Error> for ApiError {
    fn from(error: auction::Error) -> Self {
        ApiError::AuctionError(error as u8)
    }
}

// This conversion is not intended to be used by third party crates.
#[doc(hidden)]
impl From<TryFromIntError> for ApiError {
    fn from(_error: TryFromIntError) -> Self {
        ApiError::Unhandled
    }
}

impl From<TryFromSliceForAccountHashError> for ApiError {
    fn from(_error: TryFromSliceForAccountHashError) -> Self {
        ApiError::Deserialize
    }
}

impl From<mint::Error> for ApiError {
    fn from(error: mint::Error) -> Self {
        ApiError::Mint(error as u8)
    }
}

impl From<handle_payment::Error> for ApiError {
    fn from(error: handle_payment::Error) -> Self {
        ApiError::HandlePayment(error as u8)
    }
}

impl From<ApiError> for u32 {
    fn from(error: ApiError) -> Self {
        match error {
            ApiError::None => 1,
            ApiError::MissingArgument => 2,
            ApiError::InvalidArgument => 3,
            ApiError::Deserialize => 4,
            ApiError::Read => 5,
            ApiError::ValueNotFound => 6,
            ApiError::ContractNotFound => 7,
            ApiError::GetKey => 8,
            ApiError::UnexpectedKeyVariant => 9,
            ApiError::UnexpectedContractRefVariant => 10,
            ApiError::InvalidPurseName => 11,
            ApiError::InvalidPurse => 12,
            ApiError::UpgradeContractAtURef => 13,
            ApiError::Transfer => 14,
            ApiError::NoAccessRights => 15,
            ApiError::CLTypeMismatch => 16,
            ApiError::EarlyEndOfStream => 17,
            ApiError::Formatting => 18,
            ApiError::LeftOverBytes => 19,
            ApiError::OutOfMemory => 20,
            ApiError::MaxKeysLimit => 21,
            ApiError::DuplicateKey => 22,
            ApiError::PermissionDenied => 23,
            ApiError::MissingKey => 24,
            ApiError::ThresholdViolation => 25,
            ApiError::KeyManagementThreshold => 26,
            ApiError::DeploymentThreshold => 27,
            ApiError::InsufficientTotalWeight => 28,
            ApiError::InvalidSystemContract => 29,
            ApiError::PurseNotCreated => 30,
            ApiError::Unhandled => 31,
            ApiError::BufferTooSmall => 32,
            ApiError::HostBufferEmpty => 33,
            ApiError::HostBufferFull => 34,
            ApiError::AllocLayout => 35,
            ApiError::DictionaryItemKeyExceedsLength => 36,
            ApiError::InvalidDictionaryItemKey => 37,
            ApiError::MissingSystemContractHash => 38,
            ApiError::ExceededRecursionDepth => 39,
            ApiError::NonRepresentableSerialization => 40,
            ApiError::AuctionError(value) => AUCTION_ERROR_OFFSET + u32::from(value),
            ApiError::ContractHeader(value) => HEADER_ERROR_OFFSET + u32::from(value),
            ApiError::Mint(value) => MINT_ERROR_OFFSET + u32::from(value),
            ApiError::HandlePayment(value) => POS_ERROR_OFFSET + u32::from(value),
            ApiError::User(value) => RESERVED_ERROR_MAX + 1 + u32::from(value),
        }
    }
}

impl From<u32> for ApiError {
    fn from(value: u32) -> ApiError {
        match value {
            1 => ApiError::None,
            2 => ApiError::MissingArgument,
            3 => ApiError::InvalidArgument,
            4 => ApiError::Deserialize,
            5 => ApiError::Read,
            6 => ApiError::ValueNotFound,
            7 => ApiError::ContractNotFound,
            8 => ApiError::GetKey,
            9 => ApiError::UnexpectedKeyVariant,
            10 => ApiError::UnexpectedContractRefVariant,
            11 => ApiError::InvalidPurseName,
            12 => ApiError::InvalidPurse,
            13 => ApiError::UpgradeContractAtURef,
            14 => ApiError::Transfer,
            15 => ApiError::NoAccessRights,
            16 => ApiError::CLTypeMismatch,
            17 => ApiError::EarlyEndOfStream,
            18 => ApiError::Formatting,
            19 => ApiError::LeftOverBytes,
            20 => ApiError::OutOfMemory,
            21 => ApiError::MaxKeysLimit,
            22 => ApiError::DuplicateKey,
            23 => ApiError::PermissionDenied,
            24 => ApiError::MissingKey,
            25 => ApiError::ThresholdViolation,
            26 => ApiError::KeyManagementThreshold,
            27 => ApiError::DeploymentThreshold,
            28 => ApiError::InsufficientTotalWeight,
            29 => ApiError::InvalidSystemContract,
            30 => ApiError::PurseNotCreated,
            31 => ApiError::Unhandled,
            32 => ApiError::BufferTooSmall,
            33 => ApiError::HostBufferEmpty,
            34 => ApiError::HostBufferFull,
            35 => ApiError::AllocLayout,
            36 => ApiError::DictionaryItemKeyExceedsLength,
            37 => ApiError::InvalidDictionaryItemKey,
            38 => ApiError::MissingSystemContractHash,
            39 => ApiError::ExceededRecursionDepth,
            40 => ApiError::NonRepresentableSerialization,
            USER_ERROR_MIN..=USER_ERROR_MAX => ApiError::User(value as u16),
            HP_ERROR_MIN..=HP_ERROR_MAX => ApiError::HandlePayment(value as u8),
            MINT_ERROR_MIN..=MINT_ERROR_MAX => ApiError::Mint(value as u8),
            HEADER_ERROR_MIN..=HEADER_ERROR_MAX => ApiError::ContractHeader(value as u8),
            AUCTION_ERROR_MIN..=AUCTION_ERROR_MAX => ApiError::AuctionError(value as u8),
            _ => ApiError::Unhandled,
        }
    }
}

impl Debug for ApiError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            ApiError::None => write!(f, "ApiError::None")?,
            ApiError::MissingArgument => write!(f, "ApiError::MissingArgument")?,
            ApiError::InvalidArgument => write!(f, "ApiError::InvalidArgument")?,
            ApiError::Deserialize => write!(f, "ApiError::Deserialize")?,
            ApiError::Read => write!(f, "ApiError::Read")?,
            ApiError::ValueNotFound => write!(f, "ApiError::ValueNotFound")?,
            ApiError::ContractNotFound => write!(f, "ApiError::ContractNotFound")?,
            ApiError::GetKey => write!(f, "ApiError::GetKey")?,
            ApiError::UnexpectedKeyVariant => write!(f, "ApiError::UnexpectedKeyVariant")?,
            ApiError::UnexpectedContractRefVariant => {
                write!(f, "ApiError::UnexpectedContractRefVariant")?
            }
            ApiError::InvalidPurseName => write!(f, "ApiError::InvalidPurseName")?,
            ApiError::InvalidPurse => write!(f, "ApiError::InvalidPurse")?,
            ApiError::UpgradeContractAtURef => write!(f, "ApiError::UpgradeContractAtURef")?,
            ApiError::Transfer => write!(f, "ApiError::Transfer")?,
            ApiError::NoAccessRights => write!(f, "ApiError::NoAccessRights")?,
            ApiError::CLTypeMismatch => write!(f, "ApiError::CLTypeMismatch")?,
            ApiError::EarlyEndOfStream => write!(f, "ApiError::EarlyEndOfStream")?,
            ApiError::Formatting => write!(f, "ApiError::Formatting")?,
            ApiError::LeftOverBytes => write!(f, "ApiError::LeftOverBytes")?,
            ApiError::OutOfMemory => write!(f, "ApiError::OutOfMemory")?,
            ApiError::MaxKeysLimit => write!(f, "ApiError::MaxKeysLimit")?,
            ApiError::DuplicateKey => write!(f, "ApiError::DuplicateKey")?,
            ApiError::PermissionDenied => write!(f, "ApiError::PermissionDenied")?,
            ApiError::MissingKey => write!(f, "ApiError::MissingKey")?,
            ApiError::ThresholdViolation => write!(f, "ApiError::ThresholdViolation")?,
            ApiError::KeyManagementThreshold => write!(f, "ApiError::KeyManagementThreshold")?,
            ApiError::DeploymentThreshold => write!(f, "ApiError::DeploymentThreshold")?,
            ApiError::InsufficientTotalWeight => write!(f, "ApiError::InsufficientTotalWeight")?,
            ApiError::InvalidSystemContract => write!(f, "ApiError::InvalidSystemContract")?,
            ApiError::PurseNotCreated => write!(f, "ApiError::PurseNotCreated")?,
            ApiError::Unhandled => write!(f, "ApiError::Unhandled")?,
            ApiError::BufferTooSmall => write!(f, "ApiError::BufferTooSmall")?,
            ApiError::HostBufferEmpty => write!(f, "ApiError::HostBufferEmpty")?,
            ApiError::HostBufferFull => write!(f, "ApiError::HostBufferFull")?,
            ApiError::AllocLayout => write!(f, "ApiError::AllocLayout")?,
            ApiError::DictionaryItemKeyExceedsLength => {
                write!(f, "ApiError::DictionaryItemKeyTooLarge")?
            }
            ApiError::InvalidDictionaryItemKey => write!(f, "ApiError::InvalidDictionaryItemKey")?,
            ApiError::MissingSystemContractHash => write!(f, "ApiError::MissingContractHash")?,
            ApiError::NonRepresentableSerialization => {
                write!(f, "ApiError::NonRepresentableSerialization")?
            }
            ApiError::ExceededRecursionDepth => write!(f, "ApiError::ExceededRecursionDepth")?,
            ApiError::AuctionError(value) => write!(
                f,
                "ApiError::AuctionError({:?})",
                auction::Error::try_from(*value).map_err(|_err| fmt::Error)?
            )?,
            ApiError::ContractHeader(value) => write!(
                f,
                "ApiError::ContractHeader({:?})",
                contracts::Error::try_from(*value).map_err(|_err| fmt::Error)?
            )?,
            ApiError::Mint(value) => write!(
                f,
                "ApiError::Mint({:?})",
                mint::Error::try_from(*value).map_err(|_err| fmt::Error)?
            )?,
            ApiError::HandlePayment(value) => write!(
                f,
                "ApiError::HandlePayment({:?})",
                handle_payment::Error::try_from(*value).map_err(|_err| fmt::Error)?
            )?,
            ApiError::User(value) => write!(f, "ApiError::User({})", value)?,
        }
        write!(f, " [{}]", u32::from(*self))
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ApiError::User(value) => write!(f, "User error: {}", value),
            ApiError::ContractHeader(value) => write!(f, "Contract header error: {}", value),
            ApiError::Mint(value) => write!(f, "Mint error: {}", value),
            ApiError::HandlePayment(value) => write!(f, "Handle Payment error: {}", value),
            _ => <Self as Debug>::fmt(self, f),
        }
    }
}

// This function is not intended to be used by third party crates.
#[doc(hidden)]
pub fn i32_from<T>(result: Result<(), T>) -> i32
where
    ApiError: From<T>,
{
    match result {
        Ok(()) => 0,
        Err(error) => {
            let api_error = ApiError::from(error);
            u32::from(api_error) as i32
        }
    }
}

/// Converts an `i32` to a `Result<(), ApiError>`, where `0` represents `Ok(())`, and all other
/// inputs are mapped to `Err(ApiError::<variant>)`.  The full list of mappings can be found in the
/// [docs for `ApiError`](ApiError#mappings).
pub fn result_from(value: i32) -> Result<(), ApiError> {
    match value {
        0 => Ok(()),
        _ => Err(ApiError::from(value as u32)),
    }
}

#[cfg(test)]
mod tests {
    use std::{i32, u16, u8};

    use super::*;

    fn round_trip(result: Result<(), ApiError>) {
        let code = i32_from(result);
        assert_eq!(result, result_from(code));
    }

    #[test]
    fn error_values() {
        assert_eq!(65_024_u32, u32::from(ApiError::Mint(0))); // MINT_ERROR_OFFSET == 65,024
        assert_eq!(65_279_u32, u32::from(ApiError::Mint(u8::MAX)));
        assert_eq!(65_280_u32, u32::from(ApiError::HandlePayment(0))); // POS_ERROR_OFFSET == 65,280
        assert_eq!(65_535_u32, u32::from(ApiError::HandlePayment(u8::MAX)));
        assert_eq!(65_536_u32, u32::from(ApiError::User(0))); // u16::MAX + 1
        assert_eq!(131_071_u32, u32::from(ApiError::User(u16::MAX))); // 2 * u16::MAX + 1
    }

    #[test]
    fn error_descriptions_getkey() {
        assert_eq!("ApiError::GetKey [8]", &format!("{:?}", ApiError::GetKey));
        assert_eq!("ApiError::GetKey [8]", &format!("{}", ApiError::GetKey));
    }

    #[test]
    fn error_descriptions_contract_header() {
        assert_eq!(
            "ApiError::ContractHeader(PreviouslyUsedVersion) [64769]",
            &format!(
                "{:?}",
                ApiError::ContractHeader(contracts::Error::PreviouslyUsedVersion as u8)
            )
        );
        assert_eq!(
            "Contract header error: 0",
            &format!("{}", ApiError::ContractHeader(0))
        );
        assert_eq!(
            "Contract header error: 255",
            &format!("{}", ApiError::ContractHeader(u8::MAX))
        );
    }

    #[test]
    fn error_descriptions_mint() {
        assert_eq!(
            "ApiError::Mint(InsufficientFunds) [65024]",
            &format!("{:?}", ApiError::Mint(0))
        );
        assert_eq!("Mint error: 0", &format!("{}", ApiError::Mint(0)));
        assert_eq!("Mint error: 255", &format!("{}", ApiError::Mint(u8::MAX)));
    }

    #[test]
    fn error_descriptions_handle_payment() {
        assert_eq!(
            "ApiError::HandlePayment(NotBonded) [65280]",
            &format!(
                "{:?}",
                ApiError::HandlePayment(handle_payment::Error::NotBonded as u8)
            )
        );
    }
    #[test]
    fn error_descriptions_handle_payment_display() {
        assert_eq!(
            "Handle Payment error: 0",
            &format!(
                "{}",
                ApiError::HandlePayment(handle_payment::Error::NotBonded as u8)
            )
        );
    }

    #[test]
    fn error_descriptions_user_errors() {
        assert_eq!(
            "ApiError::User(0) [65536]",
            &format!("{:?}", ApiError::User(0))
        );

        assert_eq!("User error: 0", &format!("{}", ApiError::User(0)));
        assert_eq!(
            "ApiError::User(65535) [131071]",
            &format!("{:?}", ApiError::User(u16::MAX))
        );
        assert_eq!(
            "User error: 65535",
            &format!("{}", ApiError::User(u16::MAX))
        );
    }

    #[test]
    fn error_edge_cases() {
        assert_eq!(Err(ApiError::Unhandled), result_from(i32::MAX));
        assert_eq!(
            Err(ApiError::ContractHeader(255)),
            result_from(MINT_ERROR_OFFSET as i32 - 1)
        );
        assert_eq!(Err(ApiError::Unhandled), result_from(-1));
        assert_eq!(Err(ApiError::Unhandled), result_from(i32::MIN));
    }

    #[test]
    fn error_round_trips() {
        round_trip(Ok(()));
        round_trip(Err(ApiError::None));
        round_trip(Err(ApiError::MissingArgument));
        round_trip(Err(ApiError::InvalidArgument));
        round_trip(Err(ApiError::Deserialize));
        round_trip(Err(ApiError::Read));
        round_trip(Err(ApiError::ValueNotFound));
        round_trip(Err(ApiError::ContractNotFound));
        round_trip(Err(ApiError::GetKey));
        round_trip(Err(ApiError::UnexpectedKeyVariant));
        round_trip(Err(ApiError::UnexpectedContractRefVariant));
        round_trip(Err(ApiError::InvalidPurseName));
        round_trip(Err(ApiError::InvalidPurse));
        round_trip(Err(ApiError::UpgradeContractAtURef));
        round_trip(Err(ApiError::Transfer));
        round_trip(Err(ApiError::NoAccessRights));
        round_trip(Err(ApiError::CLTypeMismatch));
        round_trip(Err(ApiError::EarlyEndOfStream));
        round_trip(Err(ApiError::Formatting));
        round_trip(Err(ApiError::LeftOverBytes));
        round_trip(Err(ApiError::OutOfMemory));
        round_trip(Err(ApiError::MaxKeysLimit));
        round_trip(Err(ApiError::DuplicateKey));
        round_trip(Err(ApiError::PermissionDenied));
        round_trip(Err(ApiError::MissingKey));
        round_trip(Err(ApiError::ThresholdViolation));
        round_trip(Err(ApiError::KeyManagementThreshold));
        round_trip(Err(ApiError::DeploymentThreshold));
        round_trip(Err(ApiError::InsufficientTotalWeight));
        round_trip(Err(ApiError::InvalidSystemContract));
        round_trip(Err(ApiError::PurseNotCreated));
        round_trip(Err(ApiError::Unhandled));
        round_trip(Err(ApiError::BufferTooSmall));
        round_trip(Err(ApiError::HostBufferEmpty));
        round_trip(Err(ApiError::HostBufferFull));
        round_trip(Err(ApiError::AllocLayout));
        round_trip(Err(ApiError::NonRepresentableSerialization));
        round_trip(Err(ApiError::ContractHeader(0)));
        round_trip(Err(ApiError::ContractHeader(u8::MAX)));
        round_trip(Err(ApiError::Mint(0)));
        round_trip(Err(ApiError::Mint(u8::MAX)));
        round_trip(Err(ApiError::HandlePayment(0)));
        round_trip(Err(ApiError::HandlePayment(u8::MAX)));
        round_trip(Err(ApiError::User(0)));
        round_trip(Err(ApiError::User(u16::MAX)));
        round_trip(Err(ApiError::AuctionError(0)));
        round_trip(Err(ApiError::AuctionError(u8::MAX)));
    }
}
