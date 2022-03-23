use std::{path::PathBuf, time::Instant};

use casper_hashing::Digest;
use histogram::Histogram;
use indicatif::{ProgressBar, ProgressStyle};
use structopt::StructOpt;
use tracing::{error, info};

use casper_node::{
    contract_runtime::{execute_finalized_block, ExecutionPreState},
    types::FinalizedBlock,
};

use retrieve_state::{
    storage,
    storage::{create_storage, get_many_deploys_by_hash, normalize_path},
};

use casper_types::EraId;

#[derive(Debug, StructOpt)]
struct Opts {
    #[structopt(long, default_value = retrieve_state::CHAIN_DOWNLOAD_PATH)]
    chain_download_path: PathBuf,

    #[structopt(long, default_value = retrieve_state::LMDB_PATH)]
    lmdb_path: PathBuf,

    #[structopt(long, default_value = retrieve_state::ROCKSDB_PATH)]
    rocksdb_path: PathBuf,

    #[structopt(
        long,
        default_value = "1",
        about = "Starting block height for execution. Must be > 0."
    )]
    starting_block_height: u64,

    #[structopt(long)]
    ending_block_height: Option<u64>,

    #[structopt(short, long)]
    verbose: bool,

    #[structopt(short, long, about = "Enable manual syncing after each block to LMDB")]
    manual_sync_enabled: bool,

    #[structopt(
        long = "max-db-size",
        about = "Max LMDB database size, may be useful to set this when running under valgrind."
    )]
    max_db_size: Option<usize>,

    #[structopt(
        long = "run-rocksdb-migration",
        about = "Perform the rocksdb migration from lmdb to rockdb for all tries present in lmdb."
    )]
    run_rocksdb_migration: bool,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    env_logger::init();
    let opts = Opts::from_args();

    let chain_download_path = normalize_path(&opts.chain_download_path)?;
    let lmdb_path = normalize_path(&opts.lmdb_path)?;
    let rocksdb_path = normalize_path(&opts.rocksdb_path)?;

    // TODO: Consider reading the proper `chainspec` in the `dry-run-deploys` tool.
    let verifiable_chunked_hash_activation = EraId::from(0u64);

    // Create a separate lmdb for block/deploy storage at chain_download_path.
    let storage = create_storage(&chain_download_path, verifiable_chunked_hash_activation)
        .expect("should create storage");

    let max_db_size = opts
        .max_db_size
        .unwrap_or(retrieve_state::DEFAULT_MAX_DB_SIZE);

    if opts.run_rocksdb_migration {
        let (engine_state, _env) = storage::load_execution_engine(
            lmdb_path,
            rocksdb_path,
            max_db_size,
            Digest::from([0; 32]),
            opts.manual_sync_enabled,
            casper_execution_engine::rocksdb_defaults(),
        )?;
        info!("Running migration of data from lmdb to rocksdb...");

        let mut state_roots = Vec::new();
        let latest_block = storage.read_highest_block();
        if let Ok(Some(latest_block)) = latest_block {
            let latest_block_header = latest_block.take_header();
            for height in (0..=latest_block_header.height()).rev() {
                let block_header = match storage.read_block_by_height(height) {
                    Ok(Some(block)) => block.header().clone(),
                    Ok(None) => {
                        error!(
                            "unable to retrieve block at height {} for migration to rocksdb",
                            height,
                        );
                        continue;
                    }
                    Err(err) => {
                        error!(
                            "unable to retrieve parent block at height {} for migration to rocksdb {:?}",
                            height, err,
                        );
                        continue;
                    }
                };
                state_roots.push(*block_header.state_root_hash());
            }
            casper_node::migrate_lmdb_data_to_rocksdb(engine_state, state_roots, false);
            return Ok(());
        } else {
            error!("unable to find a highest block in storage");
            return Err(anyhow::anyhow!("unable to find a highest block in storage"));
        }
    }

    let load_height = opts.starting_block_height.saturating_sub(1);
    // Grab the block previous
    let previous_block = storage
        .read_block_by_height(load_height)?
        .unwrap_or_else(|| panic!("no block at height {}", load_height));

    let previous_block_header = previous_block.take_header();
    let (engine_state, _env) = storage::load_execution_engine(
        lmdb_path,
        rocksdb_path,
        max_db_size,
        *previous_block_header.state_root_hash(),
        opts.manual_sync_enabled,
        casper_execution_engine::rocksdb_defaults(),
    )?;

    let mut execution_pre_state = ExecutionPreState::from_block_header(
        &previous_block_header,
        verifiable_chunked_hash_activation,
    );
    let mut execute_count = 0;

    let highest_height_in_chain = storage.read_highest_block()?;
    let ending_block_height = opts
        .ending_block_height
        .unwrap_or_else(|| highest_height_in_chain.unwrap().take_header().height());

    info!(
        "Starting at height: {}\nExecuting blocks stored in {} height {} to {}.\n",
        previous_block_header.height(),
        opts.chain_download_path.display(),
        opts.starting_block_height,
        ending_block_height
    );

    if opts.verbose {
        info!("height, block_hash, transfer_count, deploy_count, execution_time_µs");
    }

    let progress = if !opts.verbose {
        let progress = ProgressBar::new(ending_block_height - previous_block_header.height());
        progress.set_style(
            ProgressStyle::default_bar()
                .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}")
                .progress_chars("##-"),
        );
        Some(progress)
    } else {
        None
    };

    let mut execution_time_hist = Histogram::new();
    for height in opts.starting_block_height..=ending_block_height {
        let block = storage.read_block_by_height(height)?.ok_or_else(|| {
            anyhow::anyhow!(
                "Block does not exist in downloaded chain at height {}",
                height
            )
        })?;

        let block_hash = *block.hash();
        let transfers = get_many_deploys_by_hash(&storage, block.transfer_hashes())?;
        let transfers_len = transfers.len();
        let deploys = get_many_deploys_by_hash(&storage, block.deploy_hashes())?;
        let deploys_len = deploys.len();
        let protocol_version = block.protocol_version();
        let finalized_block = FinalizedBlock::from(block.clone());

        let start = Instant::now();
        let block_and_execution_effects = execute_finalized_block(
            &engine_state,
            None,
            protocol_version,
            execution_pre_state,
            finalized_block,
            deploys,
            transfers,
            verifiable_chunked_hash_activation,
        )?;
        let elapsed_micros = start.elapsed().as_micros() as u64;
        execution_time_hist
            .increment(elapsed_micros)
            .map_err(anyhow::Error::msg)?;

        let header = block_and_execution_effects.block.take_header();
        let expected = block.take_header();
        assert_eq!(
            header.state_root_hash(),
            expected.state_root_hash(),
            "state root hash mismatch"
        );
        execution_pre_state =
            ExecutionPreState::from_block_header(&header, verifiable_chunked_hash_activation);
        execute_count += 1;
        if opts.verbose {
            info!(
                "{}, {}, {}, {}, {}",
                header.height(),
                block_hash,
                transfers_len,
                deploys_len,
                elapsed_micros,
            );
        } else {
            progress.as_ref().expect("should exist").inc(1);
        }
    }

    let maximum = execution_time_hist
        .maximum()
        .map_err(|err| anyhow::anyhow!(err))?;
    let duration50th = execution_time_hist
        .percentile(50.0)
        .map_err(anyhow::Error::msg)?;
    let duration999th = execution_time_hist
        .percentile(99.9)
        .map_err(anyhow::Error::msg)?;

    if !opts.verbose {
        progress.as_ref().expect("should exist").finish();
    }

    info!(
        "Executed {} blocks.\nMax: {}µs\n50th: {}µs\n99.9th: {}µs\n",
        execute_count, maximum, duration50th, duration999th
    );
    Ok(())
}
