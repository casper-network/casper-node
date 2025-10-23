use core::{convert::TryFrom, fmt};

use casper_types::{InvalidDeploy, InvalidTransaction, InvalidTransactionV1};

use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
#[cfg(test)]
use strum_macros::EnumIter;

/// The error code indicating the result of handling the binary request.
#[derive(Debug, Copy, Clone, thiserror::Error, Eq, PartialEq, FromPrimitive)]
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
    /// Internal error.
    #[error("internal error")]
    InternalError = 6,
    /// The query failed.
    #[error("the query failed")]
    FailedQuery = 7,
    /// Bad request.
    #[error("bad request")]
    BadRequest = 8,
    /// Received an unsupported type of request.
    #[error("unsupported request")]
    UnsupportedRequest = 9,
    /// Dictionary URef not found.
    #[error("dictionary URef not found")]
    DictionaryURefNotFound = 10,
    /// This node has no complete blocks.
    #[error("no complete blocks")]
    NoCompleteBlocks = 11,
    /// The deploy had an invalid chain name
    #[error("the deploy had an invalid chain name")]
    InvalidDeployChainName = 12,
    /// Deploy dependencies are no longer supported
    #[error("the dependencies for this transaction are no longer supported")]
    InvalidDeployDependenciesNoLongerSupported = 13,
    /// The deploy sent to the network had an excessive size
    #[error("the deploy had an excessive size")]
    InvalidDeployExcessiveSize = 14,
    /// The deploy sent to the network had an excessive time to live
    #[error("the deploy had an excessive time to live")]
    InvalidDeployExcessiveTimeToLive = 15,
    /// The deploy sent to the network had a timestamp referencing a time that has yet to occur.
    #[error("the deploys timestamp is in the future")]
    InvalidDeployTimestampInFuture = 16,
    /// The deploy sent to the network had an invalid body hash
    #[error("the deploy had an invalid body hash")]
    InvalidDeployBodyHash = 17,
    /// The deploy sent to the network had an invalid deploy hash i.e. the provided deploy hash
    /// didn't match the derived deploy hash
    #[error("the deploy had an invalid deploy hash")]
    InvalidDeployHash = 18,
    /// The deploy sent to the network had an empty approval set
    #[error("the deploy had no approvals")]
    InvalidDeployEmptyApprovals = 19,
    /// The deploy sent to the network had an invalid approval
    #[error("the deploy had an invalid approval")]
    InvalidDeployApproval = 20,
    /// The deploy sent to the network had an excessive session args length
    #[error("the deploy had an excessive session args length")]
    InvalidDeployExcessiveSessionArgsLength = 21,
    /// The deploy sent to the network had an excessive payment args length
    #[error("the deploy had an excessive payment args length")]
    InvalidDeployExcessivePaymentArgsLength = 22,
    /// The deploy sent to the network had a missing payment amount
    #[error("the deploy had a missing payment amount")]
    InvalidDeployMissingPaymentAmount = 23,
    /// The deploy sent to the network had a payment amount that was not parseable
    #[error("the deploy sent to the network had a payment amount that was unable to be parsed")]
    InvalidDeployFailedToParsePaymentAmount = 24,
    /// The deploy sent to the network exceeded the block gas limit
    #[error("the deploy sent to the network exceeded the block gas limit")]
    InvalidDeployExceededBlockGasLimit = 25,
    /// The deploy sent to the network was missing a transfer amount
    #[error("the deploy sent to the network was missing a transfer amount")]
    InvalidDeployMissingTransferAmount = 26,
    /// The deploy sent to the network had a transfer amount that was unable to be parseable
    #[error("the deploy sent to the network had a transfer amount that was unable to be parsed")]
    InvalidDeployFailedToParseTransferAmount = 27,
    /// The deploy sent to the network had a transfer amount that was insufficient
    #[error("the deploy sent to the network had an insufficient transfer amount")]
    InvalidDeployInsufficientTransferAmount = 28,
    /// The deploy sent to the network had excessive approvals
    #[error("the deploy sent to the network had excessive approvals")]
    InvalidDeployExcessiveApprovals = 29,
    /// The network was unable to calculate the gas limit for the deploy
    #[error("the network was unable to calculate the gas limit associated with the deploy")]
    InvalidDeployUnableToCalculateGasLimit = 30,
    /// The network was unable to calculate the gas cost for the deploy
    #[error("the network was unable to calculate the gas cost for the deploy")]
    InvalidDeployUnableToCalculateGasCost = 31,
    /// The deploy sent to the network was invalid for an unspecified reason
    #[error("the deploy sent to the network was invalid for an unspecified reason")]
    InvalidDeployUnspecified = 32,
    /// The transaction sent to the network had an invalid chain name
    #[error("the transaction sent to the network had an invalid chain name")]
    InvalidTransactionChainName = 33,
    /// The transaction sent to the network had an excessive size
    #[error("the transaction sent to the network had an excessive size")]
    InvalidTransactionExcessiveSize = 34,
    /// The transaction sent to the network had an excessive time to live
    #[error("the transaction sent to the network had an excessive time to live")]
    InvalidTransactionExcessiveTimeToLive = 35,
    /// The transaction sent to the network had a timestamp located in the future.
    #[error("the transaction sent to the network had a timestamp that has not yet occurred")]
    InvalidTransactionTimestampInFuture = 36,
    /// The transaction sent to the network had a provided body hash that conflicted with hash
    /// derived by the network
    #[error("the transaction sent to the network had an invalid body hash")]
    InvalidTransactionBodyHash = 37,
    /// The transaction sent to the network had a provided hash that conflicted with the hash
    /// derived by the network
    #[error("the transaction sent to the network had an invalid hash")]
    InvalidTransactionHash = 38,
    /// The transaction sent to the network had an empty approvals set
    #[error("the transaction sent to the network had no approvals")]
    InvalidTransactionEmptyApprovals = 39,
    /// The transaction sent to the network had an invalid approval
    #[error("the transaction sent to the network had an invalid approval")]
    InvalidTransactionInvalidApproval = 40,
    /// The transaction sent to the network had excessive args length
    #[error("the transaction sent to the network had excessive args length")]
    InvalidTransactionExcessiveArgsLength = 41,
    /// The transaction sent to the network had excessive approvals
    #[error("the transaction sent to the network had excessive approvals")]
    InvalidTransactionExcessiveApprovals = 42,
    /// The transaction sent to the network exceeds the block gas limit
    #[error("the transaction sent to the network exceeds the networks block gas limit")]
    InvalidTransactionExceedsBlockGasLimit = 43,
    /// The transaction sent to the network had a missing arg
    #[error("the transaction sent to the network was missing an argument")]
    InvalidTransactionMissingArg = 44,
    /// The transaction sent to the network had an argument with an unexpected type
    #[error("the transaction sent to the network had an unexpected argument type")]
    InvalidTransactionUnexpectedArgType = 45,
    /// The transaction sent to the network had an invalid argument
    #[error("the transaction sent to the network had an invalid argument")]
    InvalidTransactionInvalidArg = 46,
    /// The transaction sent to the network had an insufficient transfer amount
    #[error("the transaction sent to the network had an insufficient transfer amount")]
    InvalidTransactionInsufficientTransferAmount = 47,
    /// The transaction sent to the network had a custom entry point when it should have a
    /// non-custom entry point.
    #[error("the native transaction sent to the network should not have a custom entry point")]
    InvalidTransactionEntryPointCannotBeCustom = 48,
    /// The transaction sent to the network had a standard entry point when it must be custom.
    #[error("the non-native transaction sent to the network must have a custom entry point")]
    InvalidTransactionEntryPointMustBeCustom = 49,
    /// The transaction sent to the network had empty module bytes
    #[error("the transaction sent to the network had empty module bytes")]
    InvalidTransactionEmptyModuleBytes = 50,
    /// The transaction sent to the network had an invalid gas price conversion
    #[error("the transaction sent to the network had an invalid gas price conversion")]
    InvalidTransactionGasPriceConversion = 51,
    /// The network was unable to calculate the gas limit for the transaction sent.
    #[error("the network was unable to calculate the gas limit for the transaction sent")]
    InvalidTransactionUnableToCalculateGasLimit = 52,
    /// The network was unable to calculate the gas cost for the transaction sent.
    #[error("the network was unable to calculate the gas cost for the transaction sent.")]
    InvalidTransactionUnableToCalculateGasCost = 53,
    /// The transaction sent to the network had an invalid pricing mode
    #[error("the transaction sent to the network had an invalid pricing mode")]
    InvalidTransactionPricingMode = 54,
    /// The transaction sent to the network was invalid for an unspecified reason
    #[error("the transaction sent to the network was invalid for an unspecified reason")]
    InvalidTransactionUnspecified = 55,
    /// As the various enums are tagged non_exhaustive, it is possible that in the future none of
    /// these previous errors cover the error that occurred, therefore we need some catchall in
    /// the case that nothing else works.
    #[error("the transaction or deploy sent to the network was invalid for an unspecified reason")]
    InvalidTransactionOrDeployUnspecified = 56,
    /// The switch block for the requested era was not found
    #[error("the switch block for the requested era was not found")]
    SwitchBlockNotFound = 57,
    #[error("the parent of the switch block for the requested era was not found")]
    /// The parent of the switch block for the requested era was not found
    SwitchBlockParentNotFound = 58,
    #[error("cannot serve rewards stored in V1 format")]
    /// Cannot serve rewards stored in V1 format
    UnsupportedRewardsV1Request = 59,
    /// Invalid binary request header versions.
    #[error("binary request header versions mismatch")]
    CommandHeaderVersionMismatch = 60,
    /// Blockchain is empty
    #[error("blockchain is empty")]
    EmptyBlockchain = 61,
    /// Expected deploy, but got transaction
    #[error("expected deploy, got transaction")]
    ExpectedDeploy = 62,
    /// Expected transaction, but got deploy
    #[error("expected transaction V1, got deploy")]
    ExpectedTransaction = 63,
    /// Transaction has expired
    #[error("transaction has expired")]
    TransactionExpired = 64,
    /// Transactions parameters are missing or incorrect
    #[error("missing or incorrect transaction parameters")]
    MissingOrIncorrectParameters = 65,
    /// No such addressable entity
    #[error("no such addressable entity")]
    NoSuchAddressableEntity = 66,
    // No such contract at hash
    #[error("no such contract at hash")]
    NoSuchContractAtHash = 67,
    /// No such entry point
    #[error("no such entry point")]
    NoSuchEntryPoint = 68,
    /// No such package at hash
    #[error("no such package at hash")]
    NoSuchPackageAtHash = 69,
    /// Invalid entity at version
    #[error("invalid entity at version")]
    InvalidEntityAtVersion = 70,
    /// Disabled entity at version
    #[error("disabled entity at version")]
    DisabledEntityAtVersion = 71,
    /// Missing entity at version
    #[error("missing entity at version")]
    MissingEntityAtVersion = 72,
    /// Invalid associated keys
    #[error("invalid associated keys")]
    InvalidAssociatedKeys = 73,
    /// Insufficient signature weight
    #[error("insufficient signature weight")]
    InsufficientSignatureWeight = 74,
    /// Insufficient balance
    #[error("insufficient balance")]
    InsufficientBalance = 75,
    /// Unknown balance
    #[error("unknown balance")]
    UnknownBalance = 76,
    /// Invalid payment variant for deploy
    #[error("invalid payment variant for deploy")]
    DeployInvalidPaymentVariant = 77,
    /// Missing payment amount for deploy
    #[error("missing payment amount for deploy")]
    DeployMissingPaymentAmount = 78,
    /// Failed to parse payment amount for deploy
    #[error("failed to parse payment amount for deploy")]
    DeployFailedToParsePaymentAmount = 79,
    /// Missing transfer target for deploy
    #[error("missing transfer target for deploy")]
    DeployMissingTransferTarget = 80,
    /// Missing module bytes for deploy
    #[error("missing module bytes for deploy")]
    DeployMissingModuleBytes = 81,
    /// Entry point cannot be 'call'
    #[error("entry point cannot be 'call'")]
    InvalidTransactionEntryPointCannotBeCall = 82,
    /// Invalid transaction lane
    #[error("invalid transaction lane")]
    InvalidTransactionInvalidTransactionLane = 83,
    /// Gas price tolerance too low
    #[error("gas price tolerance too low")]
    GasPriceToleranceTooLow = 84,
    /// Received V1 Transaction for spec exec.
    #[error("received v1 transaction for speculative execution")]
    ReceivedV1Transaction = 85,
    /// Purse was not found for given identifier.
    #[error("purse was not found for given identifier")]
    PurseNotFound = 86,
    /// Too many requests per second.
    #[error("request was throttled")]
    RequestThrottled = 87,
    /// Expected named arguments.
    #[error("expected named arguments")]
    ExpectedNamedArguments = 88,
    /// Invalid transaction runtime.
    #[error("invalid transaction runtime")]
    InvalidTransactionRuntime = 89,
    /// Key in transfer request malformed
    #[error("malformed transfer record key")]
    TransferRecordMalformedKey = 90,
    /// Malformed information request
    #[error("malformed information request")]
    MalformedInformationRequest = 91,
    /// Malformed binary version
    #[error("not enough bytes to read version of the binary request header")]
    TooLittleBytesForRequestHeaderVersion = 92,
    /// Malformed command header version
    #[error("malformed command header version")]
    MalformedCommandHeaderVersion = 93,
    /// Malformed header
    #[error("malformed command header")]
    MalformedCommandHeader = 94,
    /// Malformed command
    #[error("malformed command")]
    MalformedCommand = 95,
    /// No matching lane for transaction
    #[error("couldn't associate a transaction lane with the transaction")]
    InvalidTransactionNoLaneMatches = 96,
    /// Entry point must be 'call'
    #[error("entry point must be 'call'")]
    InvalidTransactionEntryPointMustBeCall = 97,
    /// One of the payloads field cannot be deserialized
    #[error("One of the payloads field cannot be deserialized")]
    InvalidTransactionCannotDeserializeField = 98,
    /// Can't calculate hash of the payload fields
    #[error("Can't calculate hash of the payload fields")]
    InvalidTransactionCannotCalculateFieldsHash = 99,
    /// Unexpected fields in payload
    #[error("Unexpected fields in payload")]
    InvalidTransactionUnexpectedFields = 100,
    /// Expected bytes arguments
    #[error("expected bytes arguments")]
    InvalidTransactionExpectedBytesArguments = 101,
    /// Missing seed field in transaction
    #[error("Missing seed field in transaction")]
    InvalidTransactionMissingSeed = 102,
    /// Pricing mode not supported
    #[error("Pricing mode not supported")]
    PricingModeNotSupported = 103,
    /// Gas limit not supported
    #[error("Gas limit not supported")]
    InvalidDeployGasLimitNotSupported = 104,
    /// Invalid runtime for Transaction::Deploy
    #[error("Invalid runtime for Transaction::Deploy")]
    InvalidDeployInvalidRuntime = 105,
    /// Deploy exceeds wasm lane gas limit
    #[error("Transaction::Deploy exceeds lane gas limit")]
    InvalidDeployExceededWasmLaneGasLimit = 106,
    /// Invalid runtime for Transaction::Deploy
    #[error("Invalid payment amount for Transaction::Deploy")]
    InvalidDeployInvalidPaymentAmount = 107,
    /// Insufficient burn amount for Transaction::V1
    #[error("Insufficient burn amount for Transaction::V1")]
    InvalidTransactionInsufficientBurnAmount = 108,
    /// Invalid payment amount for Transaction::V1
    #[error("Invalid payment amount for Transaction::V1")]
    InvalidTransactionInvalidPaymentAmount = 109,
    /// Unexpected entry point for Transaction::V1
    #[error("Unexpected entry point for Transaction::V1")]
    InvalidTransactionUnexpectedEntryPoint = 110,
    /// Cannot serialize transaction
    #[error("Transaction has malformed binary representation")]
    TransactionHasMalformedBinaryRepresentation = 111,
    #[error("Transaction includes an argument named amount with a value below a relevant limit")]
    InsufficientAmountArgValue = 112,
    #[error(
        "Transaction attempts to set a minimum delegation amount below the lowest allowed value"
    )]
    InvalidMinimumDelegationAmount = 113,
    #[error(
        "Transaction attempts to set a maximum delegation amount above the highest allowed value"
    )]
    InvalidMaximumDelegationAmount = 114,
    #[error("Transaction attempts to set a reserved slots count above the highest allowed value")]
    InvalidReservedSlots = 115,
    #[error("Transaction attempts to set a delegation amount above the highest allowed value")]
    InvalidDelegationAmount = 116,
    #[error("the transaction invocation target is unsupported under V2 runtime")]
    UnsupportedInvocationTarget = 117,
    #[error("Sandboxed execution failed")]
    SandboxedExecutionFailed = 118,
}

