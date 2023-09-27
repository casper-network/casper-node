use once_cell::sync::Lazy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use casper_types::{
    system::auction::{EraInfo, SeigniorageAllocation},
    AsymmetricType, BlockHash, BlockV2, Digest, EraId, PublicKey, StoredValue, U512,
};

use crate::rpcs::common::MERKLE_PROOF;

pub(super) static ERA_SUMMARY: Lazy<EraSummary> = Lazy::new(|| {
    let delegator_amount = U512::from(1000);
    let validator_amount = U512::from(2000);
    let delegator_public_key =
        PublicKey::from_hex("01e1b46a25baa8a5c28beb3c9cfb79b572effa04076f00befa57eb70b016153f18")
            .unwrap();
    let validator_public_key =
        PublicKey::from_hex("012a1732addc639ea43a89e25d3ad912e40232156dcaa4b9edfc709f43d2fb0876")
            .unwrap();
    let delegator = SeigniorageAllocation::delegator(
        delegator_public_key,
        validator_public_key,
        delegator_amount,
    );
    let validator = SeigniorageAllocation::validator(
        PublicKey::from_hex("012a1732addc639ea43a89e25d3ad912e40232156dcaa4b9edfc709f43d2fb0876")
            .unwrap(),
        validator_amount,
    );
    let seigniorage_allocations = vec![delegator, validator];
    let mut era_info = EraInfo::new();
    *era_info.seigniorage_allocations_mut() = seigniorage_allocations;
    EraSummary {
        block_hash: *BlockV2::example().hash(),
        era_id: EraId::from(42),
        stored_value: StoredValue::EraInfo(era_info),
        state_root_hash: *BlockV2::example().state_root_hash(),
        merkle_proof: MERKLE_PROOF.clone(),
    }
});

/// The summary of an era
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EraSummary {
    /// The block hash
    pub block_hash: BlockHash,
    /// The era id
    pub era_id: EraId,
    /// The StoredValue containing era information
    pub stored_value: StoredValue,
    /// Hex-encoded hash of the state root
    pub state_root_hash: Digest,
    /// The Merkle proof
    pub merkle_proof: String,
}
