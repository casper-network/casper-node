use std::{
    env,
    path::{Path, PathBuf},
    sync::Arc,
};

use casper_execution_engine::{
    core::engine_state::{EngineConfig, EngineState},
    storage::{global_state::db::DbGlobalState, trie_store::db::RocksDbTrieStore},
};
use casper_node::{
    storage::Storage,
    types::{Deploy, DeployHash},
    StorageConfig, WithDir,
};
use casper_types::{EraId, ProtocolVersion};
use num_rational::Ratio;

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

/// Loads an existing execution engine.
pub fn load_execution_engine(
    ee_lmdb_path: impl AsRef<Path>,
    rocksdb_path: impl AsRef<Path>,
) -> Result<Arc<EngineState<DbGlobalState>>, anyhow::Error> {
    let lmdb_data_file = ee_lmdb_path.as_ref().join("data.lmdb");
    if !ee_lmdb_path.as_ref().join("data.lmdb").exists() {
        return Err(anyhow::anyhow!(
            "lmdb data file not found at: {}",
            lmdb_data_file.display()
        ));
    }
    let global_state = DbGlobalState::empty(
        Some(ee_lmdb_path.as_ref().to_path_buf()),
        RocksDbTrieStore::new(rocksdb_path)?,
    )?;
    Ok(Arc::new(EngineState::new(
        global_state,
        EngineConfig::default(),
    )))
}

/// Creates a new execution engine.
pub fn create_execution_engine(
    rocksdb_path: impl AsRef<Path>,
) -> Result<Arc<EngineState<DbGlobalState>>, anyhow::Error> {
    let global_state = DbGlobalState::empty(None, RocksDbTrieStore::new(rocksdb_path.as_ref())?)?;
    Ok(Arc::new(EngineState::new(
        global_state,
        EngineConfig::default(),
    )))
}

pub fn normalize_path(path: impl AsRef<Path>) -> Result<PathBuf, anyhow::Error> {
    let path = path.as_ref();
    if path.is_absolute() {
        Ok(path.into())
    } else {
        Ok(env::current_dir()?.join(path))
    }
}

pub fn create_storage(
    chain_download_path: impl AsRef<Path>,
    verifiable_chunked_hash_activation: EraId,
) -> Result<Storage, anyhow::Error> {
    let chain_download_path = normalize_path(chain_download_path)?;
    let mut storage_config = StorageConfig::default();
    storage_config.path = chain_download_path.clone();
    Ok(Storage::new(
        &WithDir::new(chain_download_path, storage_config),
        None,
        ProtocolVersion::from_parts(0, 0, 0),
        "test",
        Ratio::new(1, 3),
        None,
        verifiable_chunked_hash_activation,
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
        let mut storage =
            create_storage(dir, example_block.header.height.into()).expect("should create storage");

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
