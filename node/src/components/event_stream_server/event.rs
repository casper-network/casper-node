use std::fmt::{self, Display, Formatter};

use casper_types::{EraId, ExecutionEffect, ExecutionResult, PublicKey, Timestamp};
use itertools::Itertools;

use crate::types::{Block, BlockHash, Deploy, DeployHash, DeployHeader, FinalitySignature};

#[derive(Debug)]
pub enum Event {
    BlockAdded(Box<Block>),
    DeployAccepted(Box<Deploy>),
    DeployProcessed {
        deploy_hash: DeployHash,
        deploy_header: Box<DeployHeader>,
        block_hash: BlockHash,
        execution_result: Box<ExecutionResult>,
    },
    DeploysExpired(Vec<DeployHash>),
    Fault {
        era_id: EraId,
        public_key: PublicKey,
        timestamp: Timestamp,
    },
    FinalitySignature(Box<FinalitySignature>),
    Step {
        era_id: EraId,
        execution_effect: ExecutionEffect,
    },
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Event::BlockAdded(block) => write!(formatter, "block added {}", block.hash()),
            Event::DeployAccepted(deploy_hash) => {
                write!(formatter, "deploy accepted {}", deploy_hash)
            }
            Event::DeploysExpired(deploy_hashes) => {
                write!(
                    formatter,
                    "deploys expired: {}",
                    deploy_hashes.iter().join(", ")
                )
            }
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
