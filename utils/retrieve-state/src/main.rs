use std::env;

use reqwest::ClientBuilder;
use structopt::StructOpt;

use casper_node::{
    rpcs::chain::{BlockIdentifier, GetBlockParams},
    types::JsonBlock,
};

#[derive(Debug, StructOpt)]
struct Opts {
    #[structopt(short = "n", default_value = "http://localhost:11101")]
    server_host: String,

    #[structopt(short = "h", long = "--download-height")]
    download_height: Option<u64>,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let opts = Opts::from_args();
    // TODO: enable gzip if the rpc endpoint supports it.
    let mut client = ClientBuilder::new().gzip(false).build().unwrap();
    let lmdb_path = env::current_dir()?.join(retrieve_state::LMDB_PATH);
    let chain_download_path = env::current_dir()?.join(retrieve_state::CHAIN_DOWNLOAD_PATH);
    let url = format!("{}/rpc", opts.server_host);

    let engine_state = retrieve_state::offline::create_execution_engine(lmdb_path)?;

    let download_block = opts.download_height.map(|height| GetBlockParams {
        block_identifier: BlockIdentifier::Height(height),
    });

    println!("Downloading highest block...");
    let highest_block: JsonBlock = retrieve_state::get_block(&mut client, &url, download_block)
        .await?
        .block
        .unwrap();

    let highest_block_height = highest_block.clone();
    let url_clone = url.clone();
    let state_download_task = tokio::spawn(async move {
        // TODO: enable gzip if the rpc endpoint supports it.
        let mut client = ClientBuilder::new().gzip(false).build().unwrap();
        println!(
            "Retrieving global state at height {}...",
            highest_block_height.header.height
        );
        retrieve_state::download_trie(
            &mut client,
            &url_clone,
            &engine_state,
            highest_block_height.header.state_root_hash,
        )
        .await
        .unwrap();
        println!("finished downloading global state");
    });

    let download_block = {
        let block_files =
            retrieve_state::offline::get_block_files(retrieve_state::CHAIN_DOWNLOAD_PATH);
        let lowest_block_file = block_files.get(0);
        match lowest_block_file {
            Some(lowest_block_file) => {
                println!("found lowest block downloaded at {:?}", lowest_block_file);
                retrieve_state::offline::read_block_file(lowest_block_file)
                    .await?
                    .block
            }
            _ => highest_block,
        }
    };
    println!(
        "Downloading all blocks from {} to {}...",
        download_block.header.height, 0
    );
    let _block_files = retrieve_state::download_blocks(
        &mut client,
        &url,
        &chain_download_path,
        download_block.hash,
        0,
    )
    .await?;

    state_download_task.await?;
    Ok(())
}
