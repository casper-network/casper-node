use std::time::Instant;

use casper_execution_engine::{
    self,
    core::engine_state::EngineState,
    storage::{global_state::lmdb::LmdbGlobalState, transaction_source::lmdb::LmdbEnvironment},
};
use casper_node::{
    contract_runtime::{execute_finalized_block, BlockAndExecutionEffects, ExecutionPreState},
    types::{Block, Deploy, FinalizedBlock, JsonBlock},
};
use std::sync::Arc;

pub fn execute_json_block(
    environment: Arc<LmdbEnvironment>,
    engine_state: &EngineState<LmdbGlobalState>,
    json_block: JsonBlock,
    execution_pre_state: ExecutionPreState,
    deploys: Vec<Deploy>,
    transfers: Vec<Deploy>,
) -> Result<BlockAndExecutionEffects, anyhow::Error> {
    let block: Block = json_block.into();
    let protocol_version = block.protocol_version();
    let finalized_block = FinalizedBlock::from(block.clone());

    let start = Instant::now();
    let block_and_execution_effects = execute_finalized_block(
        engine_state,
        None,
        protocol_version,
        execution_pre_state,
        finalized_block,
        deploys,
        transfers,
    )?;
    println!(
        "{}, {}, {}, {}",
        block.height(),
        block.transfer_hashes().len(),
        block.deploy_hashes().len(),
        (Instant::now() - start).as_millis()
    );

    environment.env().sync(true)?;

    Ok(block_and_execution_effects)
}
