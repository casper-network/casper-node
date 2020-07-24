use thiserror::Error;

use super::config::MAX_SATURATION_LIMIT_PERCENT;

/// Error returned by a `GossipTable`.
#[derive(Debug, Error)]
pub enum Error {
    /// Invalid configuration value for `saturation_limit_percent`.
    #[error(
        "invalid saturation_limit_percent - should be between 0 and {} inclusive",
        MAX_SATURATION_LIMIT_PERCENT
    )]
    InvalidSaturationLimit,

    /// Attempted to reset data which had not been paused.
    #[error("gossiping is not paused for this data")]
    NotPaused,
}