impl TryFrom<u16> for ErrorCode {
    type Error = UnknownErrorCode;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        FromPrimitive::from_u16(value).ok_or(UnknownErrorCode)
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
            InvalidDeploy::GasLimitNotSupported => ErrorCode::InvalidDeployGasLimitNotSupported,
            InvalidDeploy::InvalidRuntime => ErrorCode::InvalidDeployInvalidRuntime,
            InvalidDeploy::NoLaneMatch => ErrorCode::InvalidTransactionNoLaneMatches,
            InvalidDeploy::ExceededLaneGasLimit { .. } => {
                ErrorCode::InvalidDeployExceededWasmLaneGasLimit
            }
            InvalidDeploy::InvalidPaymentAmount => ErrorCode::InvalidDeployInvalidPaymentAmount,
            InvalidDeploy::PricingModeNotSupported => ErrorCode::PricingModeNotSupported,
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
                ErrorCode::InvalidTransactionInvalidTransactionLane
            }
            InvalidTransactionV1::GasPriceToleranceTooLow { .. } => {
                ErrorCode::GasPriceToleranceTooLow
            }
            InvalidTransactionV1::ExpectedNamedArguments => ErrorCode::ExpectedNamedArguments,
            InvalidTransactionV1::InvalidTransactionRuntime { .. } => {
                ErrorCode::InvalidTransactionRuntime
            }
            InvalidTransactionV1::NoLaneMatch => ErrorCode::InvalidTransactionNoLaneMatches,
            InvalidTransactionV1::EntryPointMustBeCall { .. } => {
                ErrorCode::InvalidTransactionEntryPointMustBeCall
            }
            InvalidTransactionV1::CouldNotDeserializeField { .. } => {
                ErrorCode::InvalidTransactionCannotDeserializeField
            }
            InvalidTransactionV1::CannotCalculateFieldsHash => {
                ErrorCode::InvalidTransactionCannotCalculateFieldsHash
            }
            InvalidTransactionV1::UnexpectedTransactionFieldEntries => {
                ErrorCode::InvalidTransactionUnexpectedFields
            }
            InvalidTransactionV1::ExpectedBytesArguments => {
                ErrorCode::InvalidTransactionExpectedBytesArguments
            }

            InvalidTransactionV1::PricingModeNotSupported => ErrorCode::PricingModeNotSupported,
            InvalidTransactionV1::InsufficientBurnAmount { .. } => {
                ErrorCode::InvalidTransactionInsufficientBurnAmount
            }
            InvalidTransactionV1::InvalidPaymentAmount => {
                ErrorCode::InvalidTransactionInvalidPaymentAmount
            }
            InvalidTransactionV1::UnexpectedEntryPoint { .. } => {
                ErrorCode::InvalidTransactionUnexpectedEntryPoint
            }
            InvalidTransactionV1::CouldNotSerializeTransaction { .. } => {
                ErrorCode::TransactionHasMalformedBinaryRepresentation
            }
            InvalidTransactionV1::InsufficientAmount { .. } => {
                ErrorCode::InsufficientAmountArgValue
            }
            InvalidTransactionV1::InvalidMinimumDelegationAmount { .. } => {
                ErrorCode::InvalidMinimumDelegationAmount
            }
            InvalidTransactionV1::InvalidMaximumDelegationAmount { .. } => {
                ErrorCode::InvalidMaximumDelegationAmount
            }
            InvalidTransactionV1::InvalidReservedSlots { .. } => ErrorCode::InvalidReservedSlots,
            InvalidTransactionV1::InvalidDelegationAmount { .. } => {
                ErrorCode::InvalidDelegationAmount
            }
            InvalidTransactionV1::UnsupportedInvocationTarget { .. } => {
                ErrorCode::UnsupportedInvocationTarget
            }
            InvalidTransactionV1::MissingSeed => ErrorCode::InvalidTransactionMissingSeed,
            _other => ErrorCode::InvalidTransactionUnspecified,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::convert::TryFrom;

    use crate::ErrorCode;
    use casper_types::{InvalidDeploy, InvalidTransactionV1};
    use strum::IntoEnumIterator;

    #[test]
    fn verify_all_invalid_transaction_v1_errors_have_error_codes() {
        for error in InvalidTransactionV1::iter() {
            let code = ErrorCode::from(error.clone());
            assert_ne!(
                code,
                ErrorCode::InvalidTransactionUnspecified,
                "Seems like InvalidTransactionV1 {error:?} has no corresponding error code"
            );
            assert_ne!(
                code,
                ErrorCode::InvalidDeployUnspecified,
                "Seems like InvalidTransactionV1 {error:?} has no corresponding error code"
            )
        }
    }

    #[test]
    fn verify_all_invalid_deploy_errors_have_error_codes() {
        for error in InvalidDeploy::iter() {
            let code = ErrorCode::from(error.clone());
            assert_ne!(
                code,
                ErrorCode::InvalidTransactionUnspecified,
                "Seems like InvalidDeploy {error} has no corresponding error code"
            );
            assert_ne!(
                code,
                ErrorCode::InvalidDeployUnspecified,
                "Seems like InvalidDeploy {error} has no corresponding error code"
            )
        }
    }

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
