use thiserror::Error;

use casper_types::EraId;

use crate::types::BlockHeader;

#[derive(Error, Debug)]
pub enum CreateNewEraError {
    #[error("Attempted to create era with no switch blocks.")]
    AttemptedToCreateEraWithNoSwitchBlocks,
    #[error("Attempted to create {era_id} with non-switch block {last_block_header:?}.")]
    LastBlockHeaderNotASwitchBlock {
        era_id: EraId,
        last_block_header: Box<BlockHeader>,
    },
    #[error("Attempted to create {era_id} with too few switch blocks {switch_blocks:?}.")]
    InsufficientSwitchBlocks {
        era_id: EraId,
        switch_blocks: Vec<BlockHeader>,
    },
    #[error(
        "Attempted to create {era_id} with switch blocks from unexpected eras: {switch_blocks:?}."
    )]
    WrongSwitchBlockEra {
        era_id: EraId,
        switch_blocks: Vec<BlockHeader>,
    },
}
