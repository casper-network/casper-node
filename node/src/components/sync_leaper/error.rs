use datasize::DataSize;
use std::{
    fmt,
    fmt::{Display, Formatter},
};

use crate::types::{BlockHash, NodeId};

#[derive(Debug, Clone, DataSize)]
pub(crate) enum LeapActivityError {
    TooOld(BlockHash, Vec<NodeId>),
    Unobtainable(BlockHash, Vec<NodeId>),
    NoPeers(BlockHash),
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
