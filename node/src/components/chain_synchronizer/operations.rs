use std::{
    cmp,
    collections::{BTreeMap, VecDeque},
    mem,
    sync::{
        atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
        Arc, RwLock,
    },
};

use async_trait::async_trait;
use datasize::DataSize;
use futures::{
    stream::{futures_unordered::FuturesUnordered, StreamExt},
    TryStreamExt,
};
use num::rational::Ratio;
use prometheus::IntGauge;
use quanta::Instant;
use serde::Serialize;
use tokio::sync::Semaphore;
use tracing::{debug, error, info, trace, warn};

use casper_execution_engine::storage::trie::{TrieOrChunk, TrieOrChunkId};
use casper_hashing::Digest;
use casper_types::{bytesrepr::Bytes, EraId, PublicKey, TimeDiff, Timestamp, U512};

use crate::{
    components::{
        chain_synchronizer::{
            error::{Error, FetchBlockHeadersBatchError, FetchTrieError},
            Config, Metrics, ProgressHolder,
        },
        consensus::{self, error::FinalitySignatureError},
        contract_runtime::{BlockAndExecutionEffects, ExecutionPreState},
        fetcher::{FetchResult, FetchedData, FetcherError},
    },
    effect::{
        announcements::{
            BlocklistAnnouncement, ChainSynchronizerAnnouncement, ControlAnnouncement,
        },
        requests::{
            ContractRuntimeRequest, FetcherRequest, MarkBlockCompletedRequest, NetworkInfoRequest,
        },
        EffectBuilder,
    },
    storage::StorageRequest,
    types::{
        AvailableBlockRange, Block, BlockAndDeploys, BlockHash, BlockHeader,
        BlockHeaderWithMetadata, BlockHeadersBatch, BlockHeadersBatchId, BlockSignatures,
        BlockWithMetadata, Deploy, DeployHash, FinalizedApprovals, FinalizedApprovalsWithId,
        FinalizedBlock, Item, NodeId,
    },
    utils::work_queue::WorkQueue,
};

const FINALITY_SIGNATURE_FETCH_RETRY_COUNT: usize = 3;
const MAX_HEADERS_BATCH_SIZE: u64 = 1024;

/// Helper struct that is used to measure a time spent in the scope.
/// At the construction time, a reference to the gauge is provided. When the binding to `ScopeTimer`
/// is dropped, the specified gauge is updated with the duration since the scope was entered.
// In the future, this could be replaced with a mechanism that uses `tracing::instrument` attribute
// macro.
struct ScopeTimer<'a> {
    start: Instant,
    gauge: &'a IntGauge,
}

impl<'a> ScopeTimer<'a> {
    fn new(gauge: &'a IntGauge) -> Self {
        Self {
            start: Instant::now(),
            gauge,
        }
    }
}

impl<'a> Drop for ScopeTimer<'a> {
    fn drop(&mut self) {
        self.gauge
            .set(Instant::now().duration_since(self.start).as_secs() as i64);
    }
}

struct ChainSyncContext<'a, REv>
where
    REv: 'static,
{
    effect_builder: &'a EffectBuilder<REv>,
    config: &'a Config,
    trusted_block_header: Option<Arc<BlockHeader>>,
    metrics: &'a Metrics,
    progress: &'a ProgressHolder,
    /// A list of peers which should be asked for data in the near future.
    bad_peer_list: RwLock<VecDeque<NodeId>>,
    /// Number of times peer lists have been filtered.
    filter_count: AtomicI64,
    /// A range of blocks for which we already have all required data stored locally.
    locally_available_block_range_on_start: AvailableBlockRange,
    trie_fetch_limit: Semaphore,
}

impl<'a, REv> ChainSyncContext<'a, REv>
where
    REv: From<StorageRequest> + From<FetcherRequest<BlockHeader>> + From<NetworkInfoRequest>,
{
    /// Returns a new instance suitable for use in the fast-sync task.  It uses the trusted hash
    /// from `config` or else the highest block header in storage as the trust anchor.
    ///
    /// Returns `None` if there is no trusted hash specified in `config` and if storage has no
    /// blocks.  Otherwise returns a new `ChainSyncContext`.
    async fn new_for_fast_sync(
        effect_builder: &'a EffectBuilder<REv>,
        config: &'a Config,
        metrics: &'a Metrics,
        progress: &'a ProgressHolder,
    ) -> Result<Option<ChainSyncContext<'a, REv>>, Error> {
        debug_assert!(progress.is_fast_sync());
        let locally_available_block_range_on_start = effect_builder
            .get_available_block_range_from_storage()
            .await;

        let mut ctx = Self {
            effect_builder,
            config,
            trusted_block_header: None,
            metrics,
            progress,
            bad_peer_list: RwLock::new(VecDeque::new()),
            filter_count: AtomicI64::new(0),
            locally_available_block_range_on_start,
            trie_fetch_limit: Semaphore::new(config.max_parallel_trie_fetches()),
        };

        let trusted_block_header = match config.trusted_hash() {
            Some(trusted_hash) => {
                *fetch_and_store_initial_trusted_block_header(&ctx, metrics, trusted_hash).await?
            }
            None => match effect_builder.get_highest_block_header_from_storage().await {
                Some(block_header) => block_header,
                None => {
                    debug!("no highest block header found in storage");
                    return Ok(None);
                }
            },
        };

        if trusted_block_header.protocol_version() != config.protocol_version()
            && !config.is_last_block_before_activation(&trusted_block_header)
        {
            return Err(Error::TrustedHeaderTooEarly {
                trusted_header: Box::new(trusted_block_header),
                current_protocol_version: config.protocol_version(),
                activation_point: config.chainspec().protocol_config.activation_point.era_id(),
            });
        }

        ctx.trusted_block_header = Some(Arc::new(trusted_block_header));

        Ok(Some(ctx))
    }
}

impl<'a, REv> ChainSyncContext<'a, REv>
where
    REv: From<StorageRequest>,
{
    /// Returns a new instance suitable for use in the sync-to-genesis task.  It uses the highest
    /// block from storage's available block range as the trust anchor, as that block should be the
    /// one returned by the fast-sync task run previously, and used in participating mode to begin
    /// executing forwards from.
    async fn new_for_sync_to_genesis(
        effect_builder: &'a EffectBuilder<REv>,
        config: &'a Config,
        metrics: &'a Metrics,
        progress: &'a ProgressHolder,
    ) -> Result<ChainSyncContext<'a, REv>, Error> {
        debug_assert!(progress.is_sync_to_genesis());
        let locally_available_block_range_on_start = effect_builder
            .get_available_block_range_from_storage()
            .await;
        let highest_available_block_height = locally_available_block_range_on_start.high();

        let mut ctx = Self {
            effect_builder,
            config,
            trusted_block_header: None,
            metrics,
            progress,
            bad_peer_list: RwLock::new(VecDeque::new()),
            filter_count: AtomicI64::new(0),
            locally_available_block_range_on_start,
            trie_fetch_limit: Semaphore::new(config.max_parallel_trie_fetches()),
        };

        let trusted_block_header = match effect_builder
            .get_block_header_at_height_from_storage(highest_available_block_height, true)
            .await
        {
            Some(block_header) => block_header,
            None => {
                error!(
                    %highest_available_block_height,
                    "block header should be available in storage"
                );
                return Err(Error::NoHighestBlockHeader);
            }
        };

        ctx.trusted_block_header = Some(Arc::new(trusted_block_header));

        Ok(ctx)
    }
}

impl<'a, REv> ChainSyncContext<'a, REv> {
    /// Returns the trusted block header.
    ///
    /// # Panics
    ///
    /// Will panic if not initialized properly using `ChainSyncContext::new`.
    fn trusted_block_header(&self) -> &BlockHeader {
        self.trusted_block_header
            .as_ref()
            .expect("trusted block header not initialized")
    }

    /// Removes known bad peers from a given peer list.
    ///
    /// Automatically redeems the oldest bad peer after `redemption_interval` filterings.
    fn filter_bad_peers(&self, peers: &mut Vec<NodeId>) {
        {
            let bad_peer_list = self
                .bad_peer_list
                .read()
                .expect("bad peer list lock poisoned");
            // Note: This is currently quadratic in the amount of peers (e.g. in a network with 400
            // out of 401 bad peers, this would result in 160k comparisons), but we estimate that
            // this is fine given the expected low number of bad peers for now. If this proves a
            // problem, proper sets should be used instead.
            //
            // Using a vec is currently convenient because it allows for FIFO-ordered redemption.
            peers.retain(|p| !bad_peer_list.contains(p));
        }

        let redemption_interval = self.config.redemption_interval as i64;
        if redemption_interval != 0
            && self.filter_count.fetch_add(1, Ordering::Relaxed) > redemption_interval
        {
            self.filter_count
                .fetch_sub(redemption_interval, Ordering::Relaxed);

            // Redeem the oldest bad node.
            self.bad_peer_list
                .write()
                .expect("bad peer list lock poisoned")
                .pop_front();
        }
    }

    /// Marks a peer as bad.
    fn mark_bad_peer(&self, peer: NodeId) {
        if self.config.redemption_interval == 0 {
            debug!(%peer, "not marking peer as bad for syncing, redemption is disabled");
            return;
        }

        let mut bad_peer_list = self
            .bad_peer_list
            .write()
            .expect("bad peer list lock poisoned");

        // Note: Like `filter_bad_peers`, this may need to be migrated to use sets instead.
        if bad_peer_list.contains(&peer) {
            debug!(%peer, "peer already marked as bad for syncing");
        } else {
            bad_peer_list.push_back(peer);
            info!(%peer, "marked peer as bad for syncing");
        }
    }

    /// Clears the list of bad peers.
    fn redeem_all(&self) {
        let mut bad_peer_list = self
            .bad_peer_list
            .write()
            .expect("bad peer list lock poisoned");

        let bad_peer_list = mem::take::<VecDeque<NodeId>>(&mut bad_peer_list);
        if !bad_peer_list.is_empty() {
            warn!(
                bad_peer_count = bad_peer_list.len(),
                "redeemed all bad peers"
            )
        }
    }
}

/// Restrict the fan-out for a trie being retrieved by chunks to query at most 10 peers at a time.
const TRIE_CHUNK_FETCH_FAN_OUT: usize = 10;

/// Allows us to decide whether syncing peers can also be used when calling `fetch_retry_forever`.
trait CanUseSyncingNodes {
    fn can_use_syncing_nodes() -> bool {
        true
    }
}

/// Tries and trie chunks can only be retrieved from non-syncing peers to avoid syncing nodes
/// deadlocking while requesting these from each other.
impl CanUseSyncingNodes for TrieOrChunk {
    fn can_use_syncing_nodes() -> bool {
        false
    }
}

