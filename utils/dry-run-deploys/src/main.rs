use std::env;

use casper_node::contract_runtime::ExecutionPreState;

use retrieve_state::{offline, BlockWithDeploys};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let chain_path = env::current_dir()?
        .join("../retrieve-state")
        .join(retrieve_state::CHAIN_DOWNLOAD_PATH);

    let lmdb_path = env::current_dir()?
        .join("../retrieve-state")
        .join(retrieve_state::LMDB_PATH);

    let block_files = offline::get_block_files(chain_path);
    let (genesis, block_files) = block_files.split_at(1);

    let genesis_block = offline::read_block_file(&genesis[0]).await?;

    let (engine_state, env) = offline::load_execution_engine(
        lmdb_path,
        genesis_block.block.header.state_root_hash.into(),
    )?;

    let mut execution_pre_state = ExecutionPreState::from(&genesis_block.block.header.into());

    println!("block, transfer_count, deploy_count, execution_time_ms");
    for block_file_entry in block_files.iter() {
        let BlockWithDeploys {
            block,
            transfers,
            deploys,
        } = offline::read_block_file(block_file_entry).await?;

        let block_and_execution_effects = dry_run_deploys::execute_json_block(
            Arc::clone(&env),
            &engine_state,
            block,
            execution_pre_state,
            deploys,
            transfers,
        )?;
        let header = block_and_execution_effects.block.take_header();
        execution_pre_state = ExecutionPreState::from(&header);
    }
    Ok(())
}
