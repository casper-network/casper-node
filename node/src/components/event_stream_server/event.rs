use std::fmt::{self, Display, Formatter};

use casper_types::{EraId, ExecutionEffect, ExecutionResult, PublicKey};

use crate::{
    components::consensus::EraId,
    types::{Block, BlockHash, DeployHash, DeployHeader, FinalitySignature, Timestamp},
};

#[derive(Debug)]
pub enum Event {
    BlockAdded {
        block_hash: BlockHash,
        block: Box<Block>,
    },
    DeployProcessed {
        deploy_hash: DeployHash,
        deploy_header: Box<DeployHeader>,
        block_hash: BlockHash,
        execution_result: Box<ExecutionResult>,
    },
    Fault {
        era_id: EraId,
        public_key: PublicKey,
        timestamp: Timestamp,
    },
    FinalitySignature(Box<FinalitySignature>),
    Step {
        era_id: EraId,
        effect: ExecutionEffect,
    },
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Event::BlockAdded { block_hash, .. } => write!(formatter, "block added {}", block_hash),
            Event::DeployProcessed { deploy_hash, .. } => {
                write!(formatter, "deploy processed {}", deploy_hash)
            }
            Event::Fault {
                era_id,
                public_key,
                timestamp,
            } => write!(
                formatter,
                "An equivocator with public key: {} has been identified at time: {} in era: {}",
                public_key, timestamp, era_id,
            ),
            Event::FinalitySignature(fs) => write!(formatter, "finality signature {}", fs),
            Event::Step { era_id, .. } => write!(formatter, "step committed for {}", era_id),
        }
    }
}