/// All other `Item` types can safely be retrieved from syncing peers, as there is no networking
/// backpressure implemented for these fetch requests.
impl CanUseSyncingNodes for BlockHeader {}
impl CanUseSyncingNodes for Block {}
impl CanUseSyncingNodes for Deploy {}
impl CanUseSyncingNodes for BlockAndDeploys {}
impl CanUseSyncingNodes for BlockHeadersBatch {}

/// Returns fully-connected, non-syncing peers that are known to be not banned.
async fn get_filtered_fully_connected_non_syncing_peers<REv>(
    ctx: &ChainSyncContext<'_, REv>,
) -> Vec<NodeId>
where
    REv: From<NetworkInfoRequest>,
{
    let mut peer_list = ctx
        .effect_builder
        .get_fully_connected_non_syncing_peers()
        .await;
    ctx.filter_bad_peers(&mut peer_list);
    peer_list
}

/// Returns fully-connected, syncing and non-syncing peers that are known to be not banned.
async fn get_filtered_fully_connected_peers<REv>(ctx: &ChainSyncContext<'_, REv>) -> Vec<NodeId>
where
    REv: From<NetworkInfoRequest>,
{
    let mut peer_list = ctx.effect_builder.get_fully_connected_peers().await;
    ctx.filter_bad_peers(&mut peer_list);
    peer_list
}

/// Fetches an item. Keeps retrying to fetch until it is successful. Not suited to fetching a block
/// header or block by height, which require verification with finality signatures.
async fn fetch_retry_forever<REv, T>(ctx: &ChainSyncContext<'_, REv>, id: T::Id) -> FetchResult<T>
where
    T: Item + CanUseSyncingNodes + 'static,
    REv: From<FetcherRequest<T>> + From<NetworkInfoRequest>,
{
    let mut attempts = 0_usize;
    loop {
        let new_peer_list = if T::can_use_syncing_nodes() {
            get_filtered_fully_connected_peers(ctx).await
        } else {
            get_filtered_fully_connected_non_syncing_peers(ctx).await
        };

        if new_peer_list.is_empty() && attempts % 100 == 0 {
            warn!(
                attempts,
                item_type = ?T::TAG,
                ?id,
                can_use_syncing_nodes = %T::can_use_syncing_nodes(),
                "failed to attempt to fetch item due to no fully-connected peers"
            );
        }

        for peer in new_peer_list {
            trace!(
                "attempting to fetch {:?} with id {:?} from {:?}",
                T::TAG,
                id,
                peer
            );
            match ctx.effect_builder.fetch::<T>(id, peer).await {
                Ok(fetched_data @ FetchedData::FromStorage { .. }) => {
                    trace!(
                        "did not get {:?} with id {:?} from {:?}, got from storage instead",
                        T::TAG,
                        id,
                        peer
                    );
                    return Ok(fetched_data);
                }
                Ok(fetched_data @ FetchedData::FromPeer { .. }) => {
                    trace!("fetched {:?} with id {:?} from {:?}", T::TAG, id, peer);
                    return Ok(fetched_data);
                }
                Err(FetcherError::Absent { .. }) => {
                    warn!(
                        ?id,
                        tag = ?T::TAG,
                        ?peer,
                        "chain sync could not fetch; trying next peer",
                    );
                    ctx.mark_bad_peer(peer);
                }
                Err(FetcherError::TimedOut { .. }) => {
                    warn!(
                        ?id,
                        tag = ?T::TAG,
                        ?peer,
                        "peer timed out",
                    );
                    ctx.mark_bad_peer(peer);
                }
                Err(error @ FetcherError::CouldNotConstructGetRequest { .. }) => return Err(error),
            }
        }
        tokio::time::sleep(ctx.config.retry_interval()).await;
        attempts += 1;
    }
}

enum TrieAlreadyPresentOrDownloaded {
    AlreadyPresent,
    Downloaded(Bytes),
}

async fn fetch_trie_retry_forever<REv>(
    id: Digest,
    ctx: &ChainSyncContext<'_, REv>,
) -> Result<TrieAlreadyPresentOrDownloaded, FetchTrieError>
where
    REv: From<FetcherRequest<TrieOrChunk>> + From<NetworkInfoRequest>,
{
    let trie_or_chunk =
        match fetch_retry_forever::<_, TrieOrChunk>(ctx, TrieOrChunkId(0, id)).await? {
            FetchedData::FromStorage { .. } => {
                return Ok(TrieAlreadyPresentOrDownloaded::AlreadyPresent)
            }
            FetchedData::FromPeer {
                item: trie_or_chunk,
                ..
            } => *trie_or_chunk,
        };
    let chunk_with_proof = match trie_or_chunk {
        TrieOrChunk::Trie(trie) => return Ok(TrieAlreadyPresentOrDownloaded::Downloaded(trie)),
        TrieOrChunk::ChunkWithProof(chunk_with_proof) => chunk_with_proof,
    };

    debug_assert!(
        chunk_with_proof.proof().index() == 0,
        "Proof index was not 0.  Proof index: {}",
        chunk_with_proof.proof().index()
    );
    let count = chunk_with_proof.proof().count();
    let first_chunk = chunk_with_proof.into_chunk();
    // Start stream iter to get each chunk.
    // Start from 1 because proof.index() == 0.
    // Build a map of the chunks.
    let chunk_map_result = futures::stream::iter(1..count)
        .map(|index| async move {
            match fetch_retry_forever::<_, TrieOrChunk>(ctx, TrieOrChunkId(index, id)).await? {
                FetchedData::FromStorage { .. } => {
                    Err(FetchTrieError::TrieBeingFetchByChunksSomehowFetchedFromStorage)
                }
                FetchedData::FromPeer { item, .. } => match *item {
                    TrieOrChunk::Trie(_) => Err(
                        FetchTrieError::TrieBeingFetchedByChunksSomehowFetchWholeFromPeer {
                            digest: id,
                        },
                    ),
                    TrieOrChunk::ChunkWithProof(chunk_with_proof) => {
                        let index = chunk_with_proof.proof().index();
                        let chunk = chunk_with_proof.into_chunk();
                        Ok((index, chunk))
                    }
                },
            }
        })
        // Do not try to fetch all of the trie chunks at once; only fetch at most
        // TRIE_CHUNK_FETCH_FAN_OUT at a time.
        // Doing `buffer_unordered` followed by `try_collect` here means if one of the
        // fetches fails, then the outstanding futures are canceled.
        .buffer_unordered(TRIE_CHUNK_FETCH_FAN_OUT)
        .try_collect::<BTreeMap<u64, Bytes>>()
        .await;

    // Handle if a parallel process downloaded all the trie chunks before us.
    let mut chunk_map = match chunk_map_result {
        Ok(chunk_map) => chunk_map,
        Err(FetchTrieError::TrieBeingFetchByChunksSomehowFetchedFromStorage) => {
            // trie must have been downloaded by a parallel process...
            return Ok(TrieAlreadyPresentOrDownloaded::AlreadyPresent);
        }
        Err(error) => {
            return Err(error);
        }
    };
    chunk_map.insert(0, first_chunk);

    // Concatenate all of the chunks into a trie
    let trie_bytes = chunk_map.into_values().flat_map(Vec::<u8>::from).collect();
    Ok(TrieAlreadyPresentOrDownloaded::Downloaded(trie_bytes))
}

/// Fetches and stores a block header from the network.
async fn fetch_and_store_block_header<REv>(
    ctx: &ChainSyncContext<'_, REv>,
    block_hash: BlockHash,
) -> Result<Box<BlockHeader>, Error>
where
    REv: From<StorageRequest> + From<FetcherRequest<BlockHeader>> + From<NetworkInfoRequest>,
{
    // Only genesis should have this as previous hash, so no block should ever have it...
    if block_hash == BlockHash::default() {
        return Err(Error::NoSuchBlockHash {
            bogus_block_hash: block_hash,
        });
    }

    // We're querying storage directly and short-circuiting here (before using the fetcher)
    // as joiners don't talk to joiners and in a network comprised only of joining nodes
    // we would never move past the initial sync since we would wait on fetcher
    // trying to query a peer for block but have no peers to query for the data.
    if let Some(stored_block_header) = ctx
        .effect_builder
        .get_block_header_from_storage(block_hash, false)
        .await
    {
        return Ok(Box::new(stored_block_header));
    }

    let fetched_block_header = fetch_retry_forever::<_, BlockHeader>(ctx, block_hash).await?;
    match fetched_block_header {
        FetchedData::FromStorage { item: block_header } => Ok(block_header),
        FetchedData::FromPeer {
            item: block_header, ..
        } => {
            ctx.effect_builder
                .put_block_header_to_storage(block_header.clone())
                .await;
            Ok(block_header)
        }
    }
}

/// Fetches and stores a deploy.
async fn fetch_and_store_deploy<REv>(
    deploy_or_transfer_hash: DeployHash,
    ctx: &ChainSyncContext<'_, REv>,
) -> Result<Box<Deploy>, FetcherError<Deploy>>
where
    REv: From<StorageRequest> + From<FetcherRequest<Deploy>> + From<NetworkInfoRequest>,
{
    // We're querying storage directly and short-circuiting here (before using the fetcher)
    // as joiners don't talk to joiners and in a network comprised only of joining nodes
    // we would never move past the initial sync since we would wait on fetcher
    // trying to query a peer for a deploy but have no peers to query for the data.
    if let Some((stored_deploy, _)) = ctx
        .effect_builder
        .get_deploy_and_metadata_from_storage(deploy_or_transfer_hash)
        .await
    {
        return Ok(Box::new(stored_deploy.discard_finalized_approvals()));
    }

    let fetched_deploy = fetch_retry_forever::<_, Deploy>(ctx, deploy_or_transfer_hash).await?;
    Ok(match fetched_deploy {
        FetchedData::FromStorage { item: deploy } => deploy,
        FetchedData::FromPeer { item: deploy, .. } => {
            ctx.effect_builder
                .put_deploy_to_storage(deploy.clone())
                .await;
            deploy
        }
    })
}

/// Fetches finalized approvals for a deploy.
/// Note: this function doesn't store the approvals. They are intended to be stored after
/// confirming that the execution results match the received block.
async fn fetch_finalized_approvals<REv>(
    deploy_hash: DeployHash,
    peer: NodeId,
    ctx: &ChainSyncContext<'_, REv>,
) -> Result<FinalizedApprovalsWithId, FetcherError<FinalizedApprovalsWithId>>
where
    REv: From<FetcherRequest<FinalizedApprovalsWithId>>,
{
    let fetched_approvals = ctx
        .effect_builder
        .fetch::<FinalizedApprovalsWithId>(deploy_hash, peer)
        .await?;
    match fetched_approvals {
        FetchedData::FromStorage { item: approvals } => Ok(*approvals),
        FetchedData::FromPeer {
            item: approvals, ..
        } => Ok(*approvals),
    }
}

/// Key block info used for verifying finality signatures.
///
/// Can come from either:
///   - A switch block
///   - The global state under a genesis block
///
/// If the data was scraped from genesis, then `era_id` is 0.
/// Otherwise if it came from a switch block it is that switch block's `era_id + 1`.
#[derive(DataSize, Clone, Serialize, Debug)]
pub(crate) struct KeyBlockInfo {
    /// The block hash of the key block
    key_block_hash: BlockHash,
    /// The validator weights for checking finality signatures.
    validator_weights: BTreeMap<PublicKey, U512>,
    /// The time the era started.
    era_start: Timestamp,
    /// The height of the switch block that started this era.
    height: u64,
    /// The era in which the validators are operating
    era_id: EraId,
}

impl KeyBlockInfo {
    pub(crate) fn maybe_from_block_header(block_header: &BlockHeader) -> Option<KeyBlockInfo> {
        block_header
            .next_era_validator_weights()
            .and_then(|next_era_validator_weights| {
                Some(KeyBlockInfo {
                    key_block_hash: block_header.hash(),
                    validator_weights: next_era_validator_weights.clone(),
                    era_start: block_header.timestamp(),
                    height: block_header.height(),
                    era_id: block_header.era_id().checked_add(1)?,
                })
            })
    }

    /// Returns the era in which the validators are operating
    pub(crate) fn era_id(&self) -> EraId {
        self.era_id
    }

    /// Returns the hash of the key block, i.e. the last block before `era_id`.
    pub(crate) fn block_hash(&self) -> &BlockHash {
        &self.key_block_hash
    }

    /// Returns the validator weights for this era.
    pub(crate) fn validator_weights(&self) -> &BTreeMap<PublicKey, U512> {
        &self.validator_weights
    }
}

#[async_trait]
trait BlockOrHeaderWithMetadata: Item<Id = u64> + 'static {
    fn header(&self) -> &BlockHeader;

    fn finality_signatures(&self) -> &BlockSignatures;

    async fn store_block_or_header<REv>(&self, effect_builder: EffectBuilder<REv>)
    where
        REv: From<StorageRequest> + Send;
}

#[async_trait]
impl BlockOrHeaderWithMetadata for BlockWithMetadata {
    fn header(&self) -> &BlockHeader {
        self.block.header()
    }

    fn finality_signatures(&self) -> &BlockSignatures {
        &self.finality_signatures
    }

    async fn store_block_or_header<REv>(&self, effect_builder: EffectBuilder<REv>)
    where
        REv: From<StorageRequest> + Send,
    {
        let block = Box::new(self.block.clone());
        effect_builder.put_block_to_storage(block).await;
    }
}

#[async_trait]
impl BlockOrHeaderWithMetadata for BlockHeaderWithMetadata {
    fn header(&self) -> &BlockHeader {
        &self.block_header
    }

    fn finality_signatures(&self) -> &BlockSignatures {
        &self.block_signatures
    }

    async fn store_block_or_header<REv>(&self, effect_builder: EffectBuilder<REv>)
    where
        REv: From<StorageRequest> + Send,
    {
        let header = Box::new(self.block_header.clone());
        effect_builder.put_block_header_to_storage(header).await;
    }
}

/// Fetches the next block or block header from the network by height.
///
/// If the number of fully connected peers is less than the minimum threshold, we retry forever.
///
/// Each retry operation is invoked after the configured retry interval.
async fn fetch_and_store_next<REv, I>(
    parent_header: &BlockHeader,
    key_block_info: &KeyBlockInfo,
    ctx: &ChainSyncContext<'_, REv>,
) -> Result<Option<Box<I>>, Error>
where
    I: BlockOrHeaderWithMetadata,
    REv: From<FetcherRequest<I>>
        + From<NetworkInfoRequest>
        + From<BlocklistAnnouncement>
        + From<StorageRequest>
        + Send,
    Error: From<FetcherError<I>>,
{
    let height = parent_header
        .height()
        .checked_add(1)
        .ok_or_else(|| Error::HeightOverflow {
            parent: Box::new(parent_header.clone()),
        })?;

    loop {
        let peers = prepare_peers_applicable_for_block_fetch(ctx).await;
        let peer_count = peers.len();
        let maybe_item = try_fetch_block_or_block_header_by_height(
            peers,
            ctx,
            height,
            parent_header,
            key_block_info,
        )
        .await?;
        match maybe_item {
            Some(item) => {
                if item.header().protocol_version() < parent_header.protocol_version() {
                    return Err(Error::LowerVersionThanParent {
                        parent: Box::new(parent_header.clone()),
                        child: Box::new(item.header().clone()),
                    });
                }

                if item.header().protocol_version() > ctx.config.protocol_version() {
                    return Err(Error::RetrievedBlockHeaderFromFutureVersion {
                        current_version: ctx.config.protocol_version(),
                        block_header_with_future_version: Box::new(item.header().clone()),
                    });
                }

                return Ok(Some(item));
            }
            None => {
                if peer_count
                    >= ctx
                        .config
                        .minimum_peer_count_threshold_for_block_fetch_retry
                {
                    warn!(
                        %height,
                        attempts_to_get_fully_connected_peers =
                            %ctx.config.max_retries_while_not_connected(),
                        "unable to fetch item despite having enough peers, giving up"
                    );
                    return Ok(None);
                }
                info!(
                    %height,
                    %peer_count,
                    minimum_peer_count_threshold =
                        %ctx.config.minimum_peer_count_threshold_for_block_fetch_retry,
                    "tried fetching with not enough peers, trying again"
                );
                tokio::time::sleep(ctx.config.retry_interval()).await;
            }
        }
    }
}

/// Fetches the next block or block header from the network by height.
/// Returns `Ok(None)` if there are no more peers left.
async fn try_fetch_block_or_block_header_by_height<REv, I>(
    mut peers: Vec<NodeId>,
    ctx: &ChainSyncContext<'_, REv>,
    height: u64,
    parent_header: &BlockHeader,
    key_block_info: &KeyBlockInfo,
) -> Result<Option<Box<I>>, Error>
where
    I: BlockOrHeaderWithMetadata,
    REv: From<FetcherRequest<I>> + From<BlocklistAnnouncement> + From<StorageRequest> + Send,
    Error: From<FetcherError<I>>,
{
    Ok(loop {
        let peer = match peers.pop() {
            Some(peer) => peer,
            None => return Ok(None),
        };
        match ctx.effect_builder.fetch::<I>(height, peer).await {
            Ok(FetchedData::FromStorage { item }) => {
                if *item.header().parent_hash() != parent_header.hash() {
                    return Err(Error::UnexpectedParentHash {
                        parent: Box::new(parent_header.clone()),
                        child: Box::new(item.header().clone()),
                    });
                }
                break Some(item);
            }
            Ok(FetchedData::FromPeer { item, .. }) => {
                if *item.header().parent_hash() != parent_header.hash() {
                    warn!(
                        ?peer,
                        fetched_header = ?item.header(),
                        ?parent_header,
                        "received block with wrong parent from peer",
                    );
                    ctx.effect_builder.announce_disconnect_from_peer(peer).await;
                    continue;
                }

                if key_block_info.era_id() != item.header().era_id() {
                    error!(
                        key_block_info_era_id = key_block_info.era_id().value(),
                        item_header_era_id = item.header().era_id().value(),
                        "mismatch between key block era id and item header era id"
                    );
                }

                if let Err(error) = consensus::check_sufficient_finality_signatures(
                    key_block_info.validator_weights(),
                    ctx.config.finality_threshold_fraction(),
                    Some(item.finality_signatures()),
                ) {
                    warn!(?error, ?peer, "insufficient finality signatures from peer");
                    ctx.effect_builder.announce_disconnect_from_peer(peer).await;
                    continue;
                }

                if let Err(error) = item.finality_signatures().verify() {
                    warn!(
                        ?error,
                        ?peer,
                        "error validating finality signatures from peer"
                    );
                    ctx.effect_builder.announce_disconnect_from_peer(peer).await;
                    continue;
                }

                // Store the block or header itself, and the finality signatures.
                item.store_block_or_header(*ctx.effect_builder).await;
                let sigs = item.finality_signatures().clone();
                ctx.effect_builder.put_signatures_to_storage(sigs).await;

                break Some(item);
            }
            Err(FetcherError::Absent { .. }) => {
                warn!(height, tag = ?I::TAG, ?peer, "block by height absent from peer");
                // If the peer we requested doesn't have the item, continue with the next peer
                continue;
            }
            Err(FetcherError::TimedOut { .. }) => {
                warn!(height, tag = ?I::TAG, ?peer, "peer timed out");
                // Peer timed out fetching the item, continue with the next peer
                continue;
            }
            Err(error) => return Err(error.into()),
        }
    })
}

/// Prepares a list of peers applicable for the next fetch operation.
async fn prepare_peers_applicable_for_block_fetch<REv>(
    ctx: &ChainSyncContext<'_, REv>,
) -> Vec<NodeId>
where
    REv: From<NetworkInfoRequest>,
{
    let mut peers = vec![];
    for _ in 0..ctx.config.max_retries_while_not_connected() {
        peers = get_filtered_fully_connected_peers(ctx).await;
        if !peers.is_empty() {
            break;
        }

        // We have no peers left, might as well redeem all the bad ones.
        ctx.redeem_all();
        tokio::time::sleep(ctx.config.retry_interval()).await;
    }
    peers
}

/// Queries all of the peers for a trie, puts the trie found from the network in the trie-store, and
/// returns any outstanding descendant tries.
async fn fetch_and_store_trie<REv>(
    trie_key: Digest,
    ctx: &ChainSyncContext<'_, REv>,
) -> Result<Vec<Digest>, Error>
where
    REv:
        From<FetcherRequest<TrieOrChunk>> + From<NetworkInfoRequest> + From<ContractRuntimeRequest>,
{
    let fetched_trie = fetch_trie_retry_forever(trie_key, ctx).await?;
    match fetched_trie {
        TrieAlreadyPresentOrDownloaded::AlreadyPresent => Ok(ctx
            .effect_builder
            .find_missing_descendant_trie_keys(trie_key)
            .await?),
        TrieAlreadyPresentOrDownloaded::Downloaded(trie_bytes) => Ok(ctx
            .effect_builder
            .put_trie_and_find_missing_descendant_trie_keys(trie_bytes)
            .await?),
    }
}

/// Downloads and stores a block.
async fn fetch_and_store_block_by_hash<REv>(
    block_hash: BlockHash,
    ctx: &ChainSyncContext<'_, REv>,
) -> Result<Box<Block>, FetcherError<Block>>
where
    REv: From<StorageRequest> + From<FetcherRequest<Block>> + From<NetworkInfoRequest>,
{
    let fetched_block = fetch_retry_forever::<_, Block>(ctx, block_hash).await?;
    match fetched_block {
        FetchedData::FromStorage { item: block, .. } => Ok(block),
        FetchedData::FromPeer { item: block, .. } => {
            ctx.effect_builder.put_block_to_storage(block.clone()).await;
            Ok(block)
        }
    }
}

/// Downloads and stores a block with all its deploys.
async fn fetch_and_store_block_with_deploys_by_hash<REv>(
    block_hash: BlockHash,
    ctx: &ChainSyncContext<'_, REv>,
) -> Result<Box<BlockAndDeploys>, FetcherError<BlockAndDeploys>>
where
    REv: From<NetworkInfoRequest> + From<FetcherRequest<BlockAndDeploys>> + From<StorageRequest>,
{
    let start = Timestamp::now();
    let fetched_block = fetch_retry_forever::<_, BlockAndDeploys>(ctx, block_hash).await?;
    let res = match fetched_block {
        FetchedData::FromStorage {
            item: block_and_deploys,
            ..
        } => Ok(block_and_deploys),
        FetchedData::FromPeer {
            item: block_and_deploys,
            ..
        } => {
            ctx.effect_builder
                .put_block_and_deploys_to_storage(block_and_deploys.clone())
                .await;
            Ok(block_and_deploys)
        }
    };
    ctx.metrics
        .observe_fetch_block_and_deploys_duration_seconds(start);
    res
}

/// A worker task that takes trie keys from a queue and downloads the trie.
async fn sync_trie_store_worker<REv>(
    worker_id: usize,
    block_height: u64,
    abort: Arc<AtomicBool>,
    queue: Arc<WorkQueue<Digest>>,
    ctx: &ChainSyncContext<'_, REv>,
) -> Result<(), Error>
where
    REv:
        From<FetcherRequest<TrieOrChunk>> + From<NetworkInfoRequest> + From<ContractRuntimeRequest>,
{
    while let Some(job) = queue.next_job().await {
        let permit = match ctx.trie_fetch_limit.acquire().await {
            Ok(permit) => permit,
            Err(error) => {
                error!(?worker_id, ?job, "semaphore closed unexpectedly");
                // We can't fetch any more tries after this.
                drop(job);
                return Err(Error::SemaphoreError(error));
            }
        };
        trace!(worker_id, trie_key = %job.inner(), "worker downloading trie");
        let child_jobs = fetch_and_store_trie(*job.inner(), ctx)
            .await
            .map_err(|err| {
                abort.store(true, Ordering::Relaxed);
                warn!(?err, trie_key = %job.inner(), "failed to download trie");
                err
            })?;
        trace!(?child_jobs, trie_key = %job.inner(), "downloaded trie node");
        if abort.load(Ordering::Relaxed) {
            return Ok(()); // Another task failed and sent an error.
        }
        for child_job in child_jobs {
            queue.push_job(child_job);
        }
        ctx.progress
            .set_num_tries_to_fetch(block_height, queue.num_jobs());
        drop(job); // Make sure the job gets dropped only when the children are in the queue.
        drop(permit); // Drop permit to allow other workers to acquire it.
    }
    Ok(())
}

/// Synchronizes the trie store under a given state root hash.
async fn sync_trie_store<REv>(
    block_header: &BlockHeader,
    ctx: &ChainSyncContext<'_, REv>,
) -> Result<(), Error>
where
    REv:
        From<FetcherRequest<TrieOrChunk>> + From<NetworkInfoRequest> + From<ContractRuntimeRequest>,
{
    let block_height = block_header.height();
    debug_assert!(ctx.progress.is_fetching_tries(block_height));
    let state_root_hash = *block_header.state_root_hash();
    trace!(?state_root_hash, "syncing trie store");

    if ctx
        .locally_available_block_range_on_start
        .contains(block_height)
    {
        info!(
            locally_available_block_range_on_start = %ctx.locally_available_block_range_on_start,
            block_height,
            "skipping trie store sync because it is already available locally"
        );
        return Ok(());
    }

    let start_instant = Timestamp::now();

    // We're querying storage directly and short-circuiting here (before using the fetcher)
    // as joiners don't talk to joiners and in a network comprised only of joining nodes
    // we would never move past the initial sync since we would wait on fetcher
    // trying to query a peer for a trie but have no peers to query for the data.
    if ctx
        .effect_builder
        .get_trie_full(state_root_hash)
        .await?
        .is_some()
        && ctx
            .effect_builder
            .find_missing_descendant_trie_keys(state_root_hash)
            .await?
            .is_empty()
    {
        ctx.metrics
            .observe_sync_trie_store_duration_seconds(start_instant);
        return Ok(());
    }

    // Flag set by a worker when it encounters an error.
    let abort = Arc::new(AtomicBool::new(false));

    let queue = Arc::new(WorkQueue::default());
    queue.push_job(state_root_hash);
    ctx.progress.set_num_tries_to_fetch(block_height, 1);

    let mut workers: FuturesUnordered<_> = (0..ctx.config.max_parallel_trie_fetches())
        .map(|worker_id| {
            sync_trie_store_worker(worker_id, block_height, abort.clone(), queue.clone(), ctx)
        })
        .collect();
    while let Some(result) = workers.next().await {
        result?; // Return the error if a download failed.
    }

    ctx.metrics
        .observe_sync_trie_store_duration_seconds(start_instant);
    Ok(())
}

/// Fetches the current header and fast-syncs to it.
///
/// Performs the following:
///
///  1. Starting at the trusted block, fetches block headers by iterating forwards towards tip until
///     getting to one from the current era or failing to get a higher one from any peer.
///  2. Starting at that highest synced block, iterates backwards towards genesis, fetching
///     `deploy_max_ttl`'s worth of blocks (for deploy replay protection).
///  3. Starting at the same highest synced block, again iterates backwards, fetching enough block
///     headers to allow consensus to be initialized.
///  4. Fetches the tries under the same highest synced block's state root hash (parallelized
///     tasks).
///
/// Returns the highest synced block header and the corresponding highest synced key block info.
async fn fast_sync<REv>(
    ctx: &ChainSyncContext<'_, REv>,
) -> Result<(BlockHeader, KeyBlockInfo), Error>
where
    REv: From<FetcherRequest<TrieOrChunk>>
        + From<NetworkInfoRequest>
        + From<ContractRuntimeRequest>
        + From<StorageRequest>
        + From<BlocklistAnnouncement>
        + From<FetcherRequest<Block>>
        + From<FetcherRequest<BlockHeader>>
        + From<FetcherRequest<BlockHeaderWithMetadata>>
        + Send,
{
    let _metric = ScopeTimer::new(&ctx.metrics.chain_sync_fast_sync_total_duration_seconds);

    let trusted_key_block_info = get_trusted_key_block_info(ctx).await?;
    let (highest_synced_block_header, highest_synced_key_block_info) =
        fetch_block_headers_up_to_current_era(&trusted_key_block_info, ctx).await?;

    fetch_blocks_for_deploy_replay_protection(
        &highest_synced_block_header,
        &highest_synced_key_block_info,
        ctx,
    )
    .await?;

    fetch_block_headers_needed_for_era_supervisor_initialization(&highest_synced_block_header, ctx)
        .await?;

    // Synchronize the trie store for the highest synced block header.
    ctx.progress.start_fetching_tries_for_fast_sync(
        highest_synced_block_header.height(),
        *highest_synced_block_header.state_root_hash(),
    );
    sync_trie_store(&highest_synced_block_header, ctx).await?;

    Ok((highest_synced_block_header, highest_synced_key_block_info))
}

/// Gets the trusted key block info for a trusted block header.
///
/// Fetches block headers back towards genesis from the trusted hash until we get to a switch block.
/// If the trusted hash _is_ from a switch block, the trusted key block is the same block.
async fn get_trusted_key_block_info<REv>(
    ctx: &ChainSyncContext<'_, REv>,
) -> Result<KeyBlockInfo, Error>
where
    REv: From<StorageRequest> + From<NetworkInfoRequest> + From<FetcherRequest<BlockHeader>>,
{
    let _metric = ScopeTimer::new(
        &ctx.metrics
            .chain_sync_get_trusted_key_block_info_duration_seconds,
    );

    // Fetch each parent hash one by one until we have the switch block info.
    let mut current_header_to_walk_back_from = ctx.trusted_block_header().clone();
    loop {
        if let Some(key_block_info) =
            KeyBlockInfo::maybe_from_block_header(&current_header_to_walk_back_from)
        {
            break Ok(key_block_info);
        }

        if current_header_to_walk_back_from.height() == 0 {
            break Err(Error::HitGenesisBlockTryingToGetTrustedEraValidators {
                trusted_header: ctx.trusted_block_header().clone(),
            });
        }

        current_header_to_walk_back_from =
            *fetch_and_store_block_header(ctx, *current_header_to_walk_back_from.parent_hash())
                .await?;
    }
}

/// Get the highest header which has the same version as ours.
///
/// We keep fetching by height until none of our peers have a block at that height and we are in
/// the current era.
async fn fetch_block_headers_up_to_current_era<REv>(
    trusted_key_block_info: &KeyBlockInfo,
    ctx: &ChainSyncContext<'_, REv>,
) -> Result<(BlockHeader, KeyBlockInfo), Error>
where
    REv: From<FetcherRequest<BlockHeaderWithMetadata>>
        + From<NetworkInfoRequest>
        + From<BlocklistAnnouncement>
        + From<StorageRequest>
        + Send,
{
    let _metric = ScopeTimer::new(&ctx.metrics.chain_sync_fetch_block_headers_duration_seconds);

    let mut highest_synced_block_header = ctx.trusted_block_header().clone();
    let mut highest_synced_key_block_info = trusted_key_block_info.clone();
    loop {
        // If we synced up to the current era, we can consider syncing done.
        if is_current_era(
            &highest_synced_block_header,
            &highest_synced_key_block_info,
            ctx.config,
        ) {
            info!(
                era = highest_synced_block_header.era_id().value(),
                height = highest_synced_block_header.height(),
                timestamp = %highest_synced_block_header.timestamp(),
                "fetched block headers up to the current era",
            );
            break;
        }

        let maybe_fetched_block = fetch_and_store_next::<_, BlockHeaderWithMetadata>(
            &highest_synced_block_header,
            &highest_synced_key_block_info,
            ctx,
        )
        .await?;

        if let Some(higher_block_header_with_metadata) = maybe_fetched_block {
            highest_synced_block_header = higher_block_header_with_metadata.block_header;

            // If the new block is a switch block, update the validator weights, etc...
            if let Some(key_block_info) =
                KeyBlockInfo::maybe_from_block_header(&highest_synced_block_header)
            {
                highest_synced_key_block_info = key_block_info;
            }
        } else {
            // If we timed out, consider syncing done.  The only way we should reach this code
            // branch is if the network just underwent an emergency upgrade, where there was an
            // outage long enough that the highest block of the network now causes `is_current_era`
            // to return `false`.
            info!(
                era = highest_synced_block_header.era_id().value(),
                height = highest_synced_block_header.height(),
                timestamp = %highest_synced_block_header.timestamp(),
                "failed to fetch a higher block header",
            );
            break;
        }
    }
    Ok((highest_synced_block_header, highest_synced_key_block_info))
}

/// Fetch and store all blocks that can contain not-yet-expired deploys. These are needed for
/// replay detection.
async fn fetch_blocks_for_deploy_replay_protection<REv>(
    highest_synced_block_header: &BlockHeader,
    highest_synced_key_block_info: &KeyBlockInfo,
    ctx: &ChainSyncContext<'_, REv>,
) -> Result<(), Error>
where
    REv: From<StorageRequest> + From<FetcherRequest<Block>> + From<NetworkInfoRequest>,
{
    let _metric = ScopeTimer::new(&ctx.metrics.chain_sync_replay_protection_duration_seconds);

    let mut current_header = highest_synced_block_header.clone();
    while highest_synced_key_block_info
        .era_start
        .saturating_diff(current_header.timestamp())
        < ctx.config.deploy_max_ttl()
        && current_header.height() != 0
    {
        current_header = fetch_and_store_block_by_hash(*current_header.parent_hash(), ctx)
            .await?
            .take_header();
    }
    Ok(())
}

/// The era supervisor requires enough switch blocks to be stored in the database to be able to
/// initialize the most recent eras.
async fn fetch_block_headers_needed_for_era_supervisor_initialization<REv>(
    highest_synced_block_header: &BlockHeader,
    ctx: &ChainSyncContext<'_, REv>,
) -> Result<(), Error>
where
    REv: From<StorageRequest> + From<FetcherRequest<BlockHeader>> + From<NetworkInfoRequest>,
{
    let _metric = ScopeTimer::new(&ctx.metrics.chain_sync_era_supervisor_init_duration_seconds);

    let earliest_open_era = ctx
        .config
        .earliest_open_era(highest_synced_block_header.era_id());
    let earliest_era_needed_by_era_supervisor =
        ctx.config.earliest_switch_block_needed(earliest_open_era);
    let mut current_walk_back_header = highest_synced_block_header.clone();
    while current_walk_back_header.era_id() > earliest_era_needed_by_era_supervisor {
        current_walk_back_header =
            *fetch_and_store_block_header(ctx, *current_walk_back_header.parent_hash()).await?;
    }
    Ok(())
}

/// Runs the sync-to-genesis task.
///
/// Performs the following:
///
///  1. Sets the trusted block as the highest available block, as this will be the one returned by
///     the fast-sync task run previously.
///  2. Starting at the trusted block and iterating in batches of `MAX_HEADERS_BATCH_SIZE` at a
///     time, fetches block headers all the way back to genesis (not a parallelized task).
///  3. Starting at genesis and iterating forwards towards tip until we reach the trusted block, in
///     parallelized tasks:
///    a. Fetches the full block and its deploys
///    b. Fetches the tries under that block's state root hash (further parallelized tasks).
///    c. Fetches the block signatures.
pub(super) async fn run_sync_to_genesis_task<REv>(
    effect_builder: EffectBuilder<REv>,
    config: Config,
    metrics: Metrics,
    progress: ProgressHolder,
) -> Result<(), Error>
where
    REv: From<StorageRequest>
        + From<NetworkInfoRequest>
        + From<FetcherRequest<TrieOrChunk>>
        + From<FetcherRequest<BlockAndDeploys>>
        + From<FetcherRequest<BlockSignatures>>
        + From<FetcherRequest<BlockHeadersBatch>>
        + From<ContractRuntimeRequest>
        + From<BlocklistAnnouncement>
        + From<MarkBlockCompletedRequest>
        + From<ChainSynchronizerAnnouncement>,
{
    info!("starting chain sync to genesis");
    let _metric = ScopeTimer::new(&metrics.chain_sync_to_genesis_total_duration_seconds);
    progress.start();
    let ctx =
        ChainSyncContext::new_for_sync_to_genesis(&effect_builder, &config, &metrics, &progress)
            .await?;
    fetch_headers_till_genesis(&ctx).await?;
    fetch_blocks_and_state_and_finality_signatures_since_genesis(&ctx).await?;
    effect_builder.announce_finished_chain_syncing().await;
    ctx.progress.finish();
    info!("finished chain sync to genesis");
    Ok(())
}

/// Fetches block headers in batches starting from `trusted_block` till the Genesis.
async fn fetch_headers_till_genesis<REv>(ctx: &ChainSyncContext<'_, REv>) -> Result<(), Error>
where
    REv: From<FetcherRequest<BlockHeadersBatch>>
        + From<NetworkInfoRequest>
        + From<StorageRequest>
        + From<BlocklistAnnouncement>,
{
    let _metric = ScopeTimer::new(&ctx.metrics.chain_sync_fetch_to_genesis_duration_seconds);
    info!(
        trusted_block_height = %ctx.trusted_block_header().height(),
        locally_available_block_range_on_start = %ctx.locally_available_block_range_on_start,
        "starting to fetch headers to genesis",
    );

    let mut lowest_trusted_block_header = ctx.trusted_block_header().clone();

    loop {
        ctx.progress
            .set_fetching_headers_back_to_genesis(lowest_trusted_block_header.height());
        match fetch_block_headers_batch(&lowest_trusted_block_header, ctx).await {
            Ok(new_lowest) => {
                if new_lowest.height() % 1_000 == 0 {
                    info!(?new_lowest, "new lowest trusted block header stored");
                }
                lowest_trusted_block_header = new_lowest;
            }
            Err(err) => {
                // If we get an error here it means something must have gone really wrong.
                // We either get the data from storage or from a peer where we retry ad infinitum if
                // peer times out or item is absent. The only reason we would end up
                // here is if fetcher couldn't construct a fetch request.
                error!(
                    ?err,
                    "failed to download block headers batch with infinite retries"
                );
                return Err(err.into());
            }
        }

        if lowest_trusted_block_header.height() == 0 {
            break;
        }
    }
    Ok(())
}

/// Fetches a batch of block headers, validates and stores in storage.
///
/// Returns either an error or lowest valid block in the chain.
async fn fetch_block_headers_batch<REv>(
    lowest_trusted_block_header: &BlockHeader,
    ctx: &ChainSyncContext<'_, REv>,
) -> Result<BlockHeader, FetchBlockHeadersBatchError>
where
    REv: From<FetcherRequest<BlockHeadersBatch>>
        + From<NetworkInfoRequest>
        + From<StorageRequest>
        + From<BlocklistAnnouncement>,
{
    let batch_id =
        BlockHeadersBatchId::from_known(lowest_trusted_block_header, MAX_HEADERS_BATCH_SIZE);

    loop {
        let fetched_headers_data: FetchedData<BlockHeadersBatch> =
            fetch_retry_forever::<_, BlockHeadersBatch>(ctx, batch_id).await?;
        match fetched_headers_data {
            FetchedData::FromStorage { item } => {
                return item
                    .lowest()
                    .cloned()
                    .ok_or(FetchBlockHeadersBatchError::EmptyBatchFromStorage)
            }
            FetchedData::FromPeer { item, peer } => {
                match BlockHeadersBatch::validate(&*item, &batch_id, lowest_trusted_block_header) {
                    Ok(new_lowest) => {
                        info!(?batch_id, ?peer, "received valid batch of headers");
                        ctx.effect_builder
                            .put_block_headers_batch_to_storage(item.into_inner())
                            .await;
                        return Ok(new_lowest);
                    }
                    Err(err) => {
                        error!(
                            ?err,
                            ?peer,
                            ?batch_id,
                            "block headers batch failed validation. Trying next peer..."
                        );
                        ctx.effect_builder.announce_disconnect_from_peer(peer).await;
                    }
                }
            }
        }
    }
}

/// Fetches blocks, their transactions and tries from Genesis until `trusted_block`.
async fn fetch_blocks_and_state_and_finality_signatures_since_genesis<REv>(
    ctx: &ChainSyncContext<'_, REv>,
) -> Result<(), Error>
where
    REv: From<StorageRequest>
        + From<NetworkInfoRequest>
        + From<FetcherRequest<TrieOrChunk>>
        + From<FetcherRequest<BlockAndDeploys>>
        + From<FetcherRequest<BlockSignatures>>
        + From<NetworkInfoRequest>
        + From<ContractRuntimeRequest>
        + From<BlocklistAnnouncement>
        + From<MarkBlockCompletedRequest>,
{
    let _metric = ScopeTimer::new(&ctx.metrics.chain_sync_fetch_forward_duration_seconds);
    info!("syncing blocks and deploys and state since Genesis");

    // NOTE: Currently there's no good way to read the known, highest *FULL* block.
    // Full means that we have:
    //  * the block header
    //  * the block body
    //  * the trie
    // Since we fetch and store each of these independently, we might crash midway through the
    // process and have partial data. We could reindex storages on startup but that had proven to
    // take 30min+ in the past. We need to prioritize correctness over performance so we
    // choose to "re-sync" from Genesis, even if it means we will go through thousands of blocks
    // that we already have. Hopefully, local checks will be fast enough.
    let latest_height_requested: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));

    let mut workers: FuturesUnordered<_> = (0..ctx.config.max_parallel_block_fetches())
        .map(|worker_id| fetch_block_worker(worker_id, latest_height_requested.clone(), ctx))
        .collect();

    while let Some(result) = workers.next().await {
        result?; // Return the error if a download failed.
    }

    Ok(())
}

