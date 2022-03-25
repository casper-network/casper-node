// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use std::fmt::{self, Display, Formatter};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::error;

/// An unbroken, inclusive range of blocks.
#[derive(
    Copy, Clone, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize, Debug, Error,
)]
#[error("invalid available block range [low: {}, high: {}]", .low, .high)]
pub struct AvailableBlockRangeError {
    low: u64,
    high: u64,
}

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

impl AvailableBlockRange {
    /// Returns a new `AvailableBlockRange`.
    pub fn new(low: u64, high: u64) -> Result<Self, AvailableBlockRangeError> {
        if low > high {
            let error = AvailableBlockRangeError { low, high };
            error!("{}", error);
            return Err(error);
        }
        Ok(AvailableBlockRange { low, high })
    }

    /// Returns `true` if `height` is within the range.
    pub fn contains(&self, height: u64) -> bool {
        height >= self.low && height <= self.high
    }
}

impl Default for AvailableBlockRange {
    fn default() -> Self {
        AvailableBlockRange {
            low: u64::MAX,
            high: u64::MAX,
        }
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
