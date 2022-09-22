use datasize::DataSize;

use crate::types::{BlockHash, NodeId};

#[derive(Debug, Clone, DataSize)]
pub(crate) enum LeapActivityError {
    TooOld(BlockHash, Vec<NodeId>),
    Unobtainable(BlockHash, Vec<NodeId>),
    TooBusy(BlockHash),
    NoPeers(BlockHash),
}

#[derive(Debug, Clone, DataSize)]
pub(crate) enum ConstructSyncLeapError {
    TooOld(BlockHash),
    UnknownHash(BlockHash),
    CouldntProve,
    StorageError,
}
