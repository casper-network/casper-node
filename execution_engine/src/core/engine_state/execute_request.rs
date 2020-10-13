use std::mem;

use casper_types::ProtocolVersion;

use super::{deploy_item::DeployItem, execution_result::ExecutionResult};
use crate::shared::newtypes::Blake2bHash;

#[derive(Debug)]
pub struct ExecuteRequest {
    pub state_root_hash: Blake2bHash,
    pub block_time: u64,
    pub deploys: Vec<Result<DeployItem, ExecutionResult>>,
    pub protocol_version: ProtocolVersion,
}

impl ExecuteRequest {
    pub fn new(
        state_root_hash: Blake2bHash,
        block_time: u64,
        deploys: Vec<Result<DeployItem, ExecutionResult>>,
        protocol_version: ProtocolVersion,
    ) -> Self {
        Self {
            state_root_hash,
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
            state_root_hash: Blake2bHash::new(&[]),
            block_time: 0,
            deploys: vec![],
            protocol_version: Default::default(),
        }
    }
}
