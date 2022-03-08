use std::{
    net::{AddrParseError, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};

use futures::stream::FuturesUnordered;
use futures_util::stream::StreamExt;
pub use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{mpsc, Mutex},
    time::error::Elapsed,
};
use tracing::{error, info, trace, warn};

use casper_execution_engine::{
    core::engine_state::EngineState,
    shared::newtypes::CorrelationId,
    storage::{global_state::db::DbGlobalState, trie::Trie},
};
use casper_hashing::Digest;
use casper_node::{
    rpcs::{
        chain::{
            BlockIdentifier, GetBlockParams, GetBlockResult, GetEraInfoParams, GetEraInfoResult,
        },
        info::{GetDeployParams, GetDeployResult, GetPeersResult},
        state::{
            GetItemParams, GetItemResult, GetTrieParams, GetTrieResult, QueryGlobalStateParams,
            QueryGlobalStateResult,
        },
    },
    storage::Storage,
    types::{Block, BlockHash, Deploy, DeployOrTransferHash, JsonBlock},
    utils::work_queue::WorkQueue,
};
use casper_types::{
    bytesrepr::{self, FromBytes},
    Key, StoredValue,
};

pub mod rpc;
pub mod storage;

pub const LMDB_PATH: &str = "lmdb-data";
pub const ROCKSDB_PATH: &str = "rocksdb-data";
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

/// Get the list of peers from the perspective of the queried node.
pub async fn get_peers_list(client: &Client, url: &str) -> Result<GetPeersResult, rpc::Error> {
    rpc::rpc_without_params(client, url, "info_get_peers").await
}

/// Query global state for StoredValue
pub async fn query_global_state(
    client: &Client,
    url: &str,
    params: QueryGlobalStateParams,
) -> Result<QueryGlobalStateResult, rpc::Error> {
    let result = rpc::rpc(client, url, "query_global_state", params).await?;
    Ok(result)
}

/// Query global state for EraInfo
pub async fn get_era_info(
    client: &Client,
    url: &str,
    params: GetEraInfoParams,
) -> Result<GetEraInfoResult, rpc::Error> {
    let result = rpc::rpc(client, url, "chain_get_era_info", params).await?;
    Ok(result)
}

