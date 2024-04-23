use core::{convert::TryFrom, fmt};

use casper_types::{InvalidDeploy, InvalidTransaction, InvalidTransactionV1};

/// The error code indicating the result of handling the binary request.
#[derive(Debug, Clone, thiserror::Error)]
#[repr(u8)]
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
    ///The transaction had an invalid chain name
    #[error("The deploy had an invalid chain name")]
    InvalidDeployChainName = 13,
    ///The transaction had a dependency that is no longer supported.
    #[error("The dependencies for this transaction are no longer supported")]
    InvalidDeployDependenciesNoLongerSupported = 14,
    ///The deploy sent to the network had an excessive size
    #[error("The deploy had an excessive size")]
    InvalidDeployExcessiveSize = 15,
    ///The deploy sent to the network had an excessive time to live
    #[error("The deploy had an excessive time to live")]
    InvalidDeployExcessiveTimeToLive = 16,
    ///The deploy sent to the network had a timestamp referencing a time that has yet to occur.
    #[error("The deploys timestamp is in the future")]
    InvalidDeployTimestampInFuture = 17,
    ///The deploy sent to the network had an invalid body hash
    #[error("The deploy had an invalid body hash")]
    InvalidDeployBodyHash = 18,
    ///The deploy sent to the network had an invalid deploy hash i.e. the provided deploy hash
    /// didn't match the derived deploy hash
    #[error("The deploy had an invalid deploy hash")]
    InvalidDeployHash = 19,
    ///The deploy sent to the network had an empty approval set
    #[error("The deploy had no approvals")]
    InvalidDeployEmptyApprovals = 20,
    ///The deploy sent to the network had an invalid approval
    #[error("The deploy had an invalid approval")]
    InvalidDeployApproval = 21,
    ///The deploy sent to the network had an excessive session args length
    #[error("The deploy had an excessive session args length")]
    InvalidDeployExcessiveSessionArgsLength = 22,
    ///The deploy sent to the network had an excessive payment args length
    #[error("The deploy had an excessive payment args length")]
    InvalidDeployExcessivePaymentArgsLength = 23,
    ///The deploy sent to the network had a missing payment amount
    #[error("The deploy had a missing payment amount")]
    InvalidDeployMisssingPaymentAmount = 24,
    ///The deploy sent to the network had a payment amount that was not parseable
    #[error("The deploy sent to the network had a payment amount that was unable to be parsed")]
    InvalidDeployFailedToParsePaymentAmount = 25,
    ///The deploy sent to the network exceeded the block gas limit
    #[error("The deploy sent to the network exceeded the block gas limit")]
    InvalidDeployExceededBlockGasLimit = 26,
    ///The deploy sent to the network was missing a transfer amount
    #[error("The deploy sent to the network was missing a transfer amount")]
    InvalidDeployMissingTransferAmount = 27,
    ///The deploy sent to the network had a transfer amount that was unable to be parseable
    #[error("The deploy sent to the network had a transfer amount that was unable to be parsed")]
    InvalidDeployFailedToParseTransferAmount = 28,
    ///The deploy sent to the network had a transfer amount that was insufficient
    #[error("The deploy sent to the network had an insufficient transfer amount")]
    InvalidDeployInsufficientTransferAmount = 29,
    ///The deploy sent to the network had excessive approvals
    #[error("The deploy sent to the network had excessive approvals")]
    InvalidDeployExcessiveApprovals = 30,
    ///The network was unable to calculate the gas limit for the deploy
    #[error("The network was unable to calculate the gas limit associated with the deploy")]
    InvalidDeployUnableToCalculateGasLimit = 31,
    ///The network was unble to calculate the gas cost for the deploy
    #[error("The network was unable to calculate the gas cost for the deploy")]
    InvalidDeployUnableToCalculateGasCost = 32,
    ///The deploy sent to the network was invalid for an unspecified reason
    #[error("The deploy sent to the network was invalid for an unspecified reason")]
    InvalidDeployUnspecified = 33,
    /// The transaction sent to the network had an invalid chain name
    #[error("The transaction sent to the network had an invalid chain name")]
    InvalidTransactionChainName = 34,
    /// The transaction sent to the network had an excessive size
    #[error("The transaction sent to the network had an excessive size")]
    InvalidTransactionExcessiveSize = 35,
    /// The transaction sent to the network had an excessive time to live
    #[error("The transaction sent to the network had an excessive time to live")]
    InvalidTransactionExcessiveTimeToLive = 36,
    /// The transaction sent to the network had a timestamp located in the future.
    #[error("The transaction sent to the network had a timestamp that has not yet occurred")]
    InvalidTransactionTimestampInFuture = 37,
    /// The transaction sent to the network had a provided body hash that conflicted with hash
    /// derived by the network
    #[error("The transaction sent to the network had an invalid body hash")]
    InvalidTransactionBodyHash = 38,
    /// The transaction sent to the network had a provided hash that conflicted with the hash
    /// dreived by the network
    #[error("The transaction sent to the network had an invalid body hash")]
    InvalidTransactionHash = 39,
    /// The transaction sent to the network had an emtpy approvals set
    #[error("The transaction sent to the network had no approvals")]
    InvalidTransactionEmptyApprovals = 40,
    /// The transaction sent to the network had an invalid approval
    #[error("The transaction sent to the network had an invalid approval")]
    InvalidTransactionInvalidApproval = 41,
    /// The transaction sent to the network had excessive args length
    #[error("The transaction sent to the network had excessive args length")]
    InvalidTransactionExcessiveArgsLength = 42,
    /// The transaction sent to the network had excessive approvals
    #[error("The transaction sent to the network had excessive approvals")]
    InvalidTransactionExcessiveApprovals = 43,
    /// The transaction sent to the network exceeds the block gas limit
    #[error("The transaction sent to the network exceeds the networks block gas limit")]
    InvalidTransactionExceedsBlockGasLimit = 44,
    /// The transaction sent to the network had a missing arg
    #[error("The transaction sent to the network was missing an argument")]
    InvalidTransactionMissingArg = 45,
    /// The transaction sent to the network had an argument with an unexpected type
    #[error("The transaction sent to the network had an unexpected argument type")]
    InvalidTransactionUnexpectedArgType = 46,
    /// The transaction sent to the network had an invalid argument
    #[error("The transaction sent to the network had an invalid argument")]
    InvalidTransactionInvalidArg = 47,
    /// The transaction sent to the network had an insufficient transfer amount
    #[error("The transaction sent to the network had an insufficient transfer amount")]
    InvalidTransactionInsufficientTransferAmount = 48,
    /// The transaction sent to the network had a custom entry point when it should have a non
    /// custom entry point.
    #[error("The transaction sent to the network should not have a custom entry point")]
    InvalidTransactionEntryPointCannotBeCustom = 49,
    /// The transaction sent to the network had a standard entry point when it must be custom.
    #[error("The transaction sent to the network must have a custom entry point")]
    InvalidTransactionEntryPointMustBeCustom = 50,
    /// The transaction sent to the network had empty module bytes
    #[error("The transaction sent to the network had empty module bytes")]
    InvalidTransactionEmptyModuleBytes = 51,
    /// The transaction sent to the network had an invalid gas price conversion
    #[error("The transaction sent to the network had an invalid gas price conversion")]
    InvalidTransactionGasPriceConversion = 52,
    /// The network was unable to calculate the gas limit for the transaction sent.
    #[error("The network was unable to calculate the gas limit for the transaction sent")]
    InvalidTransactionUnableToCalculateGasLimit = 53,
    /// The network was unable to calculate the gas limit for the transaction sent.
    #[error("The network was unable to calculate the gas limit for the transaction sent.")]
    InvalidTransactionUnableToCalculateGasCost = 54,
    /// The transaction sent to the network had an invalid pricing mode
    #[error("The transaction sent to the network had an invalid pricing mode")]
    InvalidTransactionPricingMode = 55,
    /// The transaction sent to the network was invalid for an unspecified reason
    #[error("The transaction sent to the network was invalid for an unspecified reason")]
    InvalidTransactionUnspecified = 56,
    ///As the various enums are tagged non_exhaustive, it is possible that in the future none of
    /// these previous errors cover the error that occurred, therefore we need some catchall in
    /// the case that nothing else works.
    #[error("The transaction or deploy sent to the network was invalid for an unspecified reason")]
    InvalidTransactionOrDeployUnspecified = 57,
}

