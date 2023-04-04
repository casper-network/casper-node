//! Code supporting an execution request.
use std::mem;

use casper_hashing::Digest;
use casper_types::{ProtocolVersion, PublicKey, SecretKey};

use super::deploy_item::DeployItem;

/// Represents an execution request that can contain multiple deploys.
#[derive(Debug)]
pub struct ExecuteRequest {
    /// State root hash of the global state in which the deploys will be executed.
    pub parent_state_hash: Digest,
    /// Block time represented as a unix timestamp.
    pub block_time: u64,
    /// List of deploys that will be executed as part of this request.
    pub deploys: Vec<DeployItem>,
    /// Protocol version used to execute deploys from the list.
    pub protocol_version: ProtocolVersion,
    /// The owner of the node that proposed the block containing this request.
    pub proposer: PublicKey,
}

impl ExecuteRequest {
    /// Creates new execute request.
    pub fn new(
        parent_state_hash: Digest,
        block_time: u64,
        deploys: Vec<DeployItem>,
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

    /// Returns deploys, and overwrites the existing value with empty list.
    pub fn take_deploys(&mut self) -> Vec<DeployItem> {
        mem::take(&mut self.deploys)
    }

    /// Returns list of deploys.
    pub fn deploys(&self) -> &Vec<DeployItem> {
        &self.deploys
    }
}

impl Default for ExecuteRequest {
    fn default() -> Self {
        let proposer_secret_key =
            SecretKey::ed25519_from_bytes([0; SecretKey::ED25519_LENGTH]).unwrap();
        let proposer = PublicKey::from(&proposer_secret_key);
        Self {
            parent_state_hash: Digest::hash([]),
            block_time: 0,
            deploys: vec![],
            protocol_version: Default::default(),
            proposer,
        }
    }
}
