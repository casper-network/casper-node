use casper_types::bytesrepr;
use thiserror::Error;

use super::{
    metrics::DropReason,
    transfer_guard::{Direction, TransferViolation},
};

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error(transparent)]
    BytesRepr(#[from] bytesrepr::Error),
    #[error("received request without payload")]
    NoPayload,
    #[error(transparent)]
    BinaryPort(#[from] casper_binary_port::Error),
    #[error("dropping connection: {0}")]
    TransferViolation(TransferViolation),
}

impl Error {
    /// Reason under which the connection dropped due to this error is counted.
    pub(crate) fn drop_reason(&self) -> DropReason {
        match self {
            Error::BytesRepr(_) => DropReason::ResponseSerializationFailed,
            Error::NoPayload => DropReason::EmptyRequest,
            Error::BinaryPort(error) => match error {
                casper_binary_port::Error::RequestTooLarge { .. } => DropReason::RequestTooLarge,
                casper_binary_port::Error::EmptyRequest => DropReason::EmptyRequest,
                casper_binary_port::Error::InvalidCommandTag(_)
                | casper_binary_port::Error::BytesRepr(_) => DropReason::InvalidRequest,
                casper_binary_port::Error::Io(_) => DropReason::Io,
            },
            Error::TransferViolation(violation) => match violation.direction {
                Direction::Receive => DropReason::ReceiveTooSlow,
                Direction::Send => DropReason::SendTooSlow,
            },
        }
    }
}
