use std::fmt::{self, Display, Formatter};

use casper_types::ExecutionResult;

use crate::{
    components::consensus::EraId,
    crypto::asymmetric_key::PublicKey,
    types::{BlockHash, BlockHeader, DeployHash, DeployHeader, FinalizedBlock, Timestamp},
};

#[derive(Debug)]
pub enum Event {
    BlockFinalized(Box<FinalizedBlock>),
    BlockAdded {
        block_hash: BlockHash,
        block_header: Box<BlockHeader>,
    },
    DeployProcessed {
        deploy_hash: DeployHash,
        deploy_header: Box<DeployHeader>,
        block_hash: BlockHash,
        execution_result: Box<ExecutionResult>,
    },
    Equivocation {
        era_id: EraId,
        public_key: PublicKey,
        timestamp: Timestamp,
    },
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Event::BlockFinalized(finalized_block) => write!(
                formatter,
                "block finalized {}",
                finalized_block.proto_block().hash()
            ),
            Event::BlockAdded { block_hash, .. } => write!(formatter, "block added {}", block_hash),
            Event::DeployProcessed { deploy_hash, .. } => {
                write!(formatter, "deploy processed {}", deploy_hash)
            }
            Event::Equivocation {
                era_id,
                public_key,
                timestamp,
            } => write!(
                formatter,
                "equivocation event detected for publickey:{} at time:{} in era:{}",
                public_key, timestamp, era_id,
            ),
        }
    }
}
