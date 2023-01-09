use datasize::DataSize;
use derive_more::Display;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The state of the reactor.
#[cfg_attr(doc, aquamarine::aquamarine)]
/// ```mermaid
/// flowchart TD
///     Initialize --> CatchUp
///     CatchUp --> KeepUp
///     KeepUp --> CatchUp
///     KeepUp --> Validate
///     Validate --> KeepUp
///     CatchUp --> ShutdownForUpgrade
///     KeepUp --> ShutdownForUpgrade
///     Validate --> ShutdownForUpgrade
///     CatchUp --> Upgrading
///     CatchUp --> Validate
///     Upgrading --> CatchUp
/// ```
#[derive(
    Copy, Clone, PartialEq, Eq, Serialize, Deserialize, DataSize, Debug, Display, JsonSchema,
)]
pub enum ReactorState {
    /// Get all components and reactor state set up on start.
    Initialize,
    /// Orient to the network and attempt to catch up to tip.
    CatchUp,
    /// Running commit upgrade and creating immediate switch block.
    Upgrading,
    /// Stay caught up with tip.
    KeepUp,
    /// Node is currently caught up and is an active validator.
    Validate,
    /// Node should be shut down for upgrade.
    ShutdownForUpgrade,
}
