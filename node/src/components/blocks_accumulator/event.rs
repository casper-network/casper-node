use std::fmt::{self, Display, Formatter};

use crate::types::{BlockAdded, FinalitySignature, NodeId};

#[derive(Debug)]
pub(crate) enum Event {
    ReceivedBlock {
        block: Box<BlockAdded>,
        sender: NodeId,
    },
    ReceivedFinalitySignature {
        finality_signature: Box<FinalitySignature>,
        sender: NodeId,
    },
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::ReceivedBlock { block, sender } => {
                write!(f, "received {} from {}", block, sender)
            }
            Event::ReceivedFinalitySignature {
                finality_signature,
                sender,
            } => {
                write!(f, "received {} from {}", finality_signature, sender)
            }
        }
    }
}
