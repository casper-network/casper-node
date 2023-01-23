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
            LeapActivityError::TooOld(bh, ..) => write!(formatter, "too old: {}", bh),
            LeapActivityError::Unobtainable(bh, ..) => {
                write!(formatter, "unable to acquire data for: {}", bh)
            }
            LeapActivityError::NoPeers(bh) => {
                write!(formatter, "sync leaper has no peers for: {}", bh)
            }
        }
    }
}