async fn fetch_block_worker<REv>(
    worker_id: usize,
    latest_height_requested: Arc<AtomicU64>,
    ctx: &ChainSyncContext<'_, REv>,
) -> Result<(), Error>
where
    REv: From<StorageRequest>
        + From<NetworkInfoRequest>
        + From<FetcherRequest<TrieOrChunk>>
        + From<FetcherRequest<BlockAndDeploys>>
        + From<FetcherRequest<BlockSignatures>>
        + From<NetworkInfoRequest>
        + From<ContractRuntimeRequest>
        + From<BlocklistAnnouncement>
        + From<MarkBlockCompletedRequest>,
{
    let trusted_block_height = ctx.trusted_block_header().height();
    loop {
        let block_height = latest_height_requested.fetch_add(1, Ordering::SeqCst);
        if block_height >= trusted_block_height {
            info!(%worker_id, ?block_height, ?trusted_block_height, "fetch-block worker finished");
            return Ok(());
        }

        let block_header = ctx
            .effect_builder
            .get_block_header_at_height_from_storage(block_height, false)
            .await
            .ok_or(Error::NoSuchBlockHeight(block_height))?;

        let block_hash = block_header.hash();

        if block_height % 1_000 == 0 {
            info!(
                worker_id,
                ?block_hash,
                ?block_height,
                "syncing block and deploys"
            );
        }
        ctx.progress
            .start_syncing_block_for_sync_forward(block_height);
        match fetch_and_store_block_with_deploys_by_hash(block_hash, ctx).await {
            Ok(fetched_block) => {
                debug_assert_eq!(block_header, *fetched_block.block.header());
                trace!(?block_hash, "downloaded block and deploys");
                // We want to download the trie only when we know we have the block.
                ctx.progress.start_fetching_tries_for_sync_forward(
                    block_height,
                    *block_header.state_root_hash(),
                );
                if let Err(error) = sync_trie_store(&block_header, ctx).await {
                    error!(
                        ?error,
                        ?block_hash,
                        "failed to download trie with infinite retries"
                    );
                    return Err(error);
                }
                ctx.effect_builder.mark_block_completed(block_height).await;
                ctx.metrics.chain_sync_blocks_synced.inc();
            }
            Err(err) => {
                // We're using `fetch_retry_forever` internally so we should never get
                // an error other than `FetcherError::CouldNotConstructGetRequest` that we don't
                // want to retry.
                error!(
                    ?err,
                    ?block_hash,
                    "failed to download block with infinite retries"
                );
            }
        }

        ctx.progress
            .start_fetching_block_signatures_for_sync_forward(block_height);
        fetch_and_store_finality_signatures_by_block_header(block_header, ctx).await?;
        ctx.progress
            .finish_syncing_block_for_sync_forward(block_height);
    }
}

