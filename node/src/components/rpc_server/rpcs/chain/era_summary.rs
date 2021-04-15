use once_cell::sync::Lazy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use casper_types::system::auction::EraInfo;

use crate::{
    crypto::hash::Digest,
    rpcs::{common::MERKLE_PROOF, docs::DocExample},
    types::{json_compatibility::StoredValue, Block, BlockHash, Item},
};

pub(super) static ERA_SUMMARY: Lazy<EraSummary> = Lazy::new(|| EraSummary {
    block_hash: Block::doc_example().id(),
    era_id: 42,
    stored_value: StoredValue::EraInfo(EraInfo::new()),
    state_root_hash: *Block::doc_example().header().state_root_hash(),
    merkle_proof: MERKLE_PROOF.clone(),
});

/// The summary of an era
#[derive(Clone, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EraSummary {
    /// The block hash
    pub block_hash: BlockHash,
    /// The era id
    pub era_id: u64,
    /// The StoredValue containing era information
    pub stored_value: StoredValue,
    /// Hex-encoded hash of the state root
    pub state_root_hash: Digest,
    /// The merkle proof
    pub merkle_proof: String,
}
