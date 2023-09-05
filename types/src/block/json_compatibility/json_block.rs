#[cfg(feature = "datasize")]
use datasize::DataSize;
use once_cell::sync::Lazy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    super::{Block, BlockHash, BlockSignatures, EraEnd, EraId},
    JsonBlockBody, JsonBlockHeader, JsonProof,
};
use crate::{
    crypto, BlockBody, BlockV2, DeployHash, Digest, ProtocolVersion, PublicKey, SecretKey,
    Timestamp,
};

static JSON_BLOCK: Lazy<JsonBlock> = Lazy::new(|| {
    let parent_hash = BlockHash::new(Digest::from([7; Digest::LENGTH]));
    let parent_seed = Digest::from([9; Digest::LENGTH]);
    let state_root_hash = Digest::from([8; Digest::LENGTH]);
    let random_bit = true;
    let era_end = Some(EraEnd::example().clone());
    let timestamp = *Timestamp::example();
    let era_id = EraId::from(1);
    let height = 10;
    let protocol_version = ProtocolVersion::V1_0_0;
    let secret_key = SecretKey::example();
    let proposer = PublicKey::from(secret_key);
    let deploy_hashes = vec![DeployHash::new(Digest::from([20; Digest::LENGTH]))];
    let transfer_hashes = vec![DeployHash::new(Digest::from([21; Digest::LENGTH]))];
    let block = BlockV2::new(
        parent_hash,
        parent_seed,
        state_root_hash,
        random_bit,
        era_end,
        timestamp,
        era_id,
        height,
        protocol_version,
        proposer,
        deploy_hashes,
        transfer_hashes,
    );
    let hash = *block.hash();
    let header = JsonBlockHeader::from(block.header);
    let body = JsonBlockBody::from(BlockBody::from(&block.body));
    let secret_key = SecretKey::example();
    let public_key = PublicKey::from(secret_key);
    let signature = crypto::sign(block.hash.inner(), secret_key, &public_key);
    let proofs = vec![JsonProof {
        public_key,
        signature,
    }];
    JsonBlock {
        hash,
        header,
        body,
        proofs,
    }
});

/// A JSON-friendly representation of [`Block`] along with a collection of validator signatures.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[schemars(
    description = "A block after execution, with the resulting global state root hash. This is \
    the core component of the Casper linear blockchain"
)]
#[serde(deny_unknown_fields)]
pub struct JsonBlock {
    /// The block hash identifying this block.
    pub hash: BlockHash,
    /// The header portion of the block.
    pub header: JsonBlockHeader,
    /// The body portion of the block.
    pub body: JsonBlockBody,
    /// The proofs of the block, i.e. a collection of validators' signatures of the block hash.
    pub proofs: Vec<JsonProof>,
}

impl JsonBlock {
    /// Constructs a new `JsonBlock`.
    pub fn new(block: Block, maybe_signatures: Option<BlockSignatures>) -> Self {
        let hash = *block.hash();
        let header = JsonBlockHeader::from(block.header().clone());
        let body = JsonBlockBody::from(block.body());
        let proofs = maybe_signatures
            .map(|signatures| signatures.proofs.into_iter().map(JsonProof::from).collect())
            .unwrap_or_default();

        JsonBlock {
            hash,
            header,
            body,
            proofs,
        }
    }

    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    pub fn example() -> &'static Self {
        &JSON_BLOCK
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::TestRng;

    use super::*;

    #[test]
    fn block_to_and_from_json_block() {
        let rng = &mut TestRng::new();
        let block = BlockV2::random(rng);
        let empty_signatures = BlockSignatures::new(*block.hash(), block.era_id());
        let json_block = JsonBlock::new(block.clone().into(), Some(empty_signatures));
        let recovered_block = BlockV2::from(json_block);
        assert_eq!(block, recovered_block);
    }

    #[test]
    fn json_block_roundtrip() {
        let rng = &mut TestRng::new();
        let block = BlockV2::random(rng);
        let json_string = serde_json::to_string_pretty(&block).unwrap();
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(block, decoded);
    }
}
