use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use jsonrpc_lite::{JsonRpc, Params};
use lmdb::DatabaseFlags;
use reqwest::Client;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::json;
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt},
};
use walkdir::DirEntry;

use casper_execution_engine::{
    core::engine_state::{EngineConfig, EngineState},
    shared::newtypes::CorrelationId,
    storage::{
        global_state::lmdb::LmdbGlobalState, transaction_source::lmdb::LmdbEnvironment, trie::Trie,
        trie_store::lmdb::LmdbTrieStore,
    },
};
use casper_hashing::Digest;
use casper_node::{
    rpcs::{
        chain::{BlockIdentifier, GetBlockParams, GetBlockResult},
        info::{GetDeployParams, GetDeployResult},
        state::{GetTrieParams, GetTrieResult},
    },
    types::{BlockHash, Deploy, JsonBlock},
};
use casper_types::{bytesrepr::FromBytes, Key, StoredValue};

use crate::offline::{get_block_file, read_block_file};

pub const LMDB_PATH: &str = "lmdb-data";
pub const CHAIN_DOWNLOAD_PATH: &str = "chain-download";
pub const DEFAULT_MAX_DB_SIZE: usize = 483_183_820_800; // 450 gb
pub const DEFAULT_MAX_READERS: u32 = 512;

async fn rpc<'de, R, P>(
    client: &mut Client,
    url: &str,
    method: &str,
    params: P,
) -> Result<R, anyhow::Error>
where
    R: DeserializeOwned,
    P: Serialize,
{
    let params = Params::from(json!(params));
    let rpc_req = JsonRpc::request_with_params(12345, method, params);
    let response = client.post(url).json(&rpc_req).send().await?;
    let rpc_res: JsonRpc = response.json().await?;
    if let Some(error) = rpc_res.get_error() {
        return Err(anyhow::format_err!(error.clone()));
    }
    let value = rpc_res.get_result().unwrap();
    let deserialized = serde_json::from_value(value.clone())?;
    Ok(deserialized)
}

/// Get a block with optional parameters.
pub async fn get_block(
    client: &mut Client,
    url: &str,
    params: Option<GetBlockParams>,
) -> Result<GetBlockResult, anyhow::Error> {
    rpc(client, url, "chain_get_block", params).await
}

/// Get the genesis block.
pub async fn get_genesis_block(
    client: &mut Client,
    url: &str,
) -> Result<GetBlockResult, anyhow::Error> {
    rpc(
        client,
        url,
        "chain_get_block",
        Some(GetBlockParams {
            block_identifier: BlockIdentifier::Height(0),
        }),
    )
    .await
}

async fn get_trie(
    client: &mut Client,
    url: &str,
    params: GetTrieParams,
) -> Result<GetTrieResult, anyhow::Error> {
    rpc(client, url, "state_get_trie", params).await
}

async fn get_deploy(
    client: &mut Client,
    url: &str,
    params: GetDeployParams,
) -> Result<GetDeployResult, anyhow::Error> {
    rpc(client, url, "info_get_deploy", params).await
}

/// Composes a [`JsonBlock`] along with all of it's transfers and deploys.
#[derive(Debug, Deserialize, Serialize)]
pub struct BlockWithDeploys {
    pub block: JsonBlock,
    pub transfers: Vec<Deploy>,
    pub deploys: Vec<Deploy>,
}

impl BlockWithDeploys {
    /// Save [`BlockWithDeploys`] to disk, in the provided file path.
    pub async fn save(&self, path: impl AsRef<Path>) -> Result<(), anyhow::Error> {
        let path = PathBuf::from(path.as_ref());
        let file_path = path.join(format!(
            "block-{:0>24}-{}.json",
            self.block.header.height,
            hex::encode(self.block.hash)
        ));
        let mut file = File::create(file_path).await?;
        let json = serde_json::to_string_pretty(self)?;
        file.write_all(json.as_bytes()).await?;
        Ok(())
    }
}

/// Download a block, along with all it's deploys.
pub async fn download_block_with_deploys(
    client: &mut Client,
    url: &str,
    block_hash: BlockHash,
) -> Result<BlockWithDeploys, anyhow::Error> {
    let block_identifier = BlockIdentifier::Hash(block_hash);
    let block = get_block(client, url, Some(GetBlockParams { block_identifier }))
        .await?
        .block
        .unwrap();

    let mut transfers = Vec::new();
    for transfer_hash in block.transfer_hashes() {
        let transfer: Deploy = get_deploy(
            client,
            url,
            GetDeployParams {
                deploy_hash: *transfer_hash,
            },
        )
        .await?
        .deploy;
        transfers.push(transfer);
    }

    let mut deploys = Vec::new();
    for deploy_hash in block.deploy_hashes() {
        let deploy: Deploy = get_deploy(
            client,
            url,
            GetDeployParams {
                deploy_hash: *deploy_hash,
            },
        )
        .await?
        .deploy;
        deploys.push(deploy);
    }

    Ok(BlockWithDeploys {
        block,
        transfers,
        deploys,
    })
}