#[derive(Clone, Debug)]
enum HandleSignaturesResult {
    ContinueFetching,
    HaveSufficient,
}

struct BlockSignaturesCollector(Option<BlockSignatures>);

impl BlockSignaturesCollector {
    fn new() -> Self {
        Self(None)
    }

    fn add(&mut self, signatures: BlockSignatures) {
        match &mut self.0 {
            None => {
                self.0 = Some(signatures);
            }
            Some(old_sigs) => {
                for (pub_key, signature) in signatures.proofs {
                    old_sigs.insert_proof(pub_key, signature);
                }
            }
        }
    }

    fn check_if_sufficient(
        &self,
        validator_weights: &BTreeMap<PublicKey, U512>,
        finality_threshold_fraction: Ratio<u64>,
    ) -> Result<(), FinalitySignatureError> {
        are_signatures_sufficient_for_sync_to_genesis(
            consensus::check_sufficient_finality_signatures(
                validator_weights,
                finality_threshold_fraction,
                self.0.as_ref(),
            ),
        )
    }

    fn check_if_sufficient_for_sync_to_genesis(
        &self,
        validator_weights: &BTreeMap<PublicKey, U512>,
        finality_threshold_fraction: Ratio<u64>,
    ) -> Result<(), FinalitySignatureError> {
        are_signatures_sufficient_for_sync_to_genesis(
            consensus::check_sufficient_finality_signatures_with_quorum_formula(
                validator_weights,
                finality_threshold_fraction,
                self.0.as_ref(),
                std::convert::identity,
            ),
        )
    }

