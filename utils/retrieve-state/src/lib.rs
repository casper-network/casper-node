use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use anyhow::anyhow;
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

// TODO: make these parameters
const RPC_SERVER: &str = "http://localhost:11101/rpc";
pub const LMDB_PATH: &str = "lmdb-data";
pub const CHAIN_DOWNLOAD_PATH: &str = "chain-download";
pub const DEFAULT_TEST_MAX_DB_SIZE: usize = 483_183_820_800; // 450 gb
pub const DEFAULT_TEST_MAX_READERS: u32 = 512;

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

pub async fn get_block(
    client: &mut Client,
    url: &str,
    params: Option<GetBlockParams>,
) -> Result<GetBlockResult, anyhow::Error> {
    rpc(client, url, "chain_get_block", params).await
}

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

#[derive(Debug, Deserialize, Serialize)]
pub struct BlockWithDeploys {
    pub block: JsonBlock,
    pub transfers: Vec<Deploy>,
    pub deploys: Vec<Deploy>,
}

impl BlockWithDeploys {
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

pub async fn download_blocks(
    client: &mut Client,
    url: &str,
    chain_download_path: impl AsRef<Path>,
    mut block_hash: BlockHash,
    until_height: u64,
) -> Result<Vec<DirEntry>, anyhow::Error> {
    if !chain_download_path.as_ref().exists() {
        tokio::fs::create_dir_all(&chain_download_path).await?;
    }
    let mut start = Instant::now();
    loop {
        let block_with_deploys = download_block_with_deploys(client, url, block_hash).await?;
        block_with_deploys.save(&chain_download_path).await?;

        if block_with_deploys.block.header.height == until_height {
            break;
        }
        block_hash = block_with_deploys.block.header.parent_hash;
        if block_with_deploys.block.header.height % 1000 == 0 {
            println!(
                "downloaded block at height {} in {}ms",
                block_with_deploys.block.header.height,
                start.elapsed().as_millis()
            );
            start = Instant::now();
        }
    }
    println!("finished downloading blocks");
    Ok(offline::get_block_files(chain_download_path))
}

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
            let (trie, _): (Trie<Key, StoredValue>, _) = FromBytes::from_bytes(&bytes)
                .map_err(|error| anyhow!("unable to parse trie: {:?}", error))?;
            let mut missing_descendants = engine_state
                .put_trie_and_find_missing_descendant_trie_keys(CorrelationId::new(), &trie)?;
            outstanding_tries.append(&mut missing_descendants);
            tries_downloaded += 1;
        } else {
            return Err(anyhow!("unable to download trie at {:?}", next_trie_key));
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
    println!("downloaded {} tries", tries_downloaded);
    Ok(tries_downloaded)
}

pub async fn download_global_state_at_height(
    client: &mut Client,
    url: &str,
    engine_state: &EngineState<LmdbGlobalState>,
    genesis_block: &JsonBlock,
) -> Result<(), anyhow::Error> {
    // if we can't find this block in the trie, download it from a running node
    if !matches!(
        engine_state.get_trie(Default::default(), genesis_block.header.state_root_hash),
        Ok(Some(_))
    ) {
        download_trie(
            client,
            url,
            engine_state,
            genesis_block.header.state_root_hash,
        )
        .await?;
    }
    Ok(())
}

pub mod offline {
    use super::*;

    pub fn get_lowest_block_downloaded(
        chain_download_path: impl AsRef<Path>,
    ) -> Result<Option<u64>, anyhow::Error> {
        let lowest = if chain_download_path.as_ref().exists() {
            let existing_chain = walkdir::WalkDir::new(chain_download_path);
            let mut lowest_downloaded_block = 0;
            for entry in existing_chain {
                if let Some(filename) = entry?.file_name().to_str() {
                    let split = filename.split('-').collect::<Vec<&str>>();
                    if let ["block", height, _hash] = &split[..] {
                        let height: u64 = height.parse::<u64>()?;
                        lowest_downloaded_block = lowest_downloaded_block.min(height);
                    }
                }
            }
            Some(lowest_downloaded_block)
        } else {
            None
        };
        Ok(lowest)
    }

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

    pub fn create_execution_engine(
        lmdb_path: impl AsRef<Path>,
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
            DEFAULT_TEST_MAX_DB_SIZE,
            DEFAULT_TEST_MAX_READERS,
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

    pub async fn get_protocol_data<'de, T, P>(
        client: &mut Client,
        params: P,
    ) -> Result<T, anyhow::Error>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        let url = RPC_SERVER;
        let method = "info_get_protocol_data";
        let params = Params::from(json!(params));
        let rpc_req = JsonRpc::request_with_params(12345, method, params);
        let response = client.post(url).json(&rpc_req).send().await?;
        let rpc_res: JsonRpc = response.json().await?;
        if let Some(error) = rpc_res.get_error() {
            return Err(anyhow::format_err!(error.clone()));
        }
        let value = rpc_res.get_result().unwrap();
        let keys = value.get("protocol_data").unwrap();
        let deserialized = serde_json::from_value(keys.clone())?;
        Ok(deserialized)
    }

    pub async fn read_block_file(
        block_file_entry: &DirEntry,
    ) -> Result<BlockWithDeploys, anyhow::Error> {
        let mut file = File::open(block_file_entry.path()).await?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).await?;
        Ok(serde_json::from_slice::<BlockWithDeploys>(&buffer)?)
    }
}
