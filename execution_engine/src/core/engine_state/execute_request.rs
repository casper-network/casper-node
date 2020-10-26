use std::mem;

use casper_types::{ProtocolVersion, PublicKey, ED25519_PUBLIC_KEY_LENGTH};

use super::{deploy_item::DeployItem, execution_result::ExecutionResult};
use crate::shared::newtypes::Blake2bHash;

#[derive(Debug)]
pub struct ExecuteRequest {
    pub parent_state_hash: Blake2bHash,
    pub block_time: u64,
    pub deploys: Vec<Result<DeployItem, ExecutionResult>>,
    pub protocol_version: ProtocolVersion,
    pub proposer: PublicKey,
}

impl ExecuteRequest {
    pub fn new(
        parent_state_hash: Blake2bHash,
        block_time: u64,
        deploys: Vec<Result<DeployItem, ExecutionResult>>,
        protocol_version: ProtocolVersion,
        proposer: PublicKey,
    ) -> Self {
        Self {
            parent_state_hash,
            block_time,
            deploys,
            protocol_version,
            proposer,
        }
    }

    pub fn take_deploys(&mut self) -> Vec<Result<DeployItem, ExecutionResult>> {
        mem::replace(&mut self.deploys, vec![])
    }

    pub fn deploys(&self) -> &Vec<Result<DeployItem, ExecutionResult>> {
        &self.deploys
    }
}

impl Default for ExecuteRequest {
    fn default() -> Self {
        Self {
            parent_state_hash: Blake2bHash::new(&[]),
            block_time: 0,
            deploys: vec![],
            protocol_version: Default::default(),
            proposer: PublicKey::Ed25519([0; ED25519_PUBLIC_KEY_LENGTH]),
        }
    }
}
