use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A change to a validator's status between two eras.
#[derive(Serialize, Deserialize, Debug, JsonSchema, Eq, PartialEq, Ord, PartialOrd)]
pub enum ValidatorChange {
    /// The validator got newly added to the validator set.
    Added,
    /// The validator was removed from the validator set.
    Removed,
    /// The validator was banned from this era.
    Banned,
    /// The validator was excluded from proposing new blocks in this era.
    CannotPropose,
    /// We saw the validator misbehave in this era.
    SeenAsFaulty,
}
