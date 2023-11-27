use crate::bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};
use datasize::DataSize;
use derive_more::Display;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The state of the reactor.
#[cfg_attr(doc, aquamarine::aquamarine)]
/// ```mermaid
/// flowchart TD
///     %%{init: { 'flowchart': {'diagramPadding':100} }}%%
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
///     title[CatchUp process]
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
///     E -->|more data needed<br/>from network<br/>to let us sync near tip| Leap
///     E -->|block represented by<br/>identifier is not stored<br/>locally, sync it| BlockSync
///     E -->|we think we're close<br/>enough to the tip|CaughtUp
///     Leap --> F[initiate SyncLeap<br/>and retry later]
///     BlockSync --> G[initiate BlockSync<br/>and retry later]
/// ```
#[derive(
    Copy, Clone, PartialEq, Eq, Serialize, Deserialize, DataSize, Debug, Display, JsonSchema,
)]
#[schemars(description = "The state of the reactor.")]
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

const INITIALIZE_TAG: u8 = 0;
const CATCHUP_TAG: u8 = 1;
const UPGRADING_TAG: u8 = 2;
const KEEPUP_TAG: u8 = 3;
const VALIDATE_TAG: u8 = 4;
const SHUTDOWN_FOR_UPGRADE_TAG: u8 = 5;

impl ToBytes for ReactorState {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            ReactorState::Initialize => INITIALIZE_TAG,
            ReactorState::CatchUp => CATCHUP_TAG,
            ReactorState::Upgrading => UPGRADING_TAG,
            ReactorState::KeepUp => KEEPUP_TAG,
            ReactorState::Validate => VALIDATE_TAG,
            ReactorState::ShutdownForUpgrade => SHUTDOWN_FOR_UPGRADE_TAG,
        }
        .write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }
}

impl FromBytes for ReactorState {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        let reactor_state = match tag {
            INITIALIZE_TAG => ReactorState::Initialize,
            CATCHUP_TAG => ReactorState::CatchUp,
            UPGRADING_TAG => ReactorState::Upgrading,
            KEEPUP_TAG => ReactorState::KeepUp,
            VALIDATE_TAG => ReactorState::Validate,
            SHUTDOWN_FOR_UPGRADE_TAG => ReactorState::ShutdownForUpgrade,
            _ => return Err(bytesrepr::Error::NotRepresentable),
        };
        Ok((reactor_state, remainder))
    }
}
