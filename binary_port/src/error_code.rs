use core::{convert::TryFrom, fmt};

use casper_types::{InvalidDeploy, InvalidTransaction, InvalidTransactionV1};

#[cfg(test)]
use strum_macros::EnumIter;

/// The error code indicating the result of handling the binary request.
#[derive(Debug, Copy, Clone, thiserror::Error, Eq, PartialEq)]
#[repr(u16)]
#[cfg_attr(test, derive(EnumIter))]
pub enum ErrorCode {
    /// Request executed correctly.
    #[error("request executed correctly")]
    NoError = 0,
    /// This function is disabled.
    #[error("this function is disabled")]
    FunctionDisabled = 1,
    /// Data not found.
    #[error("data not found")]
    NotFound = 2,
    /// Root not found.
    #[error("root not found")]
    RootNotFound = 3,
    /// Invalid item variant.
    #[error("invalid item variant")]
    InvalidItemVariant = 4,
    /// Wasm preprocessing.
    #[error("wasm preprocessing")]
    WasmPreprocessing = 5,
    /// Invalid protocol version.
    #[error("unsupported protocol version")]
    UnsupportedProtocolVersion = 6,
    /// Internal error.
    #[error("internal error")]
    InternalError = 7,
    /// The query failed.
    #[error("the query failed")]
    FailedQuery = 8,
    /// Bad request.
    #[error("bad request")]
    BadRequest = 9,
    /// Received an unsupported type of request.
    #[error("unsupported request")]
    UnsupportedRequest = 10,
    /// Dictionary URef not found.
    #[error("dictionary URef not found")]
    DictionaryURefNotFound = 11,
    /// This node has no complete blocks.
    #[error("no complete blocks")]
    NoCompleteBlocks = 12,
    /// The deploy had an invalid chain name
    #[error("the deploy had an invalid chain name")]
    InvalidDeployChainName = 13,
    /// Deploy dependencies are no longer supported
    #[error("the dependencies for this transaction are no longer supported")]
    InvalidDeployDependenciesNoLongerSupported = 14,
    /// The deploy sent to the network had an excessive size
    #[error("the deploy had an excessive size")]
    InvalidDeployExcessiveSize = 15,
    /// The deploy sent to the network had an excessive time to live
    #[error("the deploy had an excessive time to live")]
    InvalidDeployExcessiveTimeToLive = 16,
    /// The deploy sent to the network had a timestamp referencing a time that has yet to occur.
    #[error("the deploys timestamp is in the future")]
    InvalidDeployTimestampInFuture = 17,
    /// The deploy sent to the network had an invalid body hash
    #[error("the deploy had an invalid body hash")]
    InvalidDeployBodyHash = 18,
    /// The deploy sent to the network had an invalid deploy hash i.e. the provided deploy hash
    /// didn't match the derived deploy hash
    #[error("the deploy had an invalid deploy hash")]
    InvalidDeployHash = 19,
    /// The deploy sent to the network had an empty approval set
    #[error("the deploy had no approvals")]
    InvalidDeployEmptyApprovals = 20,
    /// The deploy sent to the network had an invalid approval
    #[error("the deploy had an invalid approval")]
    InvalidDeployApproval = 21,
    /// The deploy sent to the network had an excessive session args length
    #[error("the deploy had an excessive session args length")]
    InvalidDeployExcessiveSessionArgsLength = 22,
    /// The deploy sent to the network had an excessive payment args length
    #[error("the deploy had an excessive payment args length")]
    InvalidDeployExcessivePaymentArgsLength = 23,
    /// The deploy sent to the network had a missing payment amount
    #[error("the deploy had a missing payment amount")]
    InvalidDeployMissingPaymentAmount = 24,
    /// The deploy sent to the network had a payment amount that was not parseable
    #[error("the deploy sent to the network had a payment amount that was unable to be parsed")]
    InvalidDeployFailedToParsePaymentAmount = 25,
    /// The deploy sent to the network exceeded the block gas limit
    #[error("the deploy sent to the network exceeded the block gas limit")]
    InvalidDeployExceededBlockGasLimit = 26,
    /// The deploy sent to the network was missing a transfer amount
    #[error("the deploy sent to the network was missing a transfer amount")]
    InvalidDeployMissingTransferAmount = 27,
    /// The deploy sent to the network had a transfer amount that was unable to be parseable
    #[error("the deploy sent to the network had a transfer amount that was unable to be parsed")]
    InvalidDeployFailedToParseTransferAmount = 28,
    /// The deploy sent to the network had a transfer amount that was insufficient
    #[error("the deploy sent to the network had an insufficient transfer amount")]
    InvalidDeployInsufficientTransferAmount = 29,
    /// The deploy sent to the network had excessive approvals
    #[error("the deploy sent to the network had excessive approvals")]
    InvalidDeployExcessiveApprovals = 30,
    /// The network was unable to calculate the gas limit for the deploy
    #[error("the network was unable to calculate the gas limit associated with the deploy")]
    InvalidDeployUnableToCalculateGasLimit = 31,
    /// The network was unable to calculate the gas cost for the deploy
    #[error("the network was unable to calculate the gas cost for the deploy")]
    InvalidDeployUnableToCalculateGasCost = 32,
    /// The deploy sent to the network was invalid for an unspecified reason
    #[error("the deploy sent to the network was invalid for an unspecified reason")]
    InvalidDeployUnspecified = 33,
    /// The transaction sent to the network had an invalid chain name
    #[error("the transaction sent to the network had an invalid chain name")]
    InvalidTransactionChainName = 34,
    /// The transaction sent to the network had an excessive size
    #[error("the transaction sent to the network had an excessive size")]
    InvalidTransactionExcessiveSize = 35,
    /// The transaction sent to the network had an excessive time to live
    #[error("the transaction sent to the network had an excessive time to live")]
    InvalidTransactionExcessiveTimeToLive = 36,
    /// The transaction sent to the network had a timestamp located in the future.
    #[error("the transaction sent to the network had a timestamp that has not yet occurred")]
    InvalidTransactionTimestampInFuture = 37,
    /// The transaction sent to the network had a provided body hash that conflicted with hash
    /// derived by the network
    #[error("the transaction sent to the network had an invalid body hash")]
    InvalidTransactionBodyHash = 38,
    /// The transaction sent to the network had a provided hash that conflicted with the hash
    /// derived by the network
    #[error("the transaction sent to the network had an invalid hash")]
    InvalidTransactionHash = 39,
    /// The transaction sent to the network had an empty approvals set
    #[error("the transaction sent to the network had no approvals")]
    InvalidTransactionEmptyApprovals = 40,
    /// The transaction sent to the network had an invalid approval
    #[error("the transaction sent to the network had an invalid approval")]
    InvalidTransactionInvalidApproval = 41,
    /// The transaction sent to the network had excessive args length
    #[error("the transaction sent to the network had excessive args length")]
    InvalidTransactionExcessiveArgsLength = 42,
    /// The transaction sent to the network had excessive approvals
    #[error("the transaction sent to the network had excessive approvals")]
    InvalidTransactionExcessiveApprovals = 43,
    /// The transaction sent to the network exceeds the block gas limit
    #[error("the transaction sent to the network exceeds the networks block gas limit")]
    InvalidTransactionExceedsBlockGasLimit = 44,
    /// The transaction sent to the network had a missing arg
    #[error("the transaction sent to the network was missing an argument")]
    InvalidTransactionMissingArg = 45,
    /// The transaction sent to the network had an argument with an unexpected type
    #[error("the transaction sent to the network had an unexpected argument type")]
    InvalidTransactionUnexpectedArgType = 46,
    /// The transaction sent to the network had an invalid argument
    #[error("the transaction sent to the network had an invalid argument")]
    InvalidTransactionInvalidArg = 47,
    /// The transaction sent to the network had an insufficient transfer amount
    #[error("the transaction sent to the network had an insufficient transfer amount")]
    InvalidTransactionInsufficientTransferAmount = 48,
    /// The transaction sent to the network had a custom entry point when it should have a non
    /// custom entry point.
    #[error("the native transaction sent to the network should not have a custom entry point")]
    InvalidTransactionEntryPointCannotBeCustom = 49,
    /// The transaction sent to the network had a standard entry point when it must be custom.
    #[error("the non-native transaction sent to the network must have a custom entry point")]
    InvalidTransactionEntryPointMustBeCustom = 50,
    /// The transaction sent to the network had empty module bytes
    #[error("the transaction sent to the network had empty module bytes")]
    InvalidTransactionEmptyModuleBytes = 51,
    /// The transaction sent to the network had an invalid gas price conversion
    #[error("the transaction sent to the network had an invalid gas price conversion")]
    InvalidTransactionGasPriceConversion = 52,
    /// The network was unable to calculate the gas limit for the transaction sent.
    #[error("the network was unable to calculate the gas limit for the transaction sent")]
    InvalidTransactionUnableToCalculateGasLimit = 53,
    /// The network was unable to calculate the gas cost for the transaction sent.
    #[error("the network was unable to calculate the gas cost for the transaction sent.")]
    InvalidTransactionUnableToCalculateGasCost = 54,
    /// The transaction sent to the network had an invalid pricing mode
    #[error("the transaction sent to the network had an invalid pricing mode")]
    InvalidTransactionPricingMode = 55,
    /// The transaction sent to the network was invalid for an unspecified reason
    #[error("the transaction sent to the network was invalid for an unspecified reason")]
    InvalidTransactionUnspecified = 56,
    /// As the various enums are tagged non_exhaustive, it is possible that in the future none of
    /// these previous errors cover the error that occurred, therefore we need some catchall in
    /// the case that nothing else works.
    #[error("the transaction or deploy sent to the network was invalid for an unspecified reason")]
    InvalidTransactionOrDeployUnspecified = 57,
    /// The switch block for the requested era was not found
    #[error("the switch block for the requested era was not found")]
    SwitchBlockNotFound = 58,
    #[error("the parent of the switch block for the requested era was not found")]
    /// The parent of the switch block for the requested era was not found
    SwitchBlockParentNotFound = 59,
    #[error("cannot serve rewards stored in V1 format")]
    /// Cannot serve rewards stored in V1 format
    UnsupportedRewardsV1Request = 60,
    /// Invalid binary port version.
    #[error("binary protocol version mismatch")]
    BinaryProtocolVersionMismatch = 61,
    /// Blockchain is empty
    #[error("blockchain is empty")]
    EmptyBlockchain = 62,
    /// Expected deploy, but got transaction
    #[error("expected deploy, got transaction")]
    ExpectedDeploy = 63,
    /// Expected transaction, but got deploy
    #[error("expected transaction V1, got deploy")]
    ExpectedTransaction = 64,
    /// Transaction has expired
    #[error("transaction has expired")]
    TransactionExpired = 65,
    /// Transactions parameters are missing or incorrect
    #[error("missing or incorrect transaction parameters")]
    MissingOrIncorrectParameters = 66,
    /// No such addressable entity
    #[error("no such addressable entity")]
    NoSuchAddressableEntity = 67,
    // No such contract at hash
    #[error("no such contract at hash")]
    NoSuchContractAtHash = 68,
    /// No such entry point
    #[error("no such entry point")]
    NoSuchEntryPoint = 69,
    /// No such package at hash
    #[error("no such package at hash")]
    NoSuchPackageAtHash = 70,
    /// Invalid entity at version
    #[error("invalid entity at version")]
    InvalidEntityAtVersion = 71,
    /// Disabled entity at version
    #[error("disabled entity at version")]
    DisabledEntityAtVersion = 72,
    /// Missing entity at version
    #[error("missing entity at version")]
    MissingEntityAtVersion = 73,
    /// Invalid associated keys
    #[error("invalid associated keys")]
    InvalidAssociatedKeys = 74,
    /// Insufficient signature weight
    #[error("insufficient signature weight")]
    InsufficientSignatureWeight = 75,
    /// Insufficient balance
    #[error("insufficient balance")]
    InsufficientBalance = 76,
    /// Unknown balance
    #[error("unknown balance")]
    UnknownBalance = 77,
    /// Invalid payment variant for deploy
    #[error("invalid payment variant for deploy")]
    DeployInvalidPaymentVariant = 78,
    /// Missing payment amount for deploy
    #[error("missing payment amount for deploy")]
    DeployMissingPaymentAmount = 79,
    /// Failed to parse payment amount for deploy
    #[error("failed to parse payment amount for deploy")]
    DeployFailedToParsePaymentAmount = 80,
    /// Missing transfer target for deploy
    #[error("missing transfer target for deploy")]
    DeployMissingTransferTarget = 81,
    /// Missing module bytes for deploy
    #[error("missing module bytes for deploy")]
    DeployMissingModuleBytes = 82,
    /// Entry point cannot be 'call'
    #[error("entry point cannot be 'call'")]
    InvalidTransactionEntryPointCannotBeCall = 83,
    /// Invalid transaction kind
    #[error("invalid transaction kind")]
    InvalidTransactionInvalidTransactionKind = 84,
    /// Gas price tolerance too low
    #[error("gas price tolerance too low")]
    GasPriceToleranceTooLow = 85,
    /// Received V1 Transaction for spec exec.
    #[error("received v1 transaction for speculative execution")]
    ReceivedV1Transaction = 86,
    /// Purse was not found for given identifier.
    #[error("purse was not found for given identifier")]
    PurseNotFound = 87,
    /// Too many requests per second.
    #[error("request was throttled")]
    RequestThrottled = 88,
}

