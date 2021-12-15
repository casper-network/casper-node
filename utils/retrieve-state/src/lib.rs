use std::time::Instant;

use reqwest::Client;
use serde::{Deserialize, Serialize};

use casper_execution_engine::{
    core::engine_state::EngineState,
    shared::newtypes::CorrelationId,
    storage::{global_state::lmdb::LmdbGlobalState, trie::Trie},
};
use casper_hashing::Digest;
use casper_node::{
    rpcs::{
        chain::{BlockIdentifier, GetBlockParams, GetBlockResult},
        info::{GetDeployParams, GetDeployResult},
        state::{GetTrieParams, GetTrieResult},
    },
    storage::Storage,
    types::{Block, BlockHash, Deploy, DeployOrTransferHash, JsonBlock},
};
use casper_types::{bytesrepr::FromBytes, Key, StoredValue};

pub mod rpc;
pub mod storage;

pub const LMDB_PATH: &str = "lmdb-data";
pub const CHAIN_DOWNLOAD_PATH: &str = "chain-download";
pub const DEFAULT_MAX_DB_SIZE: usize = 483_183_820_800; // 450 gb
pub const DEFAULT_MAX_READERS: u32 = 512;

/// Specific representations of  errors from `get_block`.
#[derive(thiserror::Error, Debug)]
pub enum GetBlockError {
    /// All other errors from Rpc.
    #[error(transparent)]
    Rpc(#[from] rpc::Error),
    /// Node responded with "block not known". This means we're asking for a block that the node
    /// doesn't know about. Was it from a different network?
    #[error("block not known. params: {maybe_block_identifier:?}, rpc error: {rpc_error_message}")]
    BlockNotKnown {
        maybe_block_identifier: Option<BlockIdentifier>,
        rpc_error_message: String,
    },
}

/// Get a block with optional parameters.
pub async fn get_block(
    client: &mut Client,
    url: &str,
    params: Option<GetBlockParams>,
) -> Result<GetBlockResult, GetBlockError> {
    let maybe_block_identifier = params.as_ref().map(|value| value.block_identifier);
    let result = rpc::rpc(client, url, "chain_get_block", params).await;
    match result {
        Err(e) => match e {
            // The node rpc treats "block not found" as an error, so we need to handle this, and
            // translate it into our own application level error.
            rpc::Error::JsonRpc(jsonrpc_lite::Error { ref message, .. })
                if message == "block not known" =>
            {
                Err(GetBlockError::BlockNotKnown {
                    maybe_block_identifier,
                    rpc_error_message: message.clone(),
                })
            }
            _ => Err(e.into()),
        },
        other => other.map_err(Into::into),
    }
}

/// Get the genesis block.
pub async fn get_genesis_block(
    client: &mut Client,
    url: &str,
) -> Result<GetBlockResult, rpc::Error> {
    rpc::rpc(
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
) -> Result<GetTrieResult, rpc::Error> {
    rpc::rpc(client, url, "state_get_trie", params).await
}

async fn get_deploy(
    client: &mut Client,
    url: &str,
    params: GetDeployParams,
) -> Result<GetDeployResult, rpc::Error> {
    rpc::rpc(client, url, "info_get_deploy", params).await
}

/// Composes a [`JsonBlock`] along with all of it's transfers and deploys.
#[derive(Debug, Deserialize, Serialize)]
pub struct BlockWithDeploys {
    pub block: JsonBlock,
    pub transfers: Vec<Deploy>,
    pub deploys: Vec<Deploy>,
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
                finalized_approvals: true,
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
                finalized_approvals: true,
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
pub async fn download_or_read_blocks(
    client: &mut Client,
    storage: &mut Storage,
    url: &str,
    highest_block: Option<&BlockIdentifier>,
    progress_fn: impl Fn() + Send + Sync,
) -> Result<(usize, usize), anyhow::Error> {
    let mut new_blocks_downloaded = 0;
    let mut blocks_read_from_disk = 0;
    let mut maybe_next_block_hash_and_source = match highest_block.as_ref() {
        Some(block_identifier) => {
            download_or_read_block_and_extract_parent_hash(client, storage, url, *block_identifier)
                .await?
        }
        // Get the highest block.
        None => get_block_and_download(client, storage, url, None)
            .await?
            .map(|block_hash| (block_hash, BlockSource::Http)),
    };
    while let Some((next_block_hash, source)) = maybe_next_block_hash_and_source.as_ref() {
        match source {
            BlockSource::Http => {
                new_blocks_downloaded += 1;
            }
            BlockSource::Storage => {
                blocks_read_from_disk += 1;
            }
        }
        maybe_next_block_hash_and_source = download_or_read_block_and_extract_parent_hash(
            client,
            storage,
            url,
            &BlockIdentifier::Hash(*next_block_hash),
        )
        .await?;
        progress_fn();
    }
    Ok((new_blocks_downloaded, blocks_read_from_disk))
}

enum BlockSource {
    Http,
    Storage,
}

/// Get a block by it's [`BlockIdentifier`].
pub fn get_block_by_identifier(
    storage: &Storage,
    identifier: &BlockIdentifier,
) -> Result<Option<Block>, anyhow::Error> {
    match identifier {
        BlockIdentifier::Hash(ref block_hash) => Ok(storage.read_block(block_hash)?),
        BlockIdentifier::Height(ref height) => Ok(storage.read_block_by_height(*height)?),
    }
}

async fn download_or_read_block_and_extract_parent_hash(
    client: &mut Client,
    storage: &mut Storage,
    url: &str,
    block_identifier: &BlockIdentifier,
) -> Result<Option<(BlockHash, BlockSource)>, anyhow::Error> {
    match get_block_by_identifier(storage, block_identifier)? {
        None => Ok(
            get_block_and_download(client, storage, url, Some(*block_identifier))
                .await?
                .map(|block| (block, BlockSource::Http)),
        ),
        Some(block) => {
            if block.height() != 0 {
                Ok(Some((
                    *block.take_header().parent_hash(),
                    BlockSource::Storage,
                )))
            } else {
                Ok(None)
            }
        }
    }
}

async fn get_block_and_download(
    client: &mut Client,
    storage: &mut Storage,
    url: &str,
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
        put_block_with_deploys(storage, &block_with_deploys)?;
        if block_with_deploys.block.header.height != 0 {
            Ok(Some(block_with_deploys.block.header.parent_hash))
        } else {
            Ok(None)
        }
    } else {
        Err(anyhow::anyhow!("unable to download highest block"))
    }
}

/// Store a single [`BlockWithDeploys`]'s [`Block`], `deploys` and `transfers`.
pub fn put_block_with_deploys(
    storage: &mut Storage,
    block_with_deploys: &BlockWithDeploys,
) -> Result<(), anyhow::Error> {
    for deploy in block_with_deploys.deploys.iter() {
        if let DeployOrTransferHash::Deploy(_hash) = deploy.deploy_or_transfer_hash() {
            storage.put_deploy(deploy)?;
        } else {
            return Err(anyhow::anyhow!("transfer found in list of deploys"));
        }
    }
    for transfer in block_with_deploys.transfers.iter() {
        if let DeployOrTransferHash::Transfer(_hash) = transfer.deploy_or_transfer_hash() {
            storage.put_deploy(transfer)?;
        } else {
            return Err(anyhow::anyhow!("deploy found in list of transfers"));
        }
    }
    let block: Block = block_with_deploys.block.clone().into();
    storage.write_block(&block)?;
    Ok(())
}

/// Download the trie from a node to the provided lmdb path.
pub async fn download_trie(
    client: &mut Client,
    url: &str,
    engine_state: &EngineState<LmdbGlobalState>,
    state_root_hash: Digest,
    progress_fn: impl Fn() + Send + Sync,
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
            let (trie, _): (Trie<Key, StoredValue>, _) =
                FromBytes::from_bytes(&bytes).map_err(anyhow::Error::msg)?;
            let mut missing_descendants = engine_state
                .put_trie_and_find_missing_descendant_trie_keys(CorrelationId::new(), &trie)?;
            outstanding_tries.append(&mut missing_descendants);
            tries_downloaded += 1;
            progress_fn()
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
