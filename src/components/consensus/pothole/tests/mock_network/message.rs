use super::super::super::BlockIndex;

use super::{Block, Transaction};

/// Enum representing possible network messages
#[derive(Debug)]
pub(crate) enum NetworkMessage {
    NewTransaction(Transaction),
    NewFinalizedBlock(BlockIndex, Block),
}
