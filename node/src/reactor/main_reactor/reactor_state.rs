use datasize::DataSize;
use derive_more::Display;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The state of the reactor.
#[cfg_attr(doc, aquamarine::aquamarine)]
/// ```mermaid
/// flowchart TD
///     style Start fill:#66ccff,stroke:#333,stroke-width:4px
///     style End fill:#66ccff,stroke:#333,stroke-width:4px
///     
///     Start --> Initialize
///     Initialize --> CatchUp
///     CatchUp --> KeepUp
///     KeepUp --> CatchUp
///     KeepUp --> Validate
///     Validate --> KeepUp
///     CatchUp --> ShutdownForUpgrade
///     KeepUp --> ShutdownForUpgrade
///     Validate --> ShutdownForUpgrade
///     CatchUp --> Upgrading
///     CatchUp -->|at genesis| Validate
///     Upgrading --> CatchUp
///     ShutdownForUpgrade --> End
/// ```
/// ```mermaid
/// flowchart TD
///     style Start fill:#66ccff,stroke:#333,stroke-width:4px
///     style End fill:#66ccff,stroke:#333,stroke-width:4px
///     style F fill:#ffcc66,stroke:#333,stroke-width:4px
///     style G fill:#ffcc66,stroke:#333,stroke-width:4px
///     title[CatchUP process]
///     title---Start
///     style title fill:#FFF,stroke:#FFF
///     linkStyle 0 stroke-width:0;
///
///     Start --> A["get sync identifier (sync starting point)"]
///     A --> BlockHash
///     A --> BlockIdentifier
///     A --> SyncedBlockIdentifier
///     A --> LocalTip
///     BlockHash --> E[process identifier in<br/>block accumulator]
///     BlockIdentifier --> E
///     SyncedBlockIdentifier --> E
///     LocalTip --> E
///     CaughtUp --> H[handle upgrade<br/>if needed]
///     H --> End
///     E -->|more data needed<br/>from network| Leap
///     E -->|block represneted by<br/>identifier is not stored<br/>locally, sync it| BlockSync
///     E -->|we think we're close<br/>enough to the tip|CaughtUp
///     Leap --> F[initiate SyncLeap<br/>and retry later]
///     BlockSync --> G[initiate BlockSync<br/>and retry later]
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