    fn into_inner(self) -> Option<BlockSignatures> {
        self.0
    }

    /// Handles the new signatures fetched from peer.
    /// The signatures are validated. In case they don't pass validation, the peer is disconnected
    /// and the `ContinueFetching` is returned. Valid signatures are added to the buffer and then
    /// checked for sufficiency. If they are sufficient `HaveSufficient` is returned, otherwise
    /// `ContinueFetching` is returned.
    async fn handle_incoming_signatures<REv>(
        &mut self,
        block_header: &BlockHeader,
        signatures: BlockSignatures,
        peer: NodeId,
        ctx: &ChainSyncContext<'_, REv>,
    ) -> Result<HandleSignaturesResult, Error>
    where
        REv: From<StorageRequest> + From<BlocklistAnnouncement>,
    {
        if signatures.proofs.is_empty() {
            return Ok(HandleSignaturesResult::ContinueFetching);
        }

        let (era_for_validators_retrieval, validator_weights) =
            era_validator_weights_for_block(block_header, ctx).await?;

        if let Err(err) = consensus::validate_finality_signatures(&signatures, &validator_weights) {
            warn!(
                ?peer,
                ?err,
                height = block_header.height(),
                "peer sent invalid finality signatures, banning peer"
            );
            ctx.effect_builder.announce_disconnect_from_peer(peer).await;

            // Try with next peer.
            return Ok(HandleSignaturesResult::ContinueFetching);
        }
        self.add(signatures);

        if self
            .check_if_sufficient(&validator_weights, ctx.config.finality_threshold_fraction())
            .is_ok()
        {
            debug!(
                block_header_hash =
                    ?block_header.hash(),
                height = block_header.height(),
                ?era_for_validators_retrieval,
                "fetched sufficient finality signatures"
            );
            Ok(HandleSignaturesResult::HaveSufficient)
        } else {
            Ok(HandleSignaturesResult::ContinueFetching)
        }
    }
}