/// Get a block with optional parameters.
pub async fn get_block(
    client: &Client,
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
pub async fn get_genesis_block(client: &Client, url: &str) -> Result<GetBlockResult, rpc::Error> {
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

/// Query a node for a trie.
pub async fn get_trie(
    client: &Client,
    url: &str,
    params: GetTrieParams,
) -> Result<GetTrieResult, rpc::Error> {
    rpc::rpc(client, url, "state_get_trie", params).await
}

/// Query a node for an item.
pub async fn get_item(
    client: &Client,
    url: &str,
    params: GetItemParams,
) -> Result<GetItemResult, rpc::Error> {
    rpc::rpc(client, url, "state_get_item", params).await
}

/// Query a node for a deploy.
pub async fn get_deploy(
    client: &Client,
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
    client: &Client,
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
pub async fn download_or_read_blocks(
    client: &Client,
    storage: &mut Storage,
    url: &str,
    highest_block: Option<&BlockIdentifier>,
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
        info!("downloaded block {:?}", maybe_next_block_hash_and_source);
    }
    Ok((new_blocks_downloaded, blocks_read_from_disk))
}

#[derive(Debug)]
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
    client: &Client,
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
    client: &Client,
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

#[derive(Debug, thiserror::Error)]
pub enum PeerError {
    #[error(transparent)]
    AddrParseError(#[from] AddrParseError),

    /// Peer failed to download.
    #[error("FailedRpc {trie_key:?} from {peer_address:?} due to {error:?}")]
    FailedRpc {
        peer_address: SocketAddr,
        trie_key: Digest,
        error: rpc::Error,
    },

    #[error("TimedOut {trie_key:?} from {peer_address:?} due to {error:?}")]
    TimedOut {
        peer_address: SocketAddr,
        trie_key: Digest,
        error: Elapsed,
    },

    #[error("BadData {trie_key:?} from {peer_address:?} due to {error:?}")]
    BadData {
        peer_address: SocketAddr,
        trie_key: Digest,
        error: bytesrepr::Error,
    },
}

pub enum PeerMsg {
    /// Download and save the trie under this
    GetTrie(Digest),
    TrieDownloaded {
        trie: Box<Trie<Key, StoredValue>>,
        peer: SocketAddr,
        elapsed: Duration,
        len_in_bytes: usize,
    },
    NoBytes {
        trie_key: Digest,
        peer: SocketAddr,
    },
}

/// Download the trie from a node to the provided lmdb path.
pub async fn download_trie_work_queue(
    client: &Client,
    peers: &[SocketAddr],
    engine_state: Arc<EngineState<DbGlobalState>>,
    root_hashes: Vec<Digest>,
    peer_mailbox_size: usize,
) -> Result<(), anyhow::Error> {
    let tested_peers = check_peers_for_get_trie_endpoint(client, peers, root_hashes[0]).await;
    info!("{} valid peers", tested_peers.len());
    sync_trie_store(
        engine_state,
        root_hashes,
        client.clone(),
        &tested_peers,
        peer_mailbox_size,
    )
    .await?;

    Ok(())
}

async fn check_peers_for_get_trie_endpoint(
    client: &Client,
    peers: &[SocketAddr],
    state_root_hash: Digest,
) -> Vec<SocketAddr> {
    let mut peer_futures = Vec::new();
    for peer in peers {
        let peer_future = async move {
            let url = address_to_url(*peer);
            let req_or_timeout = tokio::time::timeout(
                Duration::from_secs(1),
                get_trie(
                    client,
                    &url,
                    GetTrieParams {
                        trie_key: state_root_hash,
                    },
                ),
            )
            .await;
            match req_or_timeout {
                Ok(Ok(_)) => Some(*peer),
                err => {
                    trace!("problem contacting peer {} {:?}", peer, err);
                    None
                }
            }
        };
        peer_futures.push(peer_future);
    }
    let tested_peers: Vec<SocketAddr> = futures::future::join_all(peer_futures)
        .await
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    tested_peers
}

// worker pool related
async fn sync_trie_store(
    engine_state: Arc<EngineState<DbGlobalState>>,
    root_hashes: Vec<Digest>,
    client: Client,
    peers: &[SocketAddr],
    max_parallel_trie_fetches: usize,
) -> Result<(), anyhow::Error> {
    // Counter for calculating bytes per second transfer rate.
    let bps_counter = Arc::new(Mutex::new((Instant::now(), 0usize)));

    // Channel for a worker thread to send an error.
    let (err_tx, mut err_rx) = mpsc::channel(1);

    let queue = Arc::new(WorkQueue::default());
    for root_hash in root_hashes {
        queue.push_job(root_hash);
    }
    let max_parallel_trie_fetches = max_parallel_trie_fetches.min(peers.len());
    info!(
        "using {}/{} valid peers",
        max_parallel_trie_fetches,
        peers.len()
    );
    let workers: FuturesUnordered<_> = peers[0..max_parallel_trie_fetches]
        .iter()
        .enumerate()
        .map(|(worker_id, address)| {
            info!("spawning worker {} for {:?}", worker_id, address);
            tokio::spawn(sync_trie_store_worker(
                worker_id,
                err_tx.clone(),
                queue.clone(),
                engine_state.clone(),
                client.clone(),
                *address,
                Arc::clone(&bps_counter),
            ))
        })
        .collect();
    drop(err_tx);
    workers.for_each(|_| async move {}).await;
    if let Some(err) = err_rx.recv().await {
        return Err(err); // At least one download failed: return the error.
    }

    Ok(())
}

/// A worker task that takes trie keys from a queue and downloads the trie.
async fn sync_trie_store_worker(
    worker_id: usize,
    err_tx: mpsc::Sender<anyhow::Error>,
    queue: Arc<WorkQueue<Digest>>,
    engine_state: Arc<EngineState<DbGlobalState>>,
    client: Client,
    address: SocketAddr,
    bps_counter: Arc<Mutex<(Instant, usize)>>,
) {
    while let Some(job) = queue.next_job().await {
        if err_tx.capacity() < 1 {
            return; // Another task failed and sent an error.
        }
        // Calculate global (all workers) total of trie bytes decoded per second.
        {
            let (last_update, total_bytes_since_update) = &mut *bps_counter.lock().await;
            let elapsed = last_update.elapsed().as_millis();
            if elapsed >= 1000 {
                let scaled = *total_bytes_since_update as f32 * (elapsed as f32 / 1000f32);
                info!("throughput rate: {} kilobytes per second", scaled / 1024f32);
                *total_bytes_since_update = 0;
                *last_update = Instant::now();
            }
        }
        trace!(worker_id, trie_key = %job.inner(), "worker downloading trie");
        match fetch_and_store_trie(
            engine_state.clone(),
            &client,
            address,
            *job.inner(),
            bps_counter.clone(),
        )
        .await
        {
            Ok(Some(child_jobs)) => {
                for child_job in child_jobs {
                    queue.push_job(child_job);
                }
            }
            Ok(None) => {
                warn!(
                    "got no bytes back for trie, requeuing job and killing worker {}",
                    worker_id
                );
                queue.push_job(*job.inner());
                return;
            }
            Err(err) => {
                match err_tx.try_send(err) {
                    Err(mpsc::error::TrySendError::Full(err)) => {
                        error!(?err, "could not send error; another task also failed");
                    }
                    // Should be unreachable since we own one of the receivers.
                    Err(mpsc::error::TrySendError::Closed(err)) => {
                        error!(?err, "mpsc channel closed unexpectedly");
                    }
                    Ok(()) => {}
                }
                return;
            }
        }
    }
}

// fetch a trie, store it, and if we didn't get any bytes then return None
async fn fetch_and_store_trie(
    engine_state: Arc<EngineState<DbGlobalState>>,
    client: &Client,
    address: SocketAddr,
    trie_key: Digest,
    bps_counter: Arc<Mutex<(Instant, usize)>>,
) -> Result<Option<Vec<Digest>>, anyhow::Error> {
    let url = address_to_url(address);
    let maybe_trie = get_trie(client, &url, GetTrieParams { trie_key }).await;
    let missing_descendants = match maybe_trie {
        Ok(GetTrieResult {
            maybe_trie_bytes: Some(blob),
            ..
        }) => {
            let bytes: Vec<u8> = blob.into();

            // total -trie bytes- per second
            {
                let (_, total_bytes_since_last_update) = &mut *bps_counter.lock().await;
                *total_bytes_since_last_update += bytes.len();
            }

            // similar to how the contract-runtime does related operations, spawn in a blocking task
            tokio::task::spawn_blocking(move || {
                let (trie, _rest) = FromBytes::from_bytes(&bytes)?;

                engine_state
                    .put_trie_and_find_missing_descendant_trie_keys(CorrelationId::new(), &trie)
            })
            .await??
        }
        Ok(GetTrieResult {
            maybe_trie_bytes: None,
            ..
        }) => {
            warn!(
                "got None for trie at key {:?} in peer {:?}, will retry with another peer",
                trie_key, address
            );
            return Ok(None);
        }
        Err(err) => {
            return Err(anyhow::anyhow!("error in get_trie {:?}", err));
        }
    };
    Ok(Some(missing_descendants))
}

pub fn address_to_url(address: SocketAddr) -> String {
    format!("http://{}:{}/rpc", address.ip(), address.port())
}