/// Download block files from the highest (provided) block hash to genesis.
pub async fn download_blocks(
    client: &mut Client,
    url: &str,
    chain_download_path: impl AsRef<Path>,
    highest_block: Option<&BlockIdentifier>,
) -> Result<Vec<DirEntry>, anyhow::Error> {
    if !chain_download_path.as_ref().exists() {
        tokio::fs::create_dir_all(&chain_download_path).await?;
    }
    let mut maybe_next_block_hash: Option<BlockHash> = match highest_block.as_ref() {
        Some(block_identifier) => {
            download_or_read_block_file_and_extract_parent_hash(
                client,
                url,
                &chain_download_path,
                *block_identifier,
            )
            .await?
        }
        // Get the highest block.
        None => get_block_and_download(client, url, &chain_download_path, None).await?,
    };
    while let Some(next_block_hash) = maybe_next_block_hash {
        maybe_next_block_hash = download_or_read_block_file_and_extract_parent_hash(
            client,
            url,
            &chain_download_path,
            &BlockIdentifier::Hash(next_block_hash),
        )
        .await?;
    }
    Ok(offline::get_block_files(chain_download_path))
}

async fn download_or_read_block_file_and_extract_parent_hash(
    client: &mut Client,
    url: &str,
    chain_download_path: &impl AsRef<Path>,
    block_identifier: &BlockIdentifier,
) -> Result<Option<BlockHash>, anyhow::Error> {
    match get_block_file(&chain_download_path, block_identifier) {
        None => {
            Ok(
                get_block_and_download(client, url, chain_download_path, Some(*block_identifier))
                    .await?,
            )
        }
        Some(block_file) => {
            let block_with_deploys = read_block_file(&block_file).await?;
            if block_with_deploys.block.header.height != 0 {
                Ok(Some(block_with_deploys.block.header.parent_hash))
            } else {
                Ok(None)
            }
        }
    }
}

async fn get_block_and_download(
    client: &mut Client,
    url: &str,
    chain_download_path: &impl AsRef<Path>,
    block_identifier: Option<BlockIdentifier>,
) -> Result<Option<BlockHash>, anyhow::Error> {
    // passing None for block_identifier will fetch highest
    let get_block_params =
        block_identifier.map(|block_identifier| GetBlockParams { block_identifier });
    let block = get_block(client, url, get_block_params).await?;
    if let GetBlockResult {
        block: Some(json_block),
        ..
    } = block
    {
        let block_with_deploys = download_block_with_deploys(client, url, json_block.hash).await?;
        block_with_deploys.save(&chain_download_path).await?;

        if block_with_deploys.block.header.height != 0 {
            Ok(Some(block_with_deploys.block.header.parent_hash))
        } else {
            Ok(None)
        }
    } else {
        Err(anyhow::anyhow!("unable to download highest block"))
    }
}

/// Download the trie from a node to the provided lmdb path.
pub async fn download_trie(
    client: &mut Client,
    url: &str,
    engine_state: &EngineState<LmdbGlobalState>,
    state_root_hash: Digest,
) -> Result<usize, anyhow::Error> {
    let mut outstanding_tries = vec![state_root_hash];

    let mut start = Instant::now();
    let mut tries_downloaded = 0;
    while let Some(next_trie_key) = outstanding_tries.pop() {
        let read_result = get_trie(
            client,
            url,
            GetTrieParams {
                trie_key: next_trie_key,
            },
        )
        .await?;
        if let Some(blob) = read_result.maybe_trie_bytes {
            let bytes: Vec<u8> = blob.into();
            let (trie, _): (Trie<Key, StoredValue>, _) = FromBytes::from_bytes(&bytes)?;
            let mut missing_descendants = engine_state
                .put_trie_and_find_missing_descendant_trie_keys(CorrelationId::new(), &trie)?;
            outstanding_tries.append(&mut missing_descendants);
            tries_downloaded += 1;
        } else {
            return Err(anyhow::anyhow!(
                "unable to download trie at {:?}",
                next_trie_key
            ));
        }
        if tries_downloaded % 1000 == 0 {
            println!(
                "downloaded {} tries in {}ms",
                tries_downloaded,
                start.elapsed().as_millis()
            );
            start = Instant::now();
        }
    }
    println!("Downloaded {} tries", tries_downloaded);
    Ok(tries_downloaded)
}

