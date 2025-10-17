//! Error code for signaling error while processing a host function.
//!
//! API inspired by `std::io::Error` and `std::io::ErrorKind` but somewhat more memory efficient.

use thiserror::Error;

#[derive(Debug, Default, PartialEq)]
#[non_exhaustive]
#[repr(u32)]
pub enum HostResult {
    #[default]
    Success = 0,
    /// An entity was not found, often a missing key in the global state.
    NotFound = 1,
    /// Data not valid for the operation were encountered.
    ///
    /// As an example this could be a malformed parameter that does not contain a valid UTF-8.
    InvalidData = 2,
    /// The input to the host function was invalid.
    InvalidInput = 3,
    /// The topic is too long.
    TopicTooLong = 4,
    /// Too many topics.
    TooManyTopics = 5,
    /// The payload is too long.
    PayloadTooLong = 6,
    /// The message topic is full and cannot accept new messages.
    MessageTopicFull = 7,
    /// The maximum number of messages emitted per block was exceeded when trying to emit a
    /// message.
    MaxMessagesPerBlockExceeded = 8,
    /// Internal error (for example, failed to acquire a lock)
    Internal = 9,
    /// Error related to CLValues
    CLValue = 10,
    /// An error code not covered by the other variants.
    Other(u32),
}

pub const HOST_ERROR_SUCCESS: u32 = 0;
pub const HOST_ERROR_NOT_FOUND: u32 = 1;
pub const HOST_ERROR_INVALID_DATA: u32 = 2;
pub const HOST_ERROR_INVALID_INPUT: u32 = 3;
pub const HOST_ERROR_TOPIC_TOO_LONG: u32 = 4;
pub const HOST_ERROR_TOO_MANY_TOPICS: u32 = 5;
pub const HOST_ERROR_PAYLOAD_TOO_LONG: u32 = 6;
pub const HOST_ERROR_MESSAGE_TOPIC_FULL: u32 = 7;
pub const HOST_ERROR_MAX_MESSAGES_PER_BLOCK_EXCEEDED: u32 = 8;
pub const HOST_ERROR_INTERNAL: u32 = 9;
pub const HOST_ERROR_CL_VALUE: u32 = 10;

impl From<u32> for HostResult {
    fn from(value: u32) -> Self {
        match value {
            HOST_ERROR_SUCCESS => Self::Success,
            HOST_ERROR_NOT_FOUND => Self::NotFound,
            HOST_ERROR_INVALID_DATA => Self::InvalidData,
            HOST_ERROR_INVALID_INPUT => Self::InvalidInput,
            HOST_ERROR_TOPIC_TOO_LONG => Self::TopicTooLong,
            HOST_ERROR_TOO_MANY_TOPICS => Self::TooManyTopics,
            HOST_ERROR_PAYLOAD_TOO_LONG => Self::PayloadTooLong,
            HOST_ERROR_MESSAGE_TOPIC_FULL => Self::MessageTopicFull,
            HOST_ERROR_MAX_MESSAGES_PER_BLOCK_EXCEEDED => Self::MaxMessagesPerBlockExceeded,
            HOST_ERROR_INTERNAL => Self::Internal,
            HOST_ERROR_CL_VALUE => Self::CLValue,
            other => Self::Other(other),
        }
    }
}

pub fn result_from_code(code: u32) -> Result<(), HostResult> {
    match code {
        HOST_ERROR_SUCCESS => Ok(()),
        other => Err(HostResult::from(other)),
    }
}

/// Wasm trap code.
#[derive(Debug, Error)]
pub enum TrapCode {
    /// Trap code for out of bounds memory access.
    #[error("call stack exhausted")]
    StackOverflow,
    /// Trap code for out of bounds memory access.
    #[error("out of bounds memory access")]
    MemoryOutOfBounds,
    /// Trap code for out of bounds table access.
    #[error("undefined element: out of bounds table access")]
    TableAccessOutOfBounds,
    /// Trap code for indirect call to null.
    #[error("uninitialized element")]
    IndirectCallToNull,
    /// Trap code for indirect call type mismatch.
    #[error("indirect call type mismatch")]
    BadSignature,
    /// Trap code for integer overflow.
    #[error("integer overflow")]
    IntegerOverflow,
    /// Trap code for division by zero.
    #[error("integer divide by zero")]
    IntegerDivisionByZero,
    /// Trap code for invalid conversion to integer.
    #[error("invalid conversion to integer")]
    BadConversionToInteger,
    /// Trap code for unreachable code reached triggered by unreachable instruction.
    #[error("unreachable")]
    UnreachableCodeReached,
}

