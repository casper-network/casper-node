use std::fmt::{self, Display, Formatter};

use casper_types::EraId;

/// A specification for a stopping point.
#[derive(Copy, Clone, Debug)]
pub(crate) enum StopAtSpec {
    /// Stop after completion of the current block.
    NextBlock,
    /// Stop after the completion of the next switch block.
    NextEra,
    /// Stop immediately.
    Immediately,
    /// Stop at a given block height.
    BlockHeight(u64),
    /// Stop at a given era id.
    EraId(EraId),
}

impl Default for StopAtSpec {
    fn default() -> Self {
        StopAtSpec::NextBlock
    }
}

impl Display for StopAtSpec {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            StopAtSpec::NextBlock => f.write_str("block:next"),
            StopAtSpec::NextEra => f.write_str("era:next"),
            StopAtSpec::Immediately => f.write_str("now"),
            StopAtSpec::BlockHeight(height) => write!(f, "block:{}", height),
            StopAtSpec::EraId(era_id) => write!(f, "era:{}", era_id.value()),
        }
    }
}