/// Reads the validator weights that should be used to check the finality signatures for the given
/// block.
async fn era_validator_weights_for_block<'a, REv>(
    block_header: &BlockHeader,
    ctx: &ChainSyncContext<'_, REv>,
) -> Result<(EraId, BTreeMap<PublicKey, U512>), Error>
where
    REv: From<StorageRequest>,
{
    let era_for_validators_retrieval = get_era_id_for_validators_retrieval(&block_header.era_id());
    let switch_block_of_previous_era = ctx
        .effect_builder
        .get_switch_block_header_at_era_id_from_storage(era_for_validators_retrieval)
        .await
        .ok_or(Error::NoSwitchBlockForEra {
            era_id: era_for_validators_retrieval,
        })?;
    let validator_weights = switch_block_of_previous_era
        .next_era_validator_weights()
        .ok_or(Error::MissingNextEraValidators {
            height: switch_block_of_previous_era.height(),
            era_id: era_for_validators_retrieval,
        })?;
    Ok((era_for_validators_retrieval, validator_weights.clone()))
}

// Fetches the finality signatures from the given peer. In case of timeout, it'll
// retry up to `retries` times. Other errors interrupt the process immediately.
async fn fetch_finality_signatures_with_retry<REv>(
    block_hash: BlockHash,
    peer: NodeId,
    retries: usize,
    ctx: &ChainSyncContext<'_, REv>,
) -> Result<FetchedData<BlockSignatures>, FetcherError<BlockSignatures>>
where
    REv: From<FetcherRequest<BlockSignatures>>,
{
    for _ in 0..retries {
        let maybe_signatures = ctx
            .effect_builder
            .fetch::<BlockSignatures>(block_hash, peer)
            .await;
        match maybe_signatures {
            Ok(result) => return Ok(result),
            Err(FetcherError::TimedOut { .. }) => continue,
            Err(_) => return maybe_signatures,
        }
    }

    Err(FetcherError::TimedOut {
        id: block_hash,
        peer,
    })
}

async fn fetch_and_store_finality_signatures_by_block_header<REv>(
    block_header: BlockHeader,
    ctx: &ChainSyncContext<'_, REv>,
) -> Result<(), Error>
where
    REv: From<StorageRequest>
        + From<NetworkInfoRequest>
        + From<FetcherRequest<BlockSignatures>>
        + From<BlocklistAnnouncement>,
{
    let start = Timestamp::now();
    let peer_list = get_filtered_fully_connected_peers(ctx).await;

    let mut sig_collector = BlockSignaturesCollector::new();

    let block_header_hash = block_header.hash();

    for peer in peer_list {
        let fetched_signatures = fetch_finality_signatures_with_retry(
            block_header_hash,
            peer,
            FINALITY_SIGNATURE_FETCH_RETRY_COUNT,
            ctx,
        )
        .await;

        let signatures = match fetched_signatures {
            Ok(FetchedData::FromStorage { .. }) => {
                trace!(
                    ?block_header_hash,
                    ?peer,
                    "fetched FinalitySignatures from storage",
                );
                // We're guaranteed that if the signatures are in our local storage, they fulfill
                // the sufficiency requirement for the default quorum fraction.
                return Ok(());
            }
            Ok(FetchedData::FromPeer { item, .. }) => {
                trace!(
                    ?block_header_hash,
                    ?peer,
                    "fetched FinalitySignatures from peer"
                );
                *item
            }
            Err(err) => {
                trace!(
                    ?block_header_hash,
                    ?peer,
                    ?err,
                    "error fetching FinalitySignatures"
                );
                continue;
            }
        };

        if let HandleSignaturesResult::HaveSufficient = sig_collector
            .handle_incoming_signatures(&block_header, signatures, peer, ctx)
            .await?
        {
            // Sufficient signatures were fetched from peers, store them and finish fetching for
            // this block.
            finalize_finality_signature_fetch(ctx, start, sig_collector).await;
            return Ok(());
        };
    }

    // We run out of peers, but still don't have sufficient finality signatures according to the
    // default quorum fraction. However, in the "sync to genesis" process, we can consider
    // finality signatures as valid when their total weight is at least
    // `finality_threshold_fraction` of the total validator weights.
    let (_, validator_weights) = era_validator_weights_for_block(&block_header, ctx).await?;
    sig_collector.check_if_sufficient_for_sync_to_genesis(
        &validator_weights,
        ctx.config.finality_threshold_fraction(),
    )?;
    info!(
        height = block_header.height(),
        "block below default finality signatures threshold, but enough for sync to genesis"
    );
    finalize_finality_signature_fetch(ctx, start, sig_collector).await;
    Ok(())
}

/// Returns true if the output from consensus can be interpreted as "sufficient finality signatures
/// for the sync to genesis process".
///
/// We need to make sure that for "sync_to_genesis" the `TooManySignatures` error should be
/// interpreted as Ok, so we're not hit by the anti-spam mechanism (i.e.: a mechanism that protects
/// against peers that send too many finality signatures during normal chain operation).
fn are_signatures_sufficient_for_sync_to_genesis(
    result: Result<(), FinalitySignatureError>,
) -> Result<(), FinalitySignatureError> {
    match result {
        Err(err) if !matches!(err, FinalitySignatureError::TooManySignatures { .. }) => Err(err),
        Err(_) | Ok(_) => Ok(()),
    }
}