pub const CALLEE_SUCCEEDED: u32 = 0;
pub const CALLEE_ROLLED_BACK: u32 = 1;
pub const CALLEE_TRAPPED: u32 = 2;
pub const CALLEE_INPUT_INVALID: u32 = 3;
pub const CALLEE_GAS_DEPLETED: u32 = 4;
pub const CALLEE_NOT_CALLABLE: u32 = 5;
pub const CALLEE_API_ERROR: u32 = 6;
pub const CALLEE_NO_ACTIVE_CONTRACT: u32 = 7;
pub const CALLEE_CODE_NOT_FOUND: u32 = 8;
pub const CALLEE_ENTITY_NOT_FOUND: u32 = 9;
pub const CALLEE_LOCKED_PACKAGE: u32 = 10;
/// Represents the result of a host function call.
///
/// 0 is used as a success.
#[derive(Debug, Error)]
pub enum CallError {
    /// Callee contract reverted.
    #[error("callee reverted")]
    CalleeRolledBack,
    /// Called contract trapped.
    #[error("callee trapped: {0}")]
    CalleeTrapped(TrapCode),
    /// Callee input data was invalid
    #[error("callee input invalid")]
    InputInvalid,
    /// Called contract reached gas limit.
    #[error("callee gas depleted")]
    CalleeGasDepleted,
    /// Called contract is not callable.
    #[error("not callable")]
    NotCallable,
    /// No active version found in the package.
    #[error("no active contract")]
    NoActiveContract,
    /// No byte code was found for execution.
    #[error("code not found")]
    CodeNotFound,
    /// Entity not found.
    #[error("entity not found")]
    EntityNotFound,
    /// Package associated with the package was locked.
    #[error("locked package")]
    LockedPackage,
    /// System is callee and signaled vm instance kill.
    #[error("kill the vm instance of the caller")]
    Api(String),
}

impl CallError {
    /// Converts the host error into a u32.
    #[must_use]
    pub fn into_u32(self) -> u32 {
        match self {
            Self::InputInvalid => CALLEE_INPUT_INVALID,
            Self::CalleeRolledBack => CALLEE_ROLLED_BACK,
            Self::CalleeTrapped(_) => CALLEE_TRAPPED,
            Self::CalleeGasDepleted => CALLEE_GAS_DEPLETED,
            Self::NotCallable => CALLEE_NOT_CALLABLE,
            Self::NoActiveContract => CALLEE_NO_ACTIVE_CONTRACT,
            Self::CodeNotFound => CALLEE_CODE_NOT_FOUND,
            Self::EntityNotFound => CALLEE_ENTITY_NOT_FOUND,
            Self::LockedPackage => CALLEE_LOCKED_PACKAGE,
            Self::Api(_) => CALLEE_API_ERROR,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_u32_not_found() {
        let error = HostResult::from(HOST_ERROR_NOT_FOUND);
        assert_eq!(error, HostResult::NotFound);
    }

    #[test]
    fn test_from_u32_invalid_data() {
        let error = HostResult::from(HOST_ERROR_INVALID_DATA);
        assert_eq!(error, HostResult::InvalidData);
    }

    #[test]
    fn test_from_u32_invalid_input() {
        let error = HostResult::from(HOST_ERROR_INVALID_INPUT);
        assert_eq!(error, HostResult::InvalidInput);
    }

    #[test]
    fn test_from_u32_other() {
        let error = HostResult::from(10);
        assert_eq!(error, HostResult::Other(10));
    }
}
