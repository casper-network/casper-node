use datasize::DataSize;
use std::{
    fmt,
    fmt::{Display, Formatter},
};

use crate::types::{NodeId, SyncLeapIdentifier};

#[derive(Debug, Clone, DataSize)]
pub(crate) enum LeapActivityError {
    TooOld(SyncLeapIdentifier, Vec<NodeId>),
    Unobtainable(SyncLeapIdentifier, Vec<NodeId>),
    NoPeers(SyncLeapIdentifier),
}

impl Display for LeapActivityError {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            LeapActivityError::TooOld(sync_leap_identifier, ..) => {
                write!(formatter, "too old: {}", sync_leap_identifier)
            }
            LeapActivityError::Unobtainable(sync_leap_identifier, ..) => {
                write!(
                    formatter,
                    "unable to acquire data for: {}",
                    sync_leap_identifier
                )
            }
            LeapActivityError::NoPeers(sync_leap_identifier) => {
                write!(
                    formatter,
                    "sync leaper has no peers for: {}",
                    sync_leap_identifier
                )
            }
        }
    }
}
