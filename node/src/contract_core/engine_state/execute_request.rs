use std::mem;

use crate::contract_shared::newtypes::Blake2bHash;
use types::ProtocolVersion;

use super::{deploy_item::DeployItem, execution_result::ExecutionResult};

#[derive(Debug)]
pub struct ExecuteRequest {
    pub parent_state_hash: Blake2bHash,
    pub block_time: u64,
    pub deploys: Vec<Result<DeployItem, ExecutionResult>>,
    pub protocol_version: ProtocolVersion,
}

impl ExecuteRequest {
    pub fn new(
        parent_state_hash: Blake2bHash,
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
            parent_state_hash: [0u8; 32].into(),
            block_time: 0,
            deploys: vec![],
            protocol_version: Default::default(),
        }
    }
}
