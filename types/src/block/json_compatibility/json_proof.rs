#[cfg(feature = "datasize")]
use datasize::DataSize;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use crate::BlockSignatures;
use crate::{PublicKey, Signature};

/// A pair of public key and signature, used to convert the `proofs` field of [`BlockSignatures`]
/// from a `BTreeMap<PublicKey, Signature>` into a `Vec<JsonProof>` to support
/// JSON-encoding/decoding.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[schemars(
    description = "A validator's public key paired with a corresponding signature of a given \
    block hash."
)]
#[serde(deny_unknown_fields)]
pub struct JsonProof {
    /// The validator's public key.
    pub public_key: PublicKey,
    /// The validator's signature.
    pub signature: Signature,
}

impl From<(PublicKey, Signature)> for JsonProof {
    fn from((public_key, signature): (PublicKey, Signature)) -> JsonProof {
        JsonProof {
            public_key,
            signature,
        }
    }
}

impl From<JsonProof> for (PublicKey, Signature) {
    fn from(proof: JsonProof) -> (PublicKey, Signature) {
        (proof.public_key, proof.signature)
    }
}