/// Download global state from `url` using the provided block for it's `state_root_hash`.
pub async fn download_global_state_at_height(
    client: &mut Client,
    url: &str,
    engine_state: &EngineState<LmdbGlobalState>,
    block: &JsonBlock,
) -> Result<(), anyhow::Error> {
    // if we can't find this block in the trie, download it from a running node
    if !matches!(
        engine_state.get_trie(Default::default(), block.header.state_root_hash),
        Ok(Some(_))
    ) {
        download_trie(client, url, engine_state, block.header.state_root_hash).await?;
    }
    Ok(())
}

pub mod offline {
    use super::*;

    /// Returns the height of the lowest block already downloaded.
    pub fn get_lowest_block_downloaded(
        chain_download_path: impl AsRef<Path>,
    ) -> Result<Option<u64>, anyhow::Error> {
        get_block_downloaded(chain_download_path, |a, b| a.min(b))
    }

    /// Returns the height of the highest block already downloaded.
    pub fn get_highest_block_downloaded(
        chain_download_path: impl AsRef<Path>,
    ) -> Result<Option<u64>, anyhow::Error> {
        get_block_downloaded(chain_download_path, |a, b| a.max(b))
    }

    fn get_block_downloaded(
        chain_download_path: impl AsRef<Path>,
        comparator: impl Fn(u64, u64) -> u64,
    ) -> Result<Option<u64>, anyhow::Error> {
        let maybe_height = if chain_download_path.as_ref().exists() {
            let existing_chain = walkdir::WalkDir::new(chain_download_path);
            let mut height_comparison = None;
            for entry in existing_chain {
                if let Some(filename) = entry?.file_name().to_str() {
                    let split = filename.split('-').collect::<Vec<&str>>();
                    if let ["block", height, _hash] = &split[..] {
                        let height: u64 = height.parse::<u64>()?;
                        height_comparison =
                            Some(comparator(height_comparison.unwrap_or(height), height));
                    }
                }
            }
            height_comparison
        } else {
            None
        };
        Ok(maybe_height)
    }

    /// Walks the chain-download directory and finds blocks that have already been downloaded.
    pub fn get_block_files(chain_path: impl AsRef<Path>) -> Vec<DirEntry> {
        let mut block_files = walkdir::WalkDir::new(chain_path)
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let file_name = entry.file_name().to_str()?;
                let split = file_name.split('-').collect::<Vec<&str>>();
                if let ["block", _height, _hash] = &split[..] {
                    Some(entry)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        block_files.sort_by_key(|entry| entry.file_name().to_str().unwrap().to_string());
        block_files
    }

    /// Look for a block file matching the given [`BlockIdentifier`], returns `None` if not found.
    pub fn get_block_file(
        chain_path: impl AsRef<Path>,
        block_identifier: &BlockIdentifier,
    ) -> Option<DirEntry> {
        walkdir::WalkDir::new(chain_path)
            .into_iter()
            .filter_map(|entry| entry.ok())
            .find(|entry| {
                let file_name = match entry.file_name().to_str() {
                    Some(file_name) => file_name,
                    None => return false,
                };
                let split = file_name.split('-').collect::<Vec<&str>>();
                if let ["block", height, hash] = &split[..] {
                    match block_identifier {
                        BlockIdentifier::Height(id_height) => *height == id_height.to_string(),
                        BlockIdentifier::Hash(id_hash) => {
                            *hash == format!("{}.json", id_hash.inner().to_string())
                        }
                    }
                } else {
                    false
                }
            })
    }

    /// Creates a new execution engine.
    pub fn create_execution_engine(
        lmdb_path: impl AsRef<Path>,
        default_max_db_size: usize,
    ) -> Result<Arc<EngineState<LmdbGlobalState>>, anyhow::Error> {
        if !lmdb_path.as_ref().exists() {
            println!(
                "creating new lmdb data dir {}",
                lmdb_path.as_ref().display()
            );
            fs::create_dir_all(&lmdb_path)?;
        }

        fs::create_dir_all(&lmdb_path)?;
        let lmdb_environment = Arc::new(LmdbEnvironment::new(
            &lmdb_path,
            default_max_db_size,
            DEFAULT_MAX_READERS,
            true,
        )?);
        lmdb_environment.env().sync(true)?;

        let lmdb_trie_store = Arc::new(LmdbTrieStore::new(
            &lmdb_environment,
            None,
            DatabaseFlags::empty(),
        )?);
        let global_state = LmdbGlobalState::empty(lmdb_environment, lmdb_trie_store)?;

        Ok(Arc::new(EngineState::new(
            global_state,
            EngineConfig::default(),
        )))
    }

    /// Read a [`BlockWithDeploys`] from a file on disk.
    pub async fn read_block_file(
        block_file_entry: &DirEntry,
    ) -> Result<BlockWithDeploys, anyhow::Error> {
        let mut file = File::open(block_file_entry.path()).await?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).await?;
        Ok(serde_json::from_slice::<BlockWithDeploys>(&buffer)?)
    }
}
