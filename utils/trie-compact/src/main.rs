use std::{collections::HashSet, path::PathBuf};

use casper_hashing::Digest;
use structopt::StructOpt;

use retrieve_state::{
    storage,
    storage::{create_storage, normalize_path},
};

use tracing::info;
use trie_compact::copy_state_root;

#[derive(Debug, StructOpt)]
struct Opts {
    #[structopt(long, required = true, default_value = retrieve_state::CHAIN_DOWNLOAD_PATH)]
    storage_path: PathBuf,

    #[structopt(long, required = true, default_value = retrieve_state::LMDB_PATH)]
    source_lmdb_path: PathBuf,

    #[structopt(long, required = true)]
    destination_lmdb_path: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let opts = Opts::from_args();
    env_logger::init();

    let storage_path = normalize_path(&opts.storage_path)?;
    let source_lmdb_path = normalize_path(&opts.source_lmdb_path)?;
    let destination_lmdb_path = normalize_path(&opts.destination_lmdb_path)?;

    assert_neq!(
        source_lmdb_path,
        destination_lmdb_path,
        "Source and destination should not be the same lmdb database file."
    );

    let (source_state, _env) = storage::load_execution_engine(
        source_lmdb_path,
        retrieve_state::DEFAULT_MAX_DB_SIZE,
        Digest::default(),
        true,
    )?;

    let (destination_state, _env) = storage::create_execution_engine(
        destination_lmdb_path,
        retrieve_state::DEFAULT_MAX_DB_SIZE,
        true,
    )?;

    // Create a separate lmdb for block/deploy storage at chain_download_path.
    let storage = create_storage(&storage_path).expect("should create storage");

    let mut block = storage.read_highest_block().unwrap().unwrap();
    let mut visited_roots = HashSet::new();

    info!("Copying state roots from source to destination.");
    loop {
        let state_root = *block.state_root_hash();
        if !visited_roots.contains(&state_root) {
            copy_state_root(state_root, &source_state, &destination_state)
                .expect("should copy state root");
            destination_state
                .flush_environment()
                .expect("should flush to lmdb");
            visited_roots.insert(state_root);
        }
        if block.height() == 0 {
            break;
        }
        block = storage
            .read_block_by_height(block.height() - 1)
            .unwrap()
            .unwrap();
    }
    info!(
        "Finished copying {} state roots to new database.",
        visited_roots.len()
    );

    Ok(())
}