/// Puts the signatures to storage and updates the fetch metric.
async fn finalize_finality_signature_fetch<REv>(
    ctx: &ChainSyncContext<'_, REv>,
    start: Timestamp,
    sig_collector: BlockSignaturesCollector,
) where
    REv: From<StorageRequest>,
{
    ctx.metrics
        .observe_fetch_finality_signatures_duration_seconds(start);
    if let Some(finality_signatures) = sig_collector.into_inner() {
        let block_hash = finality_signatures.block_hash;
        ctx.effect_builder
            .put_signatures_to_storage(finality_signatures)
            .await;
        trace!(?block_hash, "stored FinalitySignatures");
    }
}

/// Returns the EraId whose switch block should be used to obtain validator weights.
fn get_era_id_for_validators_retrieval(era_id: &EraId) -> EraId {
    // TODO: This function needs to handle upgrades with changes to the validator set.
    era_id.saturating_sub(1)
}

/// Runs the initial chain synchronization task ("fast sync").
pub(super) async fn run_fast_sync_task<REv>(
    effect_builder: EffectBuilder<REv>,
    config: Config,
    metrics: Metrics,
    progress: ProgressHolder,
) -> Result<BlockHeader, Error>
where
    REv: From<StorageRequest>
        + From<NetworkInfoRequest>
        + From<ContractRuntimeRequest>
        + From<FetcherRequest<Block>>
        + From<FetcherRequest<BlockHeader>>
        + From<FetcherRequest<BlockAndDeploys>>
        + From<FetcherRequest<BlockWithMetadata>>
        + From<FetcherRequest<BlockHeaderWithMetadata>>
        + From<FetcherRequest<Deploy>>
        + From<FetcherRequest<FinalizedApprovalsWithId>>
        + From<FetcherRequest<TrieOrChunk>>
        + From<BlocklistAnnouncement>
        + From<MarkBlockCompletedRequest>
        + From<ControlAnnouncement>
        + Send,
{
    info!("fast syncing chain");
    let _metric = ScopeTimer::new(&metrics.chain_sync_total_duration_seconds);
    progress.start();

    let ctx =
        match ChainSyncContext::new_for_fast_sync(&effect_builder, &config, &metrics, &progress)
            .await?
        {
            Some(ctx) => ctx,
            None => return Err(Error::NoBlocksInStorage),
        };
    verify_trusted_block_header(&ctx)?;

    // We should have at least one block header in storage now as a result of calling
    // `ChainSyncContext::new`.
    let mut highest_block_header =
        match effect_builder.get_highest_block_header_from_storage().await {
            Some(block_header) => block_header,
            None => return Err(Error::NoHighestBlockHeader),
        };

    let (highest_synced_block_header, highest_synced_key_block_info) = fast_sync(&ctx).await?;

    // Iterate forwards, fetching each full block and deploys but executing each block to generate
    // global state. Stop once we get to a block in the current era.
    let highest_synced_block_header = fetch_and_execute_blocks(
        &highest_synced_block_header,
        highest_synced_key_block_info,
        &ctx,
    )
    .await?;

    // If we just committed an emergency upgrade and are re-syncing right after this, potentially
    // the call to `fast_sync` and `execute_blocks` could yield a `highest_synced_block_header`
    // which is lower than the `highest_block_header` (which is the immediate switch block of the
    // upgrade).  Ensure we take the actual highest block to allow consensus to be initialised
    // properly.
    if highest_synced_block_header.height() > highest_block_header.height() {
        highest_block_header = highest_synced_block_header;
    }

    ctx.effect_builder
        .mark_block_completed(highest_block_header.height())
        .await;

    info!(
        era_id = ?highest_block_header.era_id(),
        height = highest_block_header.height(),
        now = %Timestamp::now(),
        block_timestamp = %highest_block_header.timestamp(),
        "finished initial chain sync",
    );

    Ok(highest_block_header)
}

async fn fetch_and_store_initial_trusted_block_header<REv>(
    ctx: &ChainSyncContext<'_, REv>,
    metrics: &Metrics,
    trusted_hash: BlockHash,
) -> Result<Box<BlockHeader>, Error>
where
    REv: From<StorageRequest> + From<FetcherRequest<BlockHeader>> + From<NetworkInfoRequest>,
{
    let _metric = ScopeTimer::new(
        &metrics.chain_sync_fetch_and_store_initial_trusted_block_header_duration_seconds,
    );
    ctx.progress
        .start_fetching_trusted_block_header(trusted_hash);
    let trusted_block_header = fetch_and_store_block_header(ctx, trusted_hash).await?;
    Ok(trusted_block_header)
}

fn verify_trusted_block_header<REv>(ctx: &ChainSyncContext<'_, REv>) -> Result<(), Error> {
    if ctx.trusted_block_header().protocol_version() > ctx.config.protocol_version() {
        return Err(Error::RetrievedBlockHeaderFromFutureVersion {
            current_version: ctx.config.protocol_version(),
            block_header_with_future_version: Box::new(ctx.trusted_block_header().clone()),
        });
    }

    let era_duration: TimeDiff = cmp::max(
        ctx.config.min_round_length() * ctx.config.min_era_height(),
        ctx.config.era_duration(),
    );

    if ctx.trusted_block_header().timestamp()
        + era_duration
            * ctx
                .config
                .unbonding_delay()
                .saturating_sub(ctx.config.auction_delay())
        < Timestamp::now()
    {
        warn!(
            ?ctx.trusted_block_header,
            "timestamp of trusted hash is older than \
             era_duration * (unbonding_delay - auction_delay)"
        );
    };

    Ok(())
}

async fn retry_execution_with_approvals_from_peer<REv>(
    deploys: &mut [Deploy],
    transfers: &mut [Deploy],
    peer: NodeId,
    block: &Block,
    execution_pre_state: &ExecutionPreState,
    ctx: &ChainSyncContext<'_, REv>,
) -> Result<BlockAndExecutionEffects, Error>
where
    REv: From<FetcherRequest<FinalizedApprovalsWithId>> + From<ContractRuntimeRequest>,
{
    for deploy in deploys.iter_mut().chain(transfers.iter_mut()) {
        let new_approvals = fetch_finalized_approvals(*deploy.id(), peer, ctx).await?;
        deploy.replace_approvals(new_approvals.into_inner());
    }
    Ok(ctx
        .effect_builder
        .execute_finalized_block(
            block.protocol_version(),
            execution_pre_state.clone(),
            FinalizedBlock::from(block.clone()),
            deploys.to_owned(),
            transfers.to_owned(),
        )
        .await?)
}

/// Executes forwards from the block after `highest_synced_block_header` until we can get no higher
/// block from any peer, or the block we executed is in the current era.
async fn fetch_and_execute_blocks<REv>(
    highest_synced_block_header: &BlockHeader,
    highest_synced_key_block_info: KeyBlockInfo,
    ctx: &ChainSyncContext<'_, REv>,
) -> Result<BlockHeader, Error>
where
    REv: From<FetcherRequest<BlockWithMetadata>>
        + From<FetcherRequest<FinalizedApprovalsWithId>>
        + From<FetcherRequest<Deploy>>
        + From<NetworkInfoRequest>
        + From<ContractRuntimeRequest>
        + From<BlocklistAnnouncement>
        + From<StorageRequest>
        + From<MarkBlockCompletedRequest>
        + From<ControlAnnouncement>
        + Send,
{
    let _metric = ScopeTimer::new(&ctx.metrics.chain_sync_execute_blocks_duration_seconds);

    // Execute blocks to get to current.
    let mut execution_pre_state = ExecutionPreState::from_block_header(highest_synced_block_header);
    info!(
        era_id = ?highest_synced_block_header.era_id(),
        height = highest_synced_block_header.height(),
        now = %Timestamp::now(),
        block_timestamp = %highest_synced_block_header.timestamp(),
        "fetching and executing blocks to synchronize to current",
    );

    let mut highest_synced_block_header = highest_synced_block_header.clone();
    let mut key_block_info = highest_synced_key_block_info;
    loop {
        ctx.progress.start_fetching_block_and_deploys_to_execute(
            highest_synced_block_header.height().saturating_add(1),
        );
        let result = fetch_and_store_next::<_, BlockWithMetadata>(
            &highest_synced_block_header,
            &key_block_info,
            ctx,
        )
        .await?;
        let block = match result {
            None => {
                let in_current_era =
                    is_current_era(&highest_synced_block_header, &key_block_info, ctx.config);
                info!(
                    era = highest_synced_block_header.era_id().value(),
                    in_current_era,
                    height = highest_synced_block_header.height(),
                    timestamp = %highest_synced_block_header.timestamp(),
                    "couldn't download a higher block; finishing syncing",
                );
                break;
            }
            Some(block_with_metadata) => block_with_metadata.block,
        };

        let mut deploys = fetch_and_store_deploys(block.deploy_hashes().iter(), ctx).await?;

        let mut transfers = fetch_and_store_deploys(block.transfer_hashes().iter(), ctx).await?;

        info!(
            era_id = ?block.header().era_id(),
            height = block.height(),
            now = %Timestamp::now(),
            block_timestamp = %block.timestamp(),
            "executing block",
        );
        ctx.progress.start_executing_block(block.height());
        let block_and_execution_effects = ctx
            .effect_builder
            .execute_finalized_block(
                block.protocol_version(),
                execution_pre_state.clone(),
                FinalizedBlock::from(block.clone()),
                deploys.clone(),
                transfers.clone(),
            )
            .await?;

        let mut blocks_match = block == *block_and_execution_effects.block();

        let mut attempts = 0;
        while !blocks_match {
            // Could be wrong approvals - fetch new sets of approvals from a single peer and retry.
            for peer in get_filtered_fully_connected_peers(ctx).await {
                attempts += 1;
                warn!(
                    fetched_block=%block,
                    executed_block=%block_and_execution_effects.block(),
                    attempts,
                    "retrying execution due to deploy approvals mismatch"
                );
                ctx.progress.retry_executing_block(block.height(), attempts);
                let block_and_execution_effects = retry_execution_with_approvals_from_peer(
                    &mut deploys,
                    &mut transfers,
                    peer,
                    &block,
                    &execution_pre_state,
                    ctx,
                )
                .await?;
                debug!(block_hash=%block.hash(), "finish - re-executing finalized block");
                blocks_match = block == *block_and_execution_effects.block();
                if blocks_match {
                    break;
                }
                warn!(
                    %peer,
                    "block executed with approvals from this peer doesn't match the received \
                    block; blocking peer"
                );
                ctx.effect_builder.announce_disconnect_from_peer(peer).await;
            }
        }

        // Matching now - store new approval sets for the deploys.
        for deploy in deploys.into_iter().chain(transfers.into_iter()) {
            ctx.effect_builder
                .store_finalized_approvals(
                    *deploy.id(),
                    FinalizedApprovals::new(deploy.approvals().clone()),
                )
                .await;
        }
        ctx.effect_builder
            .mark_block_completed(block_and_execution_effects.block().height())
            .await;

        highest_synced_block_header = block.take_header();
        execution_pre_state = ExecutionPreState::from_block_header(&highest_synced_block_header);

        if let Some(new_key_block_info) =
            KeyBlockInfo::maybe_from_block_header(&highest_synced_block_header)
        {
            key_block_info = new_key_block_info;
        }

        // If we managed to sync up to the current era, stop - we'll have to sync the consensus
        // protocol state, anyway.
        if is_current_era(&highest_synced_block_header, &key_block_info, ctx.config) {
            info!(
                era = highest_synced_block_header.era_id().value(),
                height = highest_synced_block_header.height(),
                timestamp = %highest_synced_block_header.timestamp(),
                "synchronized up to the current era; finishing syncing",
            );
            break;
        }
    }
    Ok(highest_synced_block_header)
}

