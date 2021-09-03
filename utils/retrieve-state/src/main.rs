use std::env;

use lmdb::EnvironmentFlags;
use reqwest::Client;
use structopt::StructOpt;

use casper_node::types::JsonBlock;

#[derive(Debug, StructOpt)]
struct Opts {}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let mut client = Client::new();
    let lmdb_path = env::current_dir()?.join(retrieve_state::LMDB_PATH);
    let chain_download_path = env::current_dir()?.join(retrieve_state::CHAIN_DOWNLOAD_PATH);

    // if lmdb_path.as_ref().join("data.lmdb").exists() {
    //     return Err(anyhow::anyhow!(
    //         "lmdb data file already exists at {}",
    //         lmdb_path.as_ref().display()
    //     ));
    // } else {
    // }
    let engine_state =
        retrieve_state::offline::create_execution_engine(lmdb_path, EnvironmentFlags::MAP_ASYNC)?;

    println!("Downloading highest block...");
    let highest_block: JsonBlock = retrieve_state::get_block(&mut client, None)
        .await?
        .block
        .unwrap();

    println!(
        "Downloading all blocks to height {}...",
        highest_block.header.height
    );
    let block_files =
        retrieve_state::download_blocks(&mut client, &chain_download_path, highest_block.hash, 0)
            .await?;

    let genesis_block = block_files.get(0).expect("should have genesis block");
    let genesis_block = retrieve_state::offline::read_block_file(genesis_block).await?;

    println!("Retrieving global state at genesis...");
    retrieve_state::download_genesis_global_state(&mut client, &engine_state, &genesis_block.block)
        .await?;

    Ok(())
}
