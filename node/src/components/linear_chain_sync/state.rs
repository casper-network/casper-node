use datasize::DataSize;

use crate::types::{BlockHash, BlockHeader};

#[derive(DataSize, Debug)]
pub enum State {
    NotStarted(BlockHash),
    NotGoingToSync,
    Syncing,
    Done(Box<BlockHeader>),
}
