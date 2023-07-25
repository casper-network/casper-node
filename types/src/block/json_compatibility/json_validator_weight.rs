#[cfg(feature = "datasize")]
use datasize::DataSize;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use crate::EraEnd;
use crate::{PublicKey, U512};

/// A pair of public key and weight, used to convert the `next_era_validator_weights` field of
/// [`EraEnd`] from a `BTreeMap<PublicKey, U512>` into a `Vec<JsonValidatorWeight>` to support
/// JSON-encoding/decoding.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[schemars(
    description = "A validator's public key paired with its weight, i.e. the total number of \
    motes staked by it and its delegators."
)]
#[serde(deny_unknown_fields)]
pub struct JsonValidatorWeight {
    /// The validator's public key.
    pub validator: PublicKey,
    /// The validator's weight.
    pub weight: U512,
}
