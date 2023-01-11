use casper_types::EraId;

use crate::types::BlockHash;

#[cfg_attr(doc, aquamarine::aquamarine)]
/// ```mermaid
/// flowchart TD
///     style LocalTip fill:#99ff99,stroke:#333,stroke-width:4px
///     style SyncedBlockIdentifier fill:#99ff99,stroke:#333,stroke-width:4px
///     style BlockHash fill:#99ff99,stroke:#333,stroke-width:4px
///     style BlockIdentifier fill:#99ff99,stroke:#333,stroke-width:4px
///     style J fill:#ffcc66,stroke:#333,stroke-width:4px
///     style Start fill:#66ccff,stroke:#333,stroke-width:4px
///     style End fill:#66ccff,stroke:#333,stroke-width:4px
/// 
///     title[Obtaining initial state hash for syncing]
///     title---Start
///     style title fill:#FFF,stroke:#FFF
///     linkStyle 0 stroke-width:0;
/// 
///     Start --> A['historical' sync state]
///     A --> Syncing
///     A --> Synced
///     A --> Idle
///     Synced --> SyncedBlockIdentifier
///     Idle --> C{trusted hash in config?}
///     C -->|Yes| D{is trusted block<br>header in storage?}
///     C -->|No| G{is highest complete<br>block available in storage?}
///     D -->|Yes| E{is highest complete<br>block available in storage?}
///     E -->|No| BlockHash
///     D -->|No| BlockHash
///     E -->|Yes| F{is trusted header height greater<br>than highest block height?}
///     F -->|Yes| BlockIdentifier
///     F -->|No| LocalTip
///     G -->|Yes| H{is it a switch block?}
///     G -->|No| J[will check later<br>possibly waiting for genesis]
///     H -->|Yes| I[remember switch block header]
///     I --> LocalTip
///     H -->|No| LocalTip
///     Syncing --> K{is height of block<br>being synced available?}
///     K -->|Yes| BlockIdentifier
///     K -->|No| BlockHash
///     SyncedBlockIdentifier --> End    
///     LocalTip --> End
///     BlockIdentifier --> End
///     BlockHash --> End
/// ```
#[derive(Clone, Debug)]
pub(crate) enum SyncIdentifier {
    // all we know about the block is its hash;
    // this is usually a trusted hash from config
    BlockHash(BlockHash),
    // we know both the hash and the height of the block
    BlockIdentifier(BlockHash, u64),
    // we have just acquired the necessary data for the block
    // including sufficient finality; this may be a historical
    // block and / or potentially the new highest block
    SyncedBlockIdentifier(BlockHash, u64, EraId),
    // we read this block from disk, and have all the parts
    // we need to discover its descendent (if any) to continue.
    LocalTip(BlockHash, u64, EraId),
}

impl SyncIdentifier {
    pub(crate) fn block_hash(&self) -> BlockHash {
        match self {
            SyncIdentifier::BlockIdentifier(hash, _)
            | SyncIdentifier::SyncedBlockIdentifier(hash, _, _)
            | SyncIdentifier::LocalTip(hash, _, _)
            | SyncIdentifier::BlockHash(hash) => *hash,
        }
    }

    pub(crate) fn block_height(&self) -> Option<u64> {
        match self {
            SyncIdentifier::BlockIdentifier(_, height)
            | SyncIdentifier::SyncedBlockIdentifier(_, height, _)
            | SyncIdentifier::LocalTip(_, height, _) => Some(*height),
            SyncIdentifier::BlockHash(_) => None,
        }
    }

    pub(crate) fn is_held_locally(&self) -> bool {
        match self {
            SyncIdentifier::BlockHash(_) | SyncIdentifier::BlockIdentifier(_, _) => false,
            SyncIdentifier::SyncedBlockIdentifier(_, _, _) | SyncIdentifier::LocalTip(_, _, _) => {
                true
            }
        }
    }

    pub(crate) fn maybe_local_tip_identifier(&self) -> Option<(u64, EraId)> {
        match self {
            SyncIdentifier::BlockHash(_) | SyncIdentifier::BlockIdentifier(_, _) => None,
            SyncIdentifier::SyncedBlockIdentifier(_, block_height, era_id)
            | SyncIdentifier::LocalTip(_, block_height, era_id) => Some((*block_height, *era_id)),
        }
    }
}