impl TryFrom<u16> for ErrorCode {
    type Error = UnknownErrorCode;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(ErrorCode::NoError),
            1 => Ok(ErrorCode::FunctionDisabled),
            2 => Ok(ErrorCode::NotFound),
            3 => Ok(ErrorCode::RootNotFound),
            4 => Ok(ErrorCode::InvalidItemVariant),
            5 => Ok(ErrorCode::WasmPreprocessing),
            6 => Ok(ErrorCode::UnsupportedProtocolVersion),
            7 => Ok(ErrorCode::InternalError),
            8 => Ok(ErrorCode::FailedQuery),
            9 => Ok(ErrorCode::BadRequest),
            10 => Ok(ErrorCode::UnsupportedRequest),
            11 => Ok(ErrorCode::DictionaryURefNotFound),
            12 => Ok(ErrorCode::NoCompleteBlocks),
            13 => Ok(ErrorCode::InvalidDeployChainName),
            14 => Ok(ErrorCode::InvalidDeployDependenciesNoLongerSupported),
            15 => Ok(ErrorCode::InvalidDeployExcessiveSize),
            16 => Ok(ErrorCode::InvalidDeployExcessiveTimeToLive),
            17 => Ok(ErrorCode::InvalidDeployTimestampInFuture),
            18 => Ok(ErrorCode::InvalidDeployBodyHash),
            19 => Ok(ErrorCode::InvalidDeployHash),
            20 => Ok(ErrorCode::InvalidDeployEmptyApprovals),
            21 => Ok(ErrorCode::InvalidDeployApproval),
            22 => Ok(ErrorCode::InvalidDeployExcessiveSessionArgsLength),
            23 => Ok(ErrorCode::InvalidDeployExcessivePaymentArgsLength),
            24 => Ok(ErrorCode::InvalidDeployMissingPaymentAmount),
            25 => Ok(ErrorCode::InvalidDeployFailedToParsePaymentAmount),
            26 => Ok(ErrorCode::InvalidDeployExceededBlockGasLimit),
            27 => Ok(ErrorCode::InvalidDeployMissingTransferAmount),
            28 => Ok(ErrorCode::InvalidDeployFailedToParseTransferAmount),
            29 => Ok(ErrorCode::InvalidDeployInsufficientTransferAmount),
            30 => Ok(ErrorCode::InvalidDeployExcessiveApprovals),
            31 => Ok(ErrorCode::InvalidDeployUnableToCalculateGasLimit),
            32 => Ok(ErrorCode::InvalidDeployUnableToCalculateGasCost),
            33 => Ok(ErrorCode::InvalidDeployUnspecified),
            34 => Ok(ErrorCode::InvalidTransactionChainName),
            35 => Ok(ErrorCode::InvalidTransactionExcessiveSize),
            36 => Ok(ErrorCode::InvalidTransactionExcessiveTimeToLive),
            37 => Ok(ErrorCode::InvalidTransactionTimestampInFuture),
            38 => Ok(ErrorCode::InvalidTransactionBodyHash),
            39 => Ok(ErrorCode::InvalidTransactionHash),
            40 => Ok(ErrorCode::InvalidTransactionEmptyApprovals),
            41 => Ok(ErrorCode::InvalidTransactionInvalidApproval),
            42 => Ok(ErrorCode::InvalidTransactionExcessiveArgsLength),
            43 => Ok(ErrorCode::InvalidTransactionExcessiveApprovals),
            44 => Ok(ErrorCode::InvalidTransactionExceedsBlockGasLimit),
            45 => Ok(ErrorCode::InvalidTransactionMissingArg),
            46 => Ok(ErrorCode::InvalidTransactionUnexpectedArgType),
            47 => Ok(ErrorCode::InvalidTransactionInvalidArg),
            48 => Ok(ErrorCode::InvalidTransactionInsufficientTransferAmount),
            49 => Ok(ErrorCode::InvalidTransactionEntryPointCannotBeCustom),
            50 => Ok(ErrorCode::InvalidTransactionEntryPointMustBeCustom),
            51 => Ok(ErrorCode::InvalidTransactionEmptyModuleBytes),
            52 => Ok(ErrorCode::InvalidTransactionGasPriceConversion),
            53 => Ok(ErrorCode::InvalidTransactionUnableToCalculateGasLimit),
            54 => Ok(ErrorCode::InvalidTransactionUnableToCalculateGasCost),
            55 => Ok(ErrorCode::InvalidTransactionPricingMode),
            56 => Ok(ErrorCode::InvalidTransactionUnspecified),
            57 => Ok(ErrorCode::InvalidTransactionOrDeployUnspecified),
            58 => Ok(ErrorCode::SwitchBlockNotFound),
            59 => Ok(ErrorCode::SwitchBlockParentNotFound),
            60 => Ok(ErrorCode::UnsupportedRewardsV1Request),
            61 => Ok(ErrorCode::BinaryProtocolVersionMismatch),
            62 => Ok(ErrorCode::EmptyBlockchain),
            63 => Ok(ErrorCode::ExpectedDeploy),
            64 => Ok(ErrorCode::ExpectedTransaction),
            65 => Ok(ErrorCode::TransactionExpired),
            66 => Ok(ErrorCode::MissingOrIncorrectParameters),
            67 => Ok(ErrorCode::NoSuchAddressableEntity),
            68 => Ok(ErrorCode::NoSuchContractAtHash),
            69 => Ok(ErrorCode::NoSuchEntryPoint),
            70 => Ok(ErrorCode::NoSuchPackageAtHash),
            71 => Ok(ErrorCode::InvalidEntityAtVersion),
            72 => Ok(ErrorCode::DisabledEntityAtVersion),
            73 => Ok(ErrorCode::MissingEntityAtVersion),
            74 => Ok(ErrorCode::InvalidAssociatedKeys),
            75 => Ok(ErrorCode::InsufficientSignatureWeight),
            76 => Ok(ErrorCode::InsufficientBalance),
            77 => Ok(ErrorCode::UnknownBalance),
            78 => Ok(ErrorCode::DeployInvalidPaymentVariant),
            79 => Ok(ErrorCode::DeployMissingPaymentAmount),
            80 => Ok(ErrorCode::DeployFailedToParsePaymentAmount),
            81 => Ok(ErrorCode::DeployMissingTransferTarget),
            82 => Ok(ErrorCode::DeployMissingModuleBytes),
            83 => Ok(ErrorCode::InvalidTransactionEntryPointCannotBeCall),
            84 => Ok(ErrorCode::InvalidTransactionInvalidTransactionKind),
            85 => Ok(ErrorCode::GasPriceToleranceTooLow),
            86 => Ok(ErrorCode::ReceivedV1Transaction),
            87 => Ok(ErrorCode::PurseNotFound),
            88 => Ok(ErrorCode::RequestThrottled),
            _ => Err(UnknownErrorCode),
        }
    }
}

