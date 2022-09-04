use crate::types::{Block, BlockHash, FinalitySignature, NodeId};

#[derive(Debug)]
pub(crate) enum Event {
    ReceivedBlock {
        block: Block,
        sender: NodeId,
    },
    ReceivedFinalitySignature {
        finality_signature: FinalitySignature,
        sender: NodeId,
    },
}
