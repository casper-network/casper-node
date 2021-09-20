use std::{env, path::PathBuf, time::Instant};

use structopt::StructOpt;

use casper_node::contract_runtime::{execute_finalized_block, ExecutionPreState};

use casper_node::types::FinalizedBlock;
use indicatif::ProgressBar;
use retrieve_state::{storage, storage::LocalStorage};
use std::path::Path;

#[derive(Debug, StructOpt)]
struct Opts {
    #[structopt(short, long, required = true, default_value = retrieve_state::CHAIN_DOWNLOAD_PATH)]
    chain_download_path: PathBuf,

    #[structopt(short, long, required = true, default_value = retrieve_state::LMDB_PATH)]
    lmdb_path: PathBuf,

    #[structopt(short, long, required = true, default_value = "0")]
    starting_block_height: u64,

    #[structopt(short, long)]
    ending_block_height: Option<u64>,

    #[structopt(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let opts = Opts::from_args();

    let chain_download_path = normalize_path(&opts.chain_download_path)?;
    let lmdb_path = normalize_path(&opts.lmdb_path)?;

    // Create a separate lmdb for block/deploy storage at chain_download_path.
    let lmdb_environment = storage::create_lmdb_environment(&chain_download_path, 10)?;
    let local_storage = LocalStorage::create(lmdb_environment)?;

    let starting_block = local_storage
        .get_block_by_height(opts.starting_block_height)?
        .unwrap();

    let starting_block_header = starting_block.take_header();
    let (engine_state, _env) = storage::load_execution_engine(
        lmdb_path,
        retrieve_state::DEFAULT_MAX_DB_SIZE,
        *starting_block_header.state_root_hash(),
    )?;

    let mut execution_pre_state = ExecutionPreState::from(&starting_block_header);
    let mut execute_count = 0;

    let highest_height_in_chain = local_storage.get_highest_height()?;
    let ending_block_height = opts.ending_block_height.unwrap_or(highest_height_in_chain);
    let progress = ProgressBar::new(ending_block_height - opts.starting_block_height);

    if opts.verbose {
        println!("height, block_hash, transfer_count, deploy_count, execution_time_ms");
    }
    for height in opts.starting_block_height + 1..=ending_block_height {
        let block = local_storage.get_block_by_height(height)?.ok_or_else(|| {
            anyhow::anyhow!(
                "Block does not exist in downloaded chain at height {}",
                height
            )
        })?;
        let block_hash = *block.hash();
        let transfers = local_storage.get_many_deploys_by_hash(block.transfer_hashes())?;
        let transfers_len = transfers.len();
        let deploys = local_storage.get_many_deploys_by_hash(block.deploy_hashes())?;
        let deploys_len = deploys.len();
        let start = Instant::now();
        let protocol_version = block.protocol_version();
        let finalized_block = FinalizedBlock::from(block.clone());
        let block_and_execution_effects = execute_finalized_block(
            &engine_state,
            None,
            protocol_version,
            execution_pre_state,
            finalized_block,
            deploys,
            transfers,
        )?;
        let header = block_and_execution_effects.block.take_header();
        execution_pre_state = ExecutionPreState::from(&header);
        execute_count += 1;
        if opts.verbose {
            println!(
                "{}, {}, {}, {}, {}",
                header.height(),
                block_hash,
                transfers_len,
                deploys_len,
                start.elapsed().as_millis()
            );
        }
        progress.inc(1);
    }
    println!("Executed {} blocks.", execute_count);
    Ok(())
}

fn normalize_path(path: impl AsRef<Path>) -> Result<PathBuf, anyhow::Error> {
    let path = path.as_ref();
    if path.is_absolute() {
        Ok(path.into())
    } else {
        Ok(env::current_dir()?.join(path))
    }
}
