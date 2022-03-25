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
#[error("invalid contiguous block range [low: {}, high: {}]", .low, .high)]
pub struct ContiguousBlockRangeError {
    low: u64,
    high: u64,
}

/// An unbroken, inclusive range of blocks.
#[derive(
    Copy, Clone, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize, Debug, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ContiguousBlockRange {
    /// The inclusive lower bound of the range.
    low: u64,
    /// The inclusive upper bound of the range.
    high: u64,
}

impl ContiguousBlockRange {
    /// Returns a new `ContiguousBlockRange`.
    pub fn new(low: u64, high: u64) -> Result<Self, ContiguousBlockRangeError> {
        if low > high {
            let error = ContiguousBlockRangeError { low, high };
            error!("{}", error);
            return Err(error);
        }
        Ok(ContiguousBlockRange { low, high })
    }
}

impl Default for ContiguousBlockRange {
    fn default() -> Self {
        ContiguousBlockRange {
            low: u64::MAX,
            high: u64::MAX,
        }
    }
}

impl Display for ContiguousBlockRange {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "contiguous block range [{}, {}]",
            self.low, self.high
        )
    }
}