async fn fetch_and_store_deploys<REv>(
    hashes: impl Iterator<Item = &DeployHash>,
    ctx: &ChainSyncContext<'_, REv>,
) -> Result<Vec<Deploy>, Error>
where
    REv: From<StorageRequest>
        + From<FetcherRequest<Deploy>>
        + From<NetworkInfoRequest>
        + From<ControlAnnouncement>,
{
    let start_instant = Timestamp::now();

    let hashes: Vec<_> = hashes.cloned().collect();

    // We want to use `buffer_unordered` to avoid being blocked on any particularly slow fetch
    // attempts (which could happen if we used for example `stream::buffered`), but we also need to
    // ensure the fetched deploys are returned from this function in the order as specified in the
    // `hashes` iterator.  Hence we keep track of the index of each of these hashes in the `Vec` of
    // fetched deploys to allow for sorting on completion of the stream.
    let mut indexed_deploys: Vec<(usize, Deploy)> = Vec::with_capacity(hashes.len());
    let mut stream = futures::stream::iter(hashes.into_iter().enumerate())
        .map(|(index, hash)| async move { (index, fetch_and_store_deploy(hash, ctx).await) })
        .buffer_unordered(ctx.config.max_parallel_deploy_fetches());
    while let Some((index, result)) = stream.next().await {
        let deploy = result?;
        trace!("fetched {:?}", deploy);
        indexed_deploys.push((index, *deploy));
    }

    indexed_deploys.sort();
    let deploys = indexed_deploys
        .into_iter()
        .map(|(_index, deploy)| deploy)
        .collect();
    ctx.metrics
        .observe_fetch_deploys_duration_seconds(start_instant);
    Ok(deploys)
}

/// Returns `true` if `highest_synced_block` indicates that we're currently in an ongoing era.
///
/// The ongoing era is either the one in which `highest_synced_block` was created, or in the case
/// where `highest_synced_block` is a switch block, it is the era immediately following it.
pub(super) fn is_current_era(
    highest_synced_block: &BlockHeader,
    trusted_key_block_info: &KeyBlockInfo,
    config: &Config,
) -> bool {
    is_current_era_given_current_timestamp(
        highest_synced_block,
        trusted_key_block_info,
        config,
        Timestamp::now(),
    )
}

fn is_current_era_given_current_timestamp(
    highest_synced_block: &BlockHeader,
    trusted_key_block_info: &KeyBlockInfo,
    config: &Config,
    current_timestamp: Timestamp,
) -> bool {
    let KeyBlockInfo {
        era_start, height, ..
    } = trusted_key_block_info;

    // If the minimum era duration has not yet run out, the era is still current.
    if current_timestamp.saturating_diff(*era_start) < config.era_duration() {
        return true;
    }

    // Otherwise estimate the earliest possible end of this era based on how many blocks remain.
    let remaining_blocks_in_this_era = config
        .min_era_height()
        .saturating_sub(highest_synced_block.height() - *height);
    let time_since_highest_synced_block =
        current_timestamp.saturating_diff(highest_synced_block.timestamp());
    time_since_highest_synced_block < config.min_round_length() * remaining_blocks_in_this_era
}

#[cfg(test)]
mod tests {
    use std::iter;

    use casper_types::{testing::TestRng, EraId, PublicKey, SecretKey};

    use super::*;
    use crate::{
        components::consensus::EraReport,
        types::{Block, BlockPayload, Chainspec, ChainspecRawBytes, FinalizedBlock, NodeConfig},
        utils::Loadable,
        SmallNetworkConfig,
    };

    /// Creates a block for testing, with the given data, and returns its header.
    ///
    /// The other fields are filled in with defaults, since they are not used in these tests.
    fn create_block(
        timestamp: Timestamp,
        era_id: EraId,
        height: u64,
        switch_block: bool,
    ) -> BlockHeader {
        let secret_key = SecretKey::doc_example();
        let public_key = PublicKey::from(secret_key);

        let maybe_era_report = switch_block.then(|| EraReport {
            equivocators: Default::default(),
            rewards: Default::default(),
            inactive_validators: Default::default(),
        });
        let next_era_validator_weights =
            switch_block.then(|| iter::once((public_key.clone(), 100.into())).collect());

        let block_payload = BlockPayload::new(
            Default::default(), // deploy hashes
            Default::default(), // transfer hashes
            Default::default(), // accusations
            false,              // random bit
        );

        let finalized_block = FinalizedBlock::new(
            block_payload,
            maybe_era_report,
            timestamp,
            era_id,
            height,
            public_key,
        );

        Block::new(
            Default::default(), // parent block hash
            Default::default(), // parent random seed
            Default::default(), // state root hash
            finalized_block,
            next_era_validator_weights,
            Default::default(), // protocol version
        )
        .expect("failed to create block for tests")
        .take_header()
    }

    #[test]
    fn test_is_current_era() {
        let (mut chainspec, _) = <(Chainspec, ChainspecRawBytes)>::from_resources("local");

        let genesis_time = chainspec
            .protocol_config
            .activation_point
            .genesis_timestamp()
            .expect("test expects genesis timestamp in chainspec");
        let min_round_length = chainspec.highway_config.min_round_length();

        // Configure eras to have at least 10 blocks but to last at least 20 minimum-length rounds.
        let era_duration = min_round_length * 20;
        let minimum_era_height = 10;
        chainspec.core_config.era_duration = era_duration;
        chainspec.core_config.minimum_era_height = minimum_era_height;
        let config = Config::new(
            Arc::new(chainspec),
            NodeConfig::default(),
            SmallNetworkConfig::default(),
        );

        // We assume era 6 started after six minimum era durations, at block 100.
        let era6_start = genesis_time + era_duration * 6;
        let switch_block5 = create_block(era6_start, EraId::from(5), 100, true);

        let trusted_switch_block_info5 = KeyBlockInfo::maybe_from_block_header(&switch_block5)
            .expect("no switch block info for switch block");

        // If we are still within the minimum era duration the era is current, even if we have the
        // required number of blocks (115 - 100 > 10).
        let block_time = era6_start + era_duration - 10.into();
        let now = block_time + 5.into();
        let block = create_block(block_time, EraId::from(6), 115, false);
        assert!(is_current_era_given_current_timestamp(
            &block,
            &trusted_switch_block_info5,
            &config,
            now
        ));

        // If the minimum duration has passed but we we know we don't have all blocks yet, it's
        // also still current. There are still five blocks missing but only four rounds have
        // passed.
        let block_time = era6_start + era_duration * 2;
        let now = block_time + min_round_length * 4;
        let block = create_block(block_time, EraId::from(6), 105, false);
        assert!(is_current_era_given_current_timestamp(
            &block,
            &trusted_switch_block_info5,
            &config,
            now
        ));

        // If both criteria are satisfied, the era could have ended.
        let block_time = era6_start + era_duration * 2;
        let now = block_time + min_round_length * 5;
        let block = create_block(block_time, EraId::from(6), 105, false);
        assert!(!is_current_era_given_current_timestamp(
            &block,
            &trusted_switch_block_info5,
            &config,
            now
        ));
    }

    #[test]
    fn gets_correct_era_id_for_validators_retrieval() {
        assert_eq!(
            EraId::from(0),
            get_era_id_for_validators_retrieval(&EraId::from(0))
        );

        assert_eq!(
            EraId::from(0),
            get_era_id_for_validators_retrieval(&EraId::from(1))
        );

        assert_eq!(
            EraId::from(1),
            get_era_id_for_validators_retrieval(&EraId::from(2))
        );

        assert_eq!(
            EraId::from(999),
            get_era_id_for_validators_retrieval(&EraId::from(1000))
        );
    }

    #[test]
    fn validates_signatures_sufficiency_for_sync_to_genesis() {
        let consensus_verdict = Ok(());
        assert!(are_signatures_sufficient_for_sync_to_genesis(consensus_verdict).is_ok());

        let mut rng = TestRng::new();
        let consensus_verdict = Err(FinalitySignatureError::TooManySignatures {
            trusted_validator_weights: BTreeMap::new(),
            block_signatures: Box::new(BlockSignatures::new(
                BlockHash::random(&mut rng),
                EraId::from(0),
            )),
            signature_weight: Box::new(U512::from(0u16)),
            weight_minus_minimum: Box::new(U512::from(0u16)),
            total_validator_weight: Box::new(U512::from(0u16)),
            finality_threshold_fraction: Ratio::new_raw(1, 2),
        });
        assert!(are_signatures_sufficient_for_sync_to_genesis(consensus_verdict).is_ok());

        let consensus_verdict = Err(FinalitySignatureError::InsufficientWeightForFinality {
            trusted_validator_weights: BTreeMap::new(),
            block_signatures: Some(Box::new(BlockSignatures::new(
                BlockHash::random(&mut rng),
                EraId::from(0),
            ))),
            signature_weight: Some(Box::new(U512::from(0u16))),
            total_validator_weight: Box::new(U512::from(0u16)),
            finality_threshold_fraction: Ratio::new_raw(1, 2),
        });
        assert!(are_signatures_sufficient_for_sync_to_genesis(consensus_verdict).is_err());

        let consensus_verdict = Err(FinalitySignatureError::BogusValidator {
            trusted_validator_weights: BTreeMap::new(),
            block_signatures: Box::new(BlockSignatures::new(
                BlockHash::random(&mut rng),
                EraId::from(0),
            )),
            bogus_validator_public_key: Box::new(PublicKey::random_ed25519(&mut rng)),
        });
        assert!(are_signatures_sufficient_for_sync_to_genesis(consensus_verdict).is_err());
    }
}
