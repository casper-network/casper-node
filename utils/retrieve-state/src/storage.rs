use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use lmdb::DatabaseFlags;
use num_rational::Ratio;
use tracing::info;

use casper_execution_engine::{
    core::engine_state::{EngineConfig, EngineState},
    storage::{
        global_state::lmdb::LmdbGlobalState, transaction_source::lmdb::LmdbEnvironment,
        trie_store::lmdb::LmdbTrieStore,
    },
};
use casper_hashing::Digest;
use casper_node::{
    storage::Storage,
    types::{Deploy, DeployHash},
    StorageConfig, WithDir,
};
use casper_types::ProtocolVersion;

use crate::DEFAULT_MAX_READERS;

const RECENT_ERA_COUNT: u64 = 5;

/// Gets many deploys by hash.
pub fn get_many_deploys_by_hash(
    storage: &Storage,
    hashes: &[DeployHash],
) -> Result<Vec<Deploy>, anyhow::Error> {
    let mut deploys = vec![];
    for deploy_hash in hashes {
        let deploy = match storage.read_deploy_by_hash(*deploy_hash)? {
            None => {
                return Err(anyhow::anyhow!(
                    "Deploy is present in block but hasn't been downloaded."
                ))
            }
            Some(deploy) => deploy,
        };
        deploys.push(deploy);
    }
    Ok(deploys)
}

/// Create an lmdb environment at a given path.
fn create_lmdb_environment(
    lmdb_path: impl AsRef<Path>,
    default_max_db_size: usize,
    manual_sync_enabled: bool,
) -> Result<Arc<LmdbEnvironment>, anyhow::Error> {
    let lmdb_environment = Arc::new(LmdbEnvironment::new(
        &lmdb_path,
        default_max_db_size,
        DEFAULT_MAX_READERS,
        manual_sync_enabled,
    )?);
    Ok(lmdb_environment)
}

/// Loads an existing execution engine.
pub fn load_execution_engine(
    ee_lmdb_path: impl AsRef<Path>,
    default_max_db_size: usize,
    state_root_hash: Digest,
    manual_sync_enabled: bool,
) -> Result<(Arc<EngineState<LmdbGlobalState>>, Arc<LmdbEnvironment>), anyhow::Error> {
    let lmdb_data_file = ee_lmdb_path.as_ref().join("data.lmdb");
    if !ee_lmdb_path.as_ref().join("data.lmdb").exists() {
        return Err(anyhow::anyhow!(
            "lmdb data file not found at: {}",
            lmdb_data_file.display()
        ));
    }
    let lmdb_environment =
        create_lmdb_environment(&ee_lmdb_path, default_max_db_size, manual_sync_enabled)?;
    let lmdb_trie_store = Arc::new(LmdbTrieStore::open(&lmdb_environment, None)?);
    let global_state = LmdbGlobalState::new(
        Arc::clone(&lmdb_environment),
        lmdb_trie_store,
        state_root_hash,
    );
    Ok((
        Arc::new(EngineState::new(global_state, EngineConfig::default())),
        lmdb_environment,
    ))
}

/// Creates a new execution engine.
pub fn create_execution_engine(
    ee_lmdb_path: impl AsRef<Path>,
    default_max_db_size: usize,
    manual_sync_enabled: bool,
) -> Result<(Arc<EngineState<LmdbGlobalState>>, Arc<LmdbEnvironment>), anyhow::Error> {
    if !ee_lmdb_path.as_ref().exists() {
        info!(
            "creating new lmdb data dir {}",
            ee_lmdb_path.as_ref().display()
        );
        fs::create_dir_all(&ee_lmdb_path)?;
    }
    fs::create_dir_all(&ee_lmdb_path)?;
    let lmdb_environment =
        create_lmdb_environment(&ee_lmdb_path, default_max_db_size, manual_sync_enabled)?;
    lmdb_environment.env().sync(true)?;

    let lmdb_trie_store = Arc::new(LmdbTrieStore::new(
        &lmdb_environment,
        None,
        DatabaseFlags::empty(),
    )?);
    let global_state = LmdbGlobalState::empty(Arc::clone(&lmdb_environment), lmdb_trie_store)?;

    Ok((
        Arc::new(EngineState::new(global_state, EngineConfig::default())),
        lmdb_environment,
    ))
}

pub fn normalize_path(path: impl AsRef<Path>) -> Result<PathBuf, anyhow::Error> {
    let path = path.as_ref();
    if path.is_absolute() {
        Ok(path.into())
    } else {
        Ok(env::current_dir()?.join(path))
    }
}

pub fn create_storage(chain_download_path: impl AsRef<Path>) -> Result<Storage, anyhow::Error> {
    let chain_download_path = normalize_path(chain_download_path)?;
    let mut storage_config = StorageConfig::default();
    storage_config.path = chain_download_path.clone();
    Ok(Storage::new(
        &WithDir::new(chain_download_path, storage_config),
        Ratio::new(1, 3),
        None,
        ProtocolVersion::from_parts(0, 0, 0),
        "test",
        RECENT_ERA_COUNT,
    )?)
}

#[cfg(test)]
mod tests {
    use crate::{
        get_block_by_identifier, put_block_with_deploys, storage::create_storage, BlockWithDeploys,
    };
    use casper_node::rpcs::{
        chain::{BlockIdentifier, GetBlockResult},
        docs::DocExample,
        info::GetDeployResult,
    };

    #[test]
    fn block_with_deploys_round_trip_lmdb() {
        let example_block = GetBlockResult::doc_example()
            .block
            .as_ref()
            .unwrap()
            .clone();

        let dir = tempfile::tempdir().unwrap().into_path();
        let mut storage = create_storage(dir).expect("should create storage");

        let example_deploy = GetDeployResult::doc_example().deploy.clone();

        let block_with_deploys = BlockWithDeploys {
            block: example_block.clone(),
            transfers: vec![example_deploy.clone()],
            deploys: vec![],
        };

        put_block_with_deploys(&mut storage, &block_with_deploys).unwrap();
        let stored_block =
            get_block_by_identifier(&storage, &BlockIdentifier::Hash(example_block.hash));
        assert!(matches!(stored_block, Ok(Some(ref _block))));

        let stored_block_by_height = get_block_by_identifier(
            &storage,
            &BlockIdentifier::Height(example_block.header.height),
        );
        assert!(matches!(stored_block, Ok(Some(ref _block))));

        assert_eq!(
            stored_block.unwrap().unwrap(),
            stored_block_by_height.unwrap().unwrap()
        );

        let stored_deploy = storage.read_deploy_by_hash(*example_deploy.id());
        assert!(matches!(stored_deploy, Ok(Some(_deploy))));
    }
}