/// Error indicating that the error code is unknown.
#[derive(Debug, Clone, Copy)]
pub struct UnknownErrorCode;

impl fmt::Display for UnknownErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown node error code")
    }
}

impl std::error::Error for UnknownErrorCode {}

impl From<InvalidTransaction> for ErrorCode {
    fn from(value: InvalidTransaction) -> Self {
        match value {
            InvalidTransaction::Deploy(invalid_deploy) => ErrorCode::from(invalid_deploy),
            InvalidTransaction::V1(invalid_transaction) => ErrorCode::from(invalid_transaction),
            _ => ErrorCode::InvalidTransactionOrDeployUnspecified,
        }
    }
}

impl From<InvalidDeploy> for ErrorCode {
    fn from(value: InvalidDeploy) -> Self {
        match value {
            InvalidDeploy::InvalidChainName { .. } => ErrorCode::InvalidDeployChainName,
            InvalidDeploy::DependenciesNoLongerSupported => {
                ErrorCode::InvalidDeployDependenciesNoLongerSupported
            }
            InvalidDeploy::ExcessiveSize(_) => ErrorCode::InvalidDeployExcessiveSize,
            InvalidDeploy::ExcessiveTimeToLive { .. } => {
                ErrorCode::InvalidDeployExcessiveTimeToLive
            }
            InvalidDeploy::TimestampInFuture { .. } => ErrorCode::InvalidDeployTimestampInFuture,
            InvalidDeploy::InvalidBodyHash => ErrorCode::InvalidDeployBodyHash,
            InvalidDeploy::InvalidDeployHash => ErrorCode::InvalidDeployHash,
            InvalidDeploy::EmptyApprovals => ErrorCode::InvalidDeployEmptyApprovals,
            InvalidDeploy::InvalidApproval { .. } => ErrorCode::InvalidDeployApproval,
            InvalidDeploy::ExcessiveSessionArgsLength { .. } => {
                ErrorCode::InvalidDeployExcessiveSessionArgsLength
            }
            InvalidDeploy::ExcessivePaymentArgsLength { .. } => {
                ErrorCode::InvalidDeployExcessivePaymentArgsLength
            }
            InvalidDeploy::MissingPaymentAmount => ErrorCode::InvalidDeployMissingPaymentAmount,
            InvalidDeploy::FailedToParsePaymentAmount => {
                ErrorCode::InvalidDeployFailedToParsePaymentAmount
            }
            InvalidDeploy::ExceededBlockGasLimit { .. } => {
                ErrorCode::InvalidDeployExceededBlockGasLimit
            }
            InvalidDeploy::MissingTransferAmount => ErrorCode::InvalidDeployMissingTransferAmount,
            InvalidDeploy::FailedToParseTransferAmount => {
                ErrorCode::InvalidDeployFailedToParseTransferAmount
            }
            InvalidDeploy::InsufficientTransferAmount { .. } => {
                ErrorCode::InvalidDeployInsufficientTransferAmount
            }
            InvalidDeploy::ExcessiveApprovals { .. } => ErrorCode::InvalidDeployExcessiveApprovals,
            InvalidDeploy::UnableToCalculateGasLimit => {
                ErrorCode::InvalidDeployUnableToCalculateGasLimit
            }
            InvalidDeploy::UnableToCalculateGasCost => {
                ErrorCode::InvalidDeployUnableToCalculateGasCost
            }
            InvalidDeploy::GasPriceToleranceTooLow { .. } => ErrorCode::GasPriceToleranceTooLow,
            _ => ErrorCode::InvalidDeployUnspecified,
        }
    }
}

