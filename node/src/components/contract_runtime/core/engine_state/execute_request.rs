use std::mem;

use casper_types::ProtocolVersion;

use super::{deploy_item::DeployItem, execution_result::ExecutionResult};
use crate::crypto::hash::{self, Digest};

#[derive(Debug)]
pub struct ExecuteRequest {
    pub parent_state_hash: Digest,
    pub block_time: u64,
    pub deploys: Vec<Result<DeployItem, ExecutionResult>>,
    pub protocol_version: ProtocolVersion,
}

impl ExecuteRequest {
    pub fn new(
        parent_state_hash: Digest,
        block_time: u64,
        deploys: Vec<Result<DeployItem, ExecutionResult>>,
        protocol_version: ProtocolVersion,
    ) -> Self {
        Self {
            parent_state_hash,
            block_time,
            deploys,
            protocol_version,
        }
    }

    pub fn take_deploys(&mut self) -> Vec<Result<DeployItem, ExecutionResult>> {
        mem::replace(&mut self.deploys, vec![])
    }
}

impl Default for ExecuteRequest {
    fn default() -> Self {
        Self {
            parent_state_hash: hash::hash(&[]),
            block_time: 0,
            deploys: vec![],
            protocol_version: Default::default(),
        }
    }
}