impl TryFrom<u8> for ErrorCode {
    type Error = UnknownErrorCode;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
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
            24 => Ok(ErrorCode::InvalidDeployMisssingPaymentAmount),
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
            InvalidTransaction::Deploy(invalid_deploy) => handle_invalid_deploy(invalid_deploy),
            InvalidTransaction::V1(invalid_transaction) => {
                handle_invalid_transaction(invalid_transaction)
            }
            _ => ErrorCode::InvalidTransactionOrDeployUnspecified,
        }
    }
}

fn handle_invalid_deploy(invalid_deploy: InvalidDeploy) -> ErrorCode {
    match invalid_deploy {
        InvalidDeploy::InvalidChainName { .. } => ErrorCode::InvalidDeployChainName,
        InvalidDeploy::DependenciesNoLongerSupported => {
            ErrorCode::InvalidDeployDependenciesNoLongerSupported
        }
        InvalidDeploy::ExcessiveSize(_) => ErrorCode::InvalidDeployExcessiveSize,
        InvalidDeploy::ExcessiveTimeToLive { .. } => ErrorCode::InvalidDeployExcessiveTimeToLive,
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
        InvalidDeploy::MissingPaymentAmount => ErrorCode::InvalidDeployMisssingPaymentAmount,
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
        InvalidDeploy::UnableToCalculateGasCost => ErrorCode::InvalidDeployUnableToCalculateGasCost,
        _ => ErrorCode::InvalidDeployUnspecified,
    }
}

fn handle_invalid_transaction(invalid_transaction: InvalidTransactionV1) -> ErrorCode {
    match invalid_transaction {
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
        InvalidTransactionV1::InvalidPricingMode { .. } => ErrorCode::InvalidTransactionPricingMode,
        _ => ErrorCode::InvalidTransactionUnspecified,
    }
}
