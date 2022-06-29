// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use std::fmt::{self, Display, Formatter};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::storage::disjoint_sequences::Sequence;

/// An unbroken, inclusive range of blocks.
#[derive(
    Copy, Clone, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize, Debug, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct AvailableBlockRange {
    /// The inclusive lower bound of the range.
    low: u64,
    /// The inclusive upper bound of the range.
    high: u64,
}

impl From<Sequence> for AvailableBlockRange {
    fn from(sequence: Sequence) -> Self {
        AvailableBlockRange {
            low: sequence.low(),
            high: sequence.high(),
        }
    }
}

impl AvailableBlockRange {
    /// An `AvailableRange` of [0, 0].
    pub const RANGE_0_0: AvailableBlockRange = AvailableBlockRange { low: 0, high: 0 };

    /// Constructs a new `AvailableBlockRange` with the given limits.
    #[cfg(test)]
    pub fn new(low: u64, high: u64) -> Self {
        assert!(
            low <= high,
            "cannot construct available block range with low > high"
        );
        AvailableBlockRange { low, high }
    }

    /// Returns `true` if `height` is within the range.
    pub fn contains(&self, height: u64) -> bool {
        height >= self.low && height <= self.high
    }

    /// Returns the low value.
    pub fn low(&self) -> u64 {
        self.low
    }

    /// Returns the high value.
    pub fn high(&self) -> u64 {
        self.high
    }
}

impl Display for AvailableBlockRange {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "available block range [{}, {}]",
            self.low, self.high
        )
    }
}
