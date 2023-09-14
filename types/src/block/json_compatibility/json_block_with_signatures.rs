use alloc::collections::BTreeMap;
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use once_cell::sync::Lazy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_map_to_array::{BTreeMapToArray, KeyValueJsonSchema, KeyValueLabels};

use crate::{crypto, Block, BlockSignatures, BlockV2, PublicKey, SecretKey, Signature};

#[cfg(feature = "json-schema")]
static JSON_SIGNED_BLOCK: Lazy<JsonBlockWithSignatures> = Lazy::new(|| {
    let block = BlockV2::example().clone();
    let secret_key = SecretKey::example();
    let public_key = PublicKey::from(secret_key);
    let signature = crypto::sign(block.hash.inner(), secret_key, &public_key);
    let mut proofs = BTreeMap::new();
    proofs.insert(public_key, signature);

    JsonBlockWithSignatures {
        block: block.into(),
        proofs,
    }
});

/// A JSON-friendly representation of a block and the signatures for that block.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[serde(deny_unknown_fields)]
pub struct JsonBlockWithSignatures {
    /// The block.
    pub block: Block,
    /// The proofs of the block, i.e. a collection of validators' signatures of the block hash.
    #[serde(with = "BTreeMapToArray::<PublicKey, Signature, BlockProofLabels>")]
    pub proofs: BTreeMap<PublicKey, Signature>,
}

impl JsonBlockWithSignatures {
    /// Constructs a new `JsonBlock`.
    pub fn new(block: Block, maybe_signatures: Option<BlockSignatures>) -> Self {
        let proofs = maybe_signatures
            .map(|signatures| signatures.proofs)
            .unwrap_or_default();

        JsonBlockWithSignatures { block, proofs }
    }

    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    pub fn example() -> &'static Self {
        &JSON_SIGNED_BLOCK
    }
}
struct BlockProofLabels;

impl KeyValueLabels for BlockProofLabels {
    const KEY: &'static str = "public_key";
    const VALUE: &'static str = "signature";
}

impl KeyValueJsonSchema for BlockProofLabels {
    const JSON_SCHEMA_KV_NAME: Option<&'static str> = Some("BlockProof");
    const JSON_SCHEMA_KV_DESCRIPTION: Option<&'static str> = Some(
        "A validator's public key paired with a corresponding signature of a given block hash.",
    );
    const JSON_SCHEMA_KEY_DESCRIPTION: Option<&'static str> = Some("The validator's public key.");
    const JSON_SCHEMA_VALUE_DESCRIPTION: Option<&'static str> = Some("The validator's signature.");
}

#[cfg(test)]
mod tests {
    use crate::{testing::TestRng, TestBlockBuilder};

    use super::*;

    #[test]
    fn block_to_and_from_json_block_with_signatures() {
        let rng = &mut TestRng::new();
        let block: Block = TestBlockBuilder::new().build(rng).into();
        let empty_signatures = BlockSignatures::new(*block.hash(), block.era_id());
        let json_block = JsonBlockWithSignatures::new(block.clone(), Some(empty_signatures));
        let recovered_block = Block::from(json_block);
        assert_eq!(block, recovered_block);
    }

    #[test]
    fn json_block_roundtrip() {
        let rng = &mut TestRng::new();
        let block: Block = TestBlockBuilder::new().build(rng).into();
        let json_string = serde_json::to_string_pretty(&block).unwrap();
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(block, decoded);
    }
}