impl From<InvalidTransactionV1> for ErrorCode {
    fn from(value: InvalidTransactionV1) -> Self {
        match value {
            InvalidTransactionV1::InvalidChainName { .. } => ErrorCode::InvalidTransactionChainName,
            InvalidTransactionV1::ExcessiveSize(_) => ErrorCode::InvalidTransactionExcessiveSize,
            InvalidTransactionV1::ExcessiveTimeToLive { .. } => {
                ErrorCode::InvalidTransactionExcessiveTimeToLive
            }
            InvalidTransactionV1::TimestampInFuture { .. } => {
                ErrorCode::InvalidTransactionTimestampInFuture
            }
            InvalidTransactionV1::InvalidBodyHash => ErrorCode::InvalidTransactionBodyHash,
            InvalidTransactionV1::InvalidTransactionHash => ErrorCode::InvalidTransactionHash,
            InvalidTransactionV1::EmptyApprovals => ErrorCode::InvalidTransactionEmptyApprovals,
            InvalidTransactionV1::InvalidApproval { .. } => {
                ErrorCode::InvalidTransactionInvalidApproval
            }
            InvalidTransactionV1::ExcessiveArgsLength { .. } => {
                ErrorCode::InvalidTransactionExcessiveArgsLength
            }
            InvalidTransactionV1::ExcessiveApprovals { .. } => {
                ErrorCode::InvalidTransactionExcessiveApprovals
            }
            InvalidTransactionV1::ExceedsBlockGasLimit { .. } => {
                ErrorCode::InvalidTransactionExceedsBlockGasLimit
            }
            InvalidTransactionV1::MissingArg { .. } => ErrorCode::InvalidTransactionMissingArg,
            InvalidTransactionV1::UnexpectedArgType { .. } => {
                ErrorCode::InvalidTransactionUnexpectedArgType
            }
            InvalidTransactionV1::InvalidArg { .. } => ErrorCode::InvalidTransactionInvalidArg,
            InvalidTransactionV1::InsufficientTransferAmount { .. } => {
                ErrorCode::InvalidTransactionInsufficientTransferAmount
            }
            InvalidTransactionV1::EntryPointCannotBeCustom { .. } => {
                ErrorCode::InvalidTransactionEntryPointCannotBeCustom
            }
            InvalidTransactionV1::EntryPointMustBeCustom { .. } => {
                ErrorCode::InvalidTransactionEntryPointMustBeCustom
            }
            InvalidTransactionV1::EmptyModuleBytes => ErrorCode::InvalidTransactionEmptyModuleBytes,
            InvalidTransactionV1::GasPriceConversion { .. } => {
                ErrorCode::InvalidTransactionGasPriceConversion
            }
            InvalidTransactionV1::UnableToCalculateGasLimit => {
                ErrorCode::InvalidTransactionUnableToCalculateGasLimit
            }
            InvalidTransactionV1::UnableToCalculateGasCost => {
                ErrorCode::InvalidTransactionUnableToCalculateGasCost
            }
            InvalidTransactionV1::InvalidPricingMode { .. } => {
                ErrorCode::InvalidTransactionPricingMode
            }
            InvalidTransactionV1::EntryPointCannotBeCall => {
                ErrorCode::InvalidTransactionEntryPointCannotBeCall
            }
            InvalidTransactionV1::InvalidTransactionLane(_) => {
                ErrorCode::InvalidTransactionInvalidTransactionKind
            }
            InvalidTransactionV1::GasPriceToleranceTooLow { .. } => {
                ErrorCode::GasPriceToleranceTooLow
            }
            _ => ErrorCode::InvalidTransactionUnspecified,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::convert::TryFrom;

    use strum::IntoEnumIterator;

    use crate::ErrorCode;

    #[test]
    fn try_from_decoded_all_variants() {
        for variant in ErrorCode::iter() {
            let as_int = variant as u16;
            let decoded = ErrorCode::try_from(as_int);
            assert!(
                decoded.is_ok(),
                "variant {} not covered by TryFrom<u16> implementation",
                as_int
            );
            assert_eq!(decoded.unwrap(), variant);
        }
    }
}
