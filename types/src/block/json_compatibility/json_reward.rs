#[cfg(feature = "datasize")]
use datasize::DataSize;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use crate::EraReport;
use crate::PublicKey;

/// A pair of public key and amount, used to convert the `rewards` field of [`EraReport`] from a
/// `BTreeMap<PublicKey, u64>` into a `Vec<JsonReward>` to support JSON-encoding/decoding.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[schemars(
    description = "A validator's public key paired with a measure of the value of its \
    contribution to consensus, as a fraction of the configured maximum block reward."
)]
#[serde(deny_unknown_fields)]
pub struct JsonReward {
    /// The validator's public key.
    pub validator: PublicKey,
    /// The reward amount.
    pub amount: u64,
}
