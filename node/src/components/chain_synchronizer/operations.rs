use std::{
    cmp,
    collections::BTreeMap,
    sync::{
        atomic::{self, AtomicBool},
        Arc,
    },
    time::Duration,
};

use async_trait::async_trait;
use datasize::DataSize;
use futures::stream::{futures_unordered::FuturesUnordered, StreamExt};
use tracing::{debug, info, trace, warn};

use casper_execution_engine::storage::trie::Trie;
use casper_hashing::Digest;
use casper_types::{EraId, Key, PublicKey, StoredValue, U512};

use crate::{
    components::{
        chain_synchronizer::error::Error,
        consensus::{check_sufficient_finality_signatures, ChainspecConsensusExt},
        contract_runtime::ExecutionPreState,
        fetcher::{FetchResult, FetchedData, FetcherError},
    },
    effect::{requests::FetcherRequest, EffectBuilder},
    reactor::joiner::JoinerEvent,
    types::{
        Block, BlockHash, BlockHeader, BlockHeaderWithMetadata, BlockSignatures, BlockWithMetadata,
        Chainspec, Deploy, DeployHash, FinalizedBlock, Item, NodeConfig, NodeId, TimeDiff,
        Timestamp,
    },
    utils::work_queue::WorkQueue,
};

const SLEEP_DURATION_SO_WE_DONT_SPAM: Duration = Duration::from_millis(100);

/// Fetches an item. Keeps retrying to fetch until it is successful. Assumes no integrity check is
/// necessary for the item. Not suited to fetching a block header or block by height, which require
/// verification with finality signatures.
async fn fetch_retry_forever<T>(
    effect_builder: EffectBuilder<JoinerEvent>,
    id: T::Id,
) -> FetchResult<T, NodeId>
where
    T: Item + 'static,
    JoinerEvent: From<FetcherRequest<NodeId, T>>,
{
    loop {
        for peer in effect_builder.get_fully_connected_peers().await {
            trace!(
                "attempting to fetch {:?} with id {:?} from {:?}",
                T::TAG,
                id,
                peer
            );
            match effect_builder.fetch::<T, NodeId>(id, peer).await {
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
                    )
                }
                Err(FetcherError::TimedOut { .. }) => {
                    warn!(
                        ?id,
                        tag = ?T::TAG,
                        ?peer,
                        "peer timed out",
                    );
                }
                Err(error @ FetcherError::CouldNotConstructGetRequest { .. }) => return Err(error),
            }
        }
        tokio::time::sleep(SLEEP_DURATION_SO_WE_DONT_SPAM).await
    }
}

async fn fetch_trie_retry_forever(
    effect_builder: EffectBuilder<JoinerEvent>,
    id: Digest,
) -> FetchedData<Trie<Key, StoredValue>, NodeId> {
    loop {
        let peers = effect_builder.get_fully_connected_peers::<NodeId>().await;
        trace!(?id, "attempting to fetch a trie",);
        match effect_builder.fetch_trie(id, peers).await {
            Ok(fetched_data) => {
                trace!(?id, "got trie successfully",);
                return fetched_data;
            }
            Err(error) => {
                warn!(?id, %error, "fast sync could not fetch a trie; trying again")
            }
        }
        tokio::time::sleep(SLEEP_DURATION_SO_WE_DONT_SPAM).await
    }
}

/// Fetches and stores a block header from the network.
async fn fetch_and_store_block_header(
    effect_builder: EffectBuilder<JoinerEvent>,
    block_hash: BlockHash,
) -> Result<Box<BlockHeader>, Error> {
    // Only genesis should have this as previous hash, so no block should ever have it...
    if block_hash == BlockHash::default() {
        return Err(Error::NoSuchBlockHash {
            bogus_block_hash: block_hash,
        });
    }
    let fetched_block_header =
        fetch_retry_forever::<BlockHeader>(effect_builder, block_hash).await?;
    match fetched_block_header {
        FetchedData::FromStorage { item: block_header } => Ok(block_header),
        FetchedData::FromPeer {
            item: block_header, ..
        } => {
            effect_builder
                .put_block_header_to_storage(block_header.clone())
                .await;
            Ok(block_header)
        }
    }
}

/// Fetches and stores a deploy.
async fn fetch_and_store_deploy(
    effect_builder: EffectBuilder<JoinerEvent>,
    deploy_or_transfer_hash: DeployHash,
) -> Result<Box<Deploy>, FetcherError<Deploy, NodeId>> {
    let fetched_deploy =
        fetch_retry_forever::<Deploy>(effect_builder, deploy_or_transfer_hash).await?;
    match fetched_deploy {
        FetchedData::FromStorage { item: deploy } => Ok(deploy),
        FetchedData::FromPeer { item: deploy, .. } => {
            effect_builder.put_deploy_to_storage(deploy.clone()).await;
            Ok(deploy)
        }
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
#[derive(DataSize, Clone, Debug)]
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
    pub(crate) fn maybe_from_block_header(
        block_header: &BlockHeader,
        verifiable_chunked_hash_activation: EraId,
    ) -> Option<KeyBlockInfo> {
        block_header
            .next_era_validator_weights()
            .and_then(|next_era_validator_weights| {
                Some(KeyBlockInfo {
                    key_block_hash: block_header.hash(verifiable_chunked_hash_activation),
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

/// Gets the trusted key block info for a trusted block header.
async fn get_trusted_key_block_info(
    effect_builder: EffectBuilder<JoinerEvent>,
    chainspec: &Chainspec,
    trusted_header: &BlockHeader,
) -> Result<KeyBlockInfo, Error> {
    // If the trusted block's version is newer than ours we return an error
    if trusted_header.protocol_version() > chainspec.protocol_config.version {
        return Err(Error::RetrievedBlockHeaderFromFutureVersion {
            current_version: chainspec.protocol_config.version,
            block_header_with_future_version: Box::new(trusted_header.clone()),
        });
    }
    // If the trusted block's version is older than ours we also return an error, except if we are
    // at the current activation point, i.e. at an upgrade.
    if trusted_header.protocol_version() < chainspec.protocol_config.version
        && trusted_header.next_block_era_id() != chainspec.protocol_config.activation_point.era_id()
    {
        return Err(Error::TrustedBlockHasOldVersion {
            current_version: chainspec.protocol_config.version,
            block_header_with_old_version: Box::new(trusted_header.clone()),
        });
    }

    // Fetch each parent hash one by one until we have the switch block info
    // This will crash if we try to get the parent hash of genesis, which is the default [0u8; 32]
    let mut current_header_to_walk_back_from = trusted_header.clone();
    loop {
        // Check that we are not restarting right after an emergency restart, which is too early
        match chainspec.protocol_config.last_emergency_restart {
            Some(last_emergency_restart)
                if last_emergency_restart > current_header_to_walk_back_from.era_id() =>
            {
                return Err(Error::TrustedHeaderEraTooEarly {
                    trusted_header: Box::new(trusted_header.clone()),
                    maybe_last_emergency_restart_era_id: chainspec
                        .protocol_config
                        .last_emergency_restart,
                })
            }
            _ => {}
        }

        if let Some(key_block_info) = KeyBlockInfo::maybe_from_block_header(
            &current_header_to_walk_back_from,
            chainspec.protocol_config.verifiable_chunked_hash_activation,
        ) {
            check_block_version(trusted_header, chainspec)?;
            break Ok(key_block_info);
        }

        if current_header_to_walk_back_from.height() == 0 {
            break Err(Error::HitGenesisBlockTryingToGetTrustedEraValidators {
                trusted_header: trusted_header.clone(),
            });
        }

        current_header_to_walk_back_from = *fetch_and_store_block_header(
            effect_builder,
            *current_header_to_walk_back_from.parent_hash(),
        )
        .await?;
    }
}

#[async_trait]
trait BlockOrHeaderWithMetadata: Item<Id = u64> + 'static {
    fn header(&self) -> &BlockHeader;

    fn finality_signatures(&self) -> &BlockSignatures;

    async fn store_block_or_header(&self, effect_builder: EffectBuilder<JoinerEvent>);
}

#[async_trait]
impl BlockOrHeaderWithMetadata for BlockWithMetadata {
    fn header(&self) -> &BlockHeader {
        self.block.header()
    }

    fn finality_signatures(&self) -> &BlockSignatures {
        &self.finality_signatures
    }

    async fn store_block_or_header(&self, effect_builder: EffectBuilder<JoinerEvent>) {
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

    async fn store_block_or_header(&self, effect_builder: EffectBuilder<JoinerEvent>) {
        let header = Box::new(self.block_header.clone());
        effect_builder.put_block_header_to_storage(header).await;
    }
}

/// Fetches the next block or block header from the network by height.
async fn fetch_and_store_next<I>(
    effect_builder: EffectBuilder<JoinerEvent>,
    parent_header: &BlockHeader,
    trusted_key_block_info: &KeyBlockInfo,
    chainspec: &Chainspec,
) -> Result<Option<Box<I>>, Error>
where
    I: BlockOrHeaderWithMetadata,
    JoinerEvent: From<FetcherRequest<NodeId, I>>,
    Error: From<FetcherError<I, NodeId>>,
{
    let height = parent_header
        .height()
        .checked_add(1)
        .ok_or_else(|| Error::HeightOverflow {
            parent: Box::new(parent_header.clone()),
        })?;
    let mut peers = effect_builder.get_fully_connected_peers().await.into_iter();
    let item = loop {
        let peer = match peers.next() {
            Some(peer) => peer,
            None => return Ok(None),
        };
        match effect_builder.fetch::<I, NodeId>(height, peer).await {
            Ok(FetchedData::FromStorage { item }) => {
                if *item.header().parent_hash()
                    != parent_header
                        .hash(chainspec.protocol_config.verifiable_chunked_hash_activation)
                {
                    return Err(Error::UnexpectedParentHash {
                        parent: Box::new(parent_header.clone()),
                        child: Box::new(item.header().clone()),
                    });
                }
                break item;
            }
            Ok(FetchedData::FromPeer { item, .. }) => {
                if *item.header().parent_hash()
                    != parent_header
                        .hash(chainspec.protocol_config.verifiable_chunked_hash_activation)
                {
                    warn!(
                        ?peer,
                        fetched_header = ?item.header(),
                        ?parent_header,
                        "received block with wrong parent from peer",
                    );
                    effect_builder.announce_disconnect_from_peer(peer).await;
                    continue;
                }

                if let Err(error) = check_sufficient_finality_signatures(
                    trusted_key_block_info.validator_weights(),
                    chainspec.highway_config.finality_threshold_fraction,
                    item.finality_signatures(),
                ) {
                    warn!(?error, ?peer, "insufficient finality signatures from peer",);
                    effect_builder.announce_disconnect_from_peer(peer).await;
                    continue;
                }

                if let Err(error) = item.finality_signatures().verify() {
                    warn!(
                        ?error,
                        ?peer,
                        "error validating finality signatures from peer",
                    );
                    effect_builder.announce_disconnect_from_peer(peer).await;
                    continue;
                }

                // Store the block or header itself, and the finality signatures.
                item.store_block_or_header(effect_builder).await;
                let sigs = item.finality_signatures().clone();
                effect_builder.put_signatures_to_storage(sigs).await;

                break item;
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
    };

    if item.header().protocol_version() < parent_header.protocol_version() {
        return Err(Error::LowerVersionThanParent {
            parent: Box::new(parent_header.clone()),
            child: Box::new(item.header().clone()),
        });
    }

    check_block_version(item.header(), chainspec)?;

    Ok(Some(item))
}

/// Compares the block's version with the current and parent version and returns an error if it is
/// too new or old.
fn check_block_version(header: &BlockHeader, chainspec: &Chainspec) -> Result<(), Error> {
    let current_version = chainspec.protocol_config.version;

    if header.protocol_version() > current_version {
        return Err(Error::RetrievedBlockHeaderFromFutureVersion {
            current_version,
            block_header_with_future_version: Box::new(header.clone()),
        });
    }

    Ok(())
}

/// Queries all of the peers for a trie, puts the trie found from the network in the trie-store, and
/// returns any outstanding descendant tries.
async fn fetch_and_store_trie(
    effect_builder: EffectBuilder<JoinerEvent>,
    trie_key: Digest,
) -> Result<Vec<Digest>, Error> {
    let fetched_trie = fetch_trie_retry_forever(effect_builder, trie_key).await;
    match fetched_trie {
        FetchedData::FromStorage { .. } => Ok(effect_builder
            .find_missing_descendant_trie_keys(trie_key)
            .await?),
        FetchedData::FromPeer { item: trie, .. } => Ok(effect_builder
            .put_trie_and_find_missing_descendant_trie_keys(trie)
            .await?),
    }
}

/// Downloads and stores a block.
async fn fetch_and_store_block_by_hash(
    effect_builder: EffectBuilder<JoinerEvent>,
    block_hash: BlockHash,
) -> Result<Box<Block>, FetcherError<Block, NodeId>> {
    let fetched_block = fetch_retry_forever::<Block>(effect_builder, block_hash).await?;
    match fetched_block {
        FetchedData::FromStorage { item: block, .. } => Ok(block),
        FetchedData::FromPeer { item: block, .. } => {
            effect_builder.put_block_to_storage(block.clone()).await;
            Ok(block)
        }
    }
}

/// A worker task that takes trie keys from a queue and downloads the trie.
async fn sync_trie_store_worker(
    worker_id: usize,
    effect_builder: EffectBuilder<JoinerEvent>,
    abort: Arc<AtomicBool>,
    queue: Arc<WorkQueue<Digest>>,
) -> Result<(), Error> {
    while let Some(job) = queue.next_job().await {
        trace!(worker_id, trie_key = %job.inner(), "worker downloading trie");
        let child_jobs = fetch_and_store_trie(effect_builder, *job.inner())
            .await
            .map_err(|err| {
                abort.store(true, atomic::Ordering::Relaxed);
                warn!(?err, trie_key = %job.inner(), "failed to download trie");
                err
            })?;
        trace!(?child_jobs, trie_key = %job.inner(), "downloaded trie node");
        if abort.load(atomic::Ordering::Relaxed) {
            return Ok(()); // Another task failed and sent an error.
        }
        for child_job in child_jobs {
            queue.push_job(child_job);
        }
        drop(job); // Make sure the job gets dropped only when the children are in the queue.
    }
    Ok(())
}

/// Synchronizes the trie store under a given state root hash.
async fn sync_trie_store(
    effect_builder: EffectBuilder<JoinerEvent>,
    state_root_hash: Digest,
    max_parallel_trie_fetches: usize,
) -> Result<(), Error> {
    info!(?state_root_hash, "syncing trie store",);

    // Flag set by a worker when it encounters an error.
    let abort = Arc::new(AtomicBool::new(false));

    let queue = Arc::new(WorkQueue::default());
    queue.push_job(state_root_hash);
    let mut workers: FuturesUnordered<_> = (0..max_parallel_trie_fetches)
        .map(|worker_id| {
            sync_trie_store_worker(worker_id, effect_builder, abort.clone(), queue.clone())
        })
        .collect();
    while let Some(result) = workers.next().await {
        result?; // Return the error if a download failed.
    }
    Ok(())
}

/// Fetches the current header and fast-syncs to it.
///
/// Performs the following:
///
///  1. Fetches the most recent block header.
///  2. Fetches deploys for replay detection and historical switch block info.
///  3. The historical switch block info is needed by the [`EraSupervisor`].
///  4. Fetches global state for the current block header.
///
/// Returns the most recent block header along with the last trusted key block information for
/// validation.
async fn fast_sync_to_most_recent(
    effect_builder: EffectBuilder<JoinerEvent>,
    trusted_block_header: BlockHeader,
    chainspec: &Chainspec,
    node_config: NodeConfig,
) -> Result<(KeyBlockInfo, BlockHeader), Error> {
    info!("start - get_trusted_key_block_info - fast sync");
    let mut trusted_key_block_info =
        get_trusted_key_block_info(effect_builder, chainspec, &trusted_block_header).await?;
    info!("finish - get_trusted_key_block_info - fast sync");

    // Get the most recent header which has the same version as ours
    // We keep fetching by height until none of our peers have a block at that height and we are in
    // the current era.
    let mut most_recent_block_header = trusted_block_header;
    info!("start - most recent header - fast sync");
    loop {
        let maybe_fetched_block = fetch_and_store_next::<BlockHeaderWithMetadata>(
            effect_builder,
            &most_recent_block_header,
            &trusted_key_block_info,
            chainspec,
        )
        .await?;
        match maybe_fetched_block {
            Some(more_recent_block_header_with_metadata) => {
                most_recent_block_header = more_recent_block_header_with_metadata.block_header;
                // If the new block is a switch block, update the validator weights, etc...
                if let Some(key_block_info) = KeyBlockInfo::maybe_from_block_header(
                    &most_recent_block_header,
                    chainspec.protocol_config.verifiable_chunked_hash_activation,
                ) {
                    trusted_key_block_info = key_block_info;
                }
            }
            // If we timed out, consider syncing done.
            None => break,
        }
        // If we synced up to the current era, we can also consider syncing done.
        if is_current_era(
            &most_recent_block_header,
            &trusted_key_block_info,
            chainspec,
        ) {
            break;
        }
    }
    info!("finish - most recent header - fast sync");

    // Fetch and store all blocks that can contain not-yet-expired deploys. These are needed for
    // replay detection.
    info!("start - fetch and store all blocks - fast sync");
    {
        let mut current_header = most_recent_block_header.clone();
        while trusted_key_block_info
            .era_start
            .saturating_diff(current_header.timestamp())
            < chainspec.deploy_config.max_ttl
            && current_header.height() != 0
        {
            current_header =
                fetch_and_store_block_by_hash(effect_builder, *current_header.parent_hash())
                    .await?
                    .take_header();
        }
    }
    info!("finish - fetch and store all blocks - fast sync");

    // The era supervisor requires enough switch blocks to be stored in the database to be able to
    // initialize the most recent eras.
    info!("start - for era supervisor - fast sync");
    {
        let earliest_open_era = chainspec.earliest_open_era(most_recent_block_header.era_id());
        let earliest_era_needed_by_era_supervisor =
            chainspec.earliest_switch_block_needed(earliest_open_era);
        let mut current_walk_back_header = most_recent_block_header.clone();
        while current_walk_back_header.era_id() > earliest_era_needed_by_era_supervisor {
            current_walk_back_header = *fetch_and_store_block_header(
                effect_builder,
                *current_walk_back_header.parent_hash(),
            )
            .await?;
        }
    }
    info!("finish - for era supervisor - fast sync");

    // Synchronize the trie store for the most recent block header.
    info!("start - sync_trie_store - fast sync");
    sync_trie_store(
        effect_builder,
        *most_recent_block_header.state_root_hash(),
        node_config.max_parallel_trie_fetches as usize,
    )
    .await?;
    info!("finish - sync_trie_store - fast sync");

    Ok((trusted_key_block_info, most_recent_block_header))
}

/// Downloads and saves the deploys and transfers for a block.
async fn sync_deploys_and_transfers_and_state(
    effect_builder: EffectBuilder<JoinerEvent>,
    block: &Block,
    node_config: &NodeConfig,
) -> Result<(), Error> {
    let hash_iter: Vec<_> = block
        .deploy_hashes()
        .iter()
        .chain(block.transfer_hashes())
        .cloned()
        .collect();
    let mut stream = futures::stream::iter(hash_iter)
        .map(|hash| {
            debug!("start - fetch_and_store_deploy - archival sync - {}", hash);
            fetch_and_store_deploy(effect_builder, hash)
        })
        .buffer_unordered(node_config.max_parallel_deploy_fetches as usize);
    while let Some(result) = stream.next().await {
        let deploy = result?;
        debug!(
            "finish - fetch_and_store_deploy - archival sync - {}",
            deploy.id()
        );
        trace!("fetched {:?}", deploy);
    }
    debug!(
        "start - sync_deploys_and_transfers_and_state - sync_trie_store - archival sync - {}",
        block.hash()
    );
    let result = sync_trie_store(
        effect_builder,
        *block.header().state_root_hash(),
        node_config.max_parallel_trie_fetches as usize,
    )
    .await;
    debug!(
        "finish - sync_deploys_and_transfers_and_state - sync_trie_store - archival sync - {}",
        block.hash()
    );
    result
}

/// Archival sync all the way up to the current version.
///
/// Performs the following:
///
///  1. Fetches blocks all the way back to genesis.
///  2. Fetches global state for each block starting from genesis onwards.
///  3. Fetches all deploys and finality signatures.
///  4. Stops after fetching a block with the current version and its global state.
///
/// Returns the block header with our current version and the last trusted key block information for
/// validation.
async fn archival_sync(
    effect_builder: EffectBuilder<JoinerEvent>,
    trusted_block_header: BlockHeader,
    chainspec: &Chainspec,
    node_config: NodeConfig,
) -> Result<(KeyBlockInfo, BlockHeader), Error> {
    // Get the trusted block info. This will fail if we are trying to join with a trusted hash in
    // era 0.
    info!("start - get_trusted_key_block_info - archival sync");
    let mut trusted_key_block_info =
        get_trusted_key_block_info(effect_builder, chainspec, &trusted_block_header).await?;
    info!("finish - get_trusted_key_block_info - archival sync");

    info!("start - fetch_and_store_block_by_hash - archival sync");
    let trusted_block = *fetch_and_store_block_by_hash(
        effect_builder,
        trusted_block_header.hash(chainspec.protocol_config.verifiable_chunked_hash_activation),
    )
    .await?;
    info!("finish - fetch_and_store_block_by_hash - archival sync");

    // Sync to genesis
    let mut walkback_block = trusted_block.clone();
    info!("start - sync to genesis - archival sync");
    loop {
        sync_deploys_and_transfers_and_state(effect_builder, &walkback_block, &node_config).await?;
        if walkback_block.height() == 0 {
            break;
        } else {
            walkback_block = *fetch_and_store_block_by_hash(
                effect_builder,
                *walkback_block.header().parent_hash(),
            )
            .await?;
        }
    }
    info!("finish - sync to genesis - archival sync");

    // Sync forward until we are at the current version.
    info!("start - sync forward - archival sync");
    let mut most_recent_block = trusted_block;
    while most_recent_block.header().protocol_version() < chainspec.protocol_config.version {
        let maybe_fetched_block_with_metadata = fetch_and_store_next::<BlockWithMetadata>(
            effect_builder,
            most_recent_block.header(),
            &trusted_key_block_info,
            chainspec,
        )
        .await?;
        most_recent_block = match maybe_fetched_block_with_metadata {
            Some(block_with_metadata) => block_with_metadata.block,
            None => {
                tokio::time::sleep(SLEEP_DURATION_SO_WE_DONT_SPAM).await;
                continue;
            }
        };
        sync_deploys_and_transfers_and_state(effect_builder, &most_recent_block, &node_config)
            .await?;
        if let Some(key_block_info) = KeyBlockInfo::maybe_from_block_header(
            most_recent_block.header(),
            chainspec.protocol_config.verifiable_chunked_hash_activation,
        ) {
            trusted_key_block_info = key_block_info;
        }
    }
    info!("finish - sync forward - archival sync");

    Ok((trusted_key_block_info, most_recent_block.take_header()))
}

/// Runs the chain synchronization task.
pub(super) async fn run_chain_sync_task(
    effect_builder: EffectBuilder<JoinerEvent>,
    trusted_hash: BlockHash,
    chainspec: Arc<Chainspec>,
    node_config: NodeConfig,
) -> Result<BlockHeader, Error> {
    // Fetch the trusted header
    info!("start - fetch trusted header");
    let trusted_block_header = fetch_and_store_block_header(effect_builder, trusted_hash).await?;
    info!("finish - fetch trusted header");

    if trusted_block_header.protocol_version() > chainspec.protocol_config.version {
        return Err(Error::RetrievedBlockHeaderFromFutureVersion {
            current_version: chainspec.protocol_config.version,
            block_header_with_future_version: trusted_block_header,
        });
    }

    let era_duration: TimeDiff = cmp::max(
        chainspec.highway_config.min_round_length() * chainspec.core_config.minimum_era_height,
        chainspec.core_config.era_duration,
    );

    if trusted_block_header.timestamp()
        + era_duration
            * chainspec
                .core_config
                .unbonding_delay
                .saturating_sub(chainspec.core_config.auction_delay)
        < Timestamp::now()
    {
        warn!(
            ?trusted_block_header,
            "timestamp of trusted hash is older than \
             era_duration * (unbonding_delay - auction_delay)"
        );
    }

    let maybe_last_emergency_restart_era_id = chainspec.protocol_config.last_emergency_restart;
    if let Some(last_emergency_restart_era) = maybe_last_emergency_restart_era_id {
        // After an emergency restart, the old validators cannot be trusted anymore. So the last
        // block before the restart or a later block must be given by the trusted hash. That way we
        // never have to use the untrusted validators' finality signatures.
        if trusted_block_header.next_block_era_id() < last_emergency_restart_era {
            return Err(Error::TryingToJoinBeforeLastEmergencyRestartEra {
                last_emergency_restart_era,
                trusted_hash,
                trusted_block_header,
            });
        }
        // If the trusted hash specifies the last block before the emergency restart, we have to
        // compute the immediate switch block ourselves, since there's no other way to verify that
        // block. We just sync the trie there and return, so the upgrade can be applied.
        if trusted_block_header.is_switch_block()
            && trusted_block_header.next_block_era_id() == last_emergency_restart_era
        {
            info!("start - sync_trie_store - emergency restart");
            sync_trie_store(
                effect_builder,
                *trusted_block_header.state_root_hash(),
                node_config.max_parallel_trie_fetches as usize,
            )
            .await?;
            info!("finish - sync_trie_store - emergency restart");
            return Ok(*trusted_block_header);
        }
    }

    // If we are at an upgrade:
    // 1. Sync the trie store
    // 2. Get the trusted era validators from the last switch block
    // 3. Try to get the next block by height; if there is `None` then switch to the participating
    //    reactor.
    if trusted_block_header.is_switch_block()
        && trusted_block_header.next_block_era_id()
            == chainspec.protocol_config.activation_point.era_id()
    {
        info!("start - get_trusted_key_block_info - upgrade");
        let trusted_key_block_info =
            get_trusted_key_block_info(effect_builder, &*chainspec, &trusted_block_header).await?;
        info!("finish - get_trusted_key_block_info - upgrade");

        info!("start - fetch_and_store_next::<BlockHeaderWithMetadata> - upgrade");
        let fetch_and_store_next_result = fetch_and_store_next::<BlockHeaderWithMetadata>(
            effect_builder,
            &trusted_block_header,
            &trusted_key_block_info,
            &*chainspec,
        )
        .await?;
        info!("finish - fetch_and_store_next::<BlockHeaderWithMetadata> - upgrade");

        if fetch_and_store_next_result.is_none() {
            info!("start - sync_trie_store - upgrade");
            sync_trie_store(
                effect_builder,
                *trusted_block_header.state_root_hash(),
                node_config.max_parallel_trie_fetches as usize,
            )
            .await?;
            info!("finish - sync_trie_store - upgrade");
            return Ok(*trusted_block_header);
        }
    }

    let (mut trusted_key_block_info, mut most_recent_block_header) = if node_config.archival_sync {
        info!("start - archival_sync - total");
        let result = archival_sync(
            effect_builder,
            *trusted_block_header,
            &chainspec,
            node_config,
        )
        .await?;
        info!("finish - archival_sync - total");
        result
    } else {
        info!("start - fast_sync_to_most_recent - total");
        let result = fast_sync_to_most_recent(
            effect_builder,
            *trusted_block_header,
            &chainspec,
            node_config,
        )
        .await?;
        info!("finish - fast_sync_to_most_recent - total");
        result
    };

    // Execute blocks to get to current.
    let mut execution_pre_state = ExecutionPreState::from_block_header(
        &most_recent_block_header,
        chainspec.protocol_config.verifiable_chunked_hash_activation,
    );
    info!(
        era_id = ?most_recent_block_header.era_id(),
        height = most_recent_block_header.height(),
        now = %Timestamp::now(),
        block_timestamp = %most_recent_block_header.timestamp(),
        "fetching and executing blocks to synchronize to current",
    );

    info!("start - fetching and executing blocks - loop total");
    loop {
        info!(
            "start - fetching block - {}",
            trusted_key_block_info.key_block_hash
        );
        let result = fetch_and_store_next::<BlockWithMetadata>(
            effect_builder,
            &most_recent_block_header,
            &trusted_key_block_info,
            &*chainspec,
        )
        .await?;
        info!(
            "finish - fetching block - {}",
            trusted_key_block_info.key_block_hash
        );
        let block = match result {
            None => {
                info!(
                    era = most_recent_block_header.era_id().value(),
                    height = most_recent_block_header.height(),
                    timestamp = %most_recent_block_header.timestamp(),
                    "couldn't download a more recent block; finishing syncing",
                );
                break;
            }
            Some(block_with_metadata) => block_with_metadata.block,
        };

        let mut deploys: Vec<Deploy> = Vec::with_capacity(block.deploy_hashes().len());
        for deploy_hash in block.deploy_hashes() {
            info!("start - fetching deploy - {}", *deploy_hash);
            let result = fetch_and_store_deploy(effect_builder, *deploy_hash).await?;
            info!("finish - fetching deploy - {}", *deploy_hash);
            deploys.push(*result);
        }
        let mut transfers: Vec<Deploy> = Vec::with_capacity(block.transfer_hashes().len());
        for transfer_hash in block.transfer_hashes() {
            info!("start - fetching transfer - {}", *transfer_hash);
            let result = fetch_and_store_deploy(effect_builder, *transfer_hash).await?;
            info!("finish - fetching transfer - {}", *transfer_hash);
            transfers.push(*result);
        }

        info!(
            era_id = ?block.header().era_id(),
            height = block.height(),
            now = %Timestamp::now(),
            block_timestamp = %block.timestamp(),
            "executing block",
        );
        info!("start - executing finalized block - {}", block.hash());
        let block_and_execution_effects = effect_builder
            .execute_finalized_block(
                block.protocol_version(),
                execution_pre_state.clone(),
                FinalizedBlock::from(block.clone()),
                deploys,
                transfers,
            )
            .await?;
        info!("finish - executing finalized block - {}", block.hash());

        if block != *block_and_execution_effects.block() {
            return Err(Error::ExecutedBlockIsNotTheSameAsDownloadedBlock {
                executed_block: Box::new(Block::from(block_and_execution_effects)),
                downloaded_block: Box::new(block.clone()),
            });
        }

        most_recent_block_header = block.take_header();
        execution_pre_state = ExecutionPreState::from_block_header(
            &most_recent_block_header,
            chainspec.protocol_config.verifiable_chunked_hash_activation,
        );

        if let Some(key_block_info) = KeyBlockInfo::maybe_from_block_header(
            &most_recent_block_header,
            chainspec.protocol_config.verifiable_chunked_hash_activation,
        ) {
            trusted_key_block_info = key_block_info;
        }

        // If we managed to sync up to the current era, stop - we'll have to sync the consensus
        // protocol state, anyway.
        if is_current_era(
            &most_recent_block_header,
            &trusted_key_block_info,
            &chainspec,
        ) {
            info!(
                era = most_recent_block_header.era_id().value(),
                height = most_recent_block_header.height(),
                timestamp = %most_recent_block_header.timestamp(),
                "synchronized up to the current era; finishing syncing",
            );
            break;
        }
    }
    info!("finish - fetching and executing blocks - loop total");

    info!(
        era_id = ?most_recent_block_header.era_id(),
        height = most_recent_block_header.height(),
        now = %Timestamp::now(),
        block_timestamp = %most_recent_block_header.timestamp(),
        "finished synchronizing",
    );

    Ok(most_recent_block_header)
}

/// Returns `true` if `most_recent_block` belongs to an era that is still ongoing.
pub(super) fn is_current_era(
    most_recent_block: &BlockHeader,
    trusted_key_block_info: &KeyBlockInfo,
    chainspec: &Chainspec,
) -> bool {
    is_current_era_given_current_timestamp(
        most_recent_block,
        trusted_key_block_info,
        chainspec,
        Timestamp::now(),
    )
}

fn is_current_era_given_current_timestamp(
    most_recent_block: &BlockHeader,
    trusted_key_block_info: &KeyBlockInfo,
    chainspec: &Chainspec,
    current_timestamp: Timestamp,
) -> bool {
    let KeyBlockInfo {
        era_start, height, ..
    } = trusted_key_block_info;

    // If the minimum era duration has not yet run out, the era is still current.
    if current_timestamp.saturating_diff(*era_start) < chainspec.core_config.era_duration {
        return true;
    }

    // Otherwise estimate the earliest possible end of this era based on how many blocks remain.
    let remaining_blocks_in_this_era = chainspec
        .core_config
        .minimum_era_height
        .saturating_sub(most_recent_block.height() - *height);
    let min_round_length = chainspec.highway_config.min_round_length();
    let time_since_most_recent_block =
        current_timestamp.saturating_diff(most_recent_block.timestamp());
    time_since_most_recent_block < min_round_length * remaining_blocks_in_this_era
}

#[cfg(test)]
mod tests {
    use std::iter;

    use rand::Rng;

    use casper_types::{EraId, ProtocolVersion, PublicKey, SecretKey};

    use super::*;

    use crate::{
        components::consensus::EraReport,
        crypto::AsymmetricKeyExt,
        testing::TestRng,
        types::{Block, BlockPayload, FinalizedBlock},
        utils::Loadable,
    };

    /// Creates a block for testing, with the given data, and returns its header.
    ///
    /// The other fields are filled in with defaults, since they are not used in these tests.
    fn create_block(
        timestamp: Timestamp,
        era_id: EraId,
        height: u64,
        switch_block: bool,
        verifiable_chunked_hash_activation: EraId,
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
            verifiable_chunked_hash_activation,
        )
        .expect("failed to create block for tests")
        .take_header()
    }

    #[test]
    fn test_is_current_era() {
        let mut rng = TestRng::new();
        let mut chainspec = Chainspec::from_resources("local");

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

        // `verifiable_chunked_hash_activation` can be chosen arbitrarily
        let verifiable_chunked_hash_activation = EraId::from(rng.gen_range(0..=10));

        // We assume era 6 started after six minimum era durations, at block 100.
        let era6_start = genesis_time + era_duration * 6;
        let switch_block5 = create_block(
            era6_start,
            EraId::from(5),
            100,
            true,
            verifiable_chunked_hash_activation,
        );

        let trusted_switch_block_info5 = KeyBlockInfo::maybe_from_block_header(
            &switch_block5,
            verifiable_chunked_hash_activation,
        )
        .expect("no switch block info for switch block");

        // If we are still within the minimum era duration the era is current, even if we have the
        // required number of blocks (115 - 100 > 10).
        let block_time = era6_start + era_duration - 10.into();
        let now = block_time + 5.into();
        let block = create_block(
            block_time,
            EraId::from(6),
            115,
            false,
            verifiable_chunked_hash_activation,
        );
        assert!(is_current_era_given_current_timestamp(
            &block,
            &trusted_switch_block_info5,
            &chainspec,
            now
        ));

        // If the minimum duration has passed but we we know we don't have all blocks yet, it's
        // also still current. There are still five blocks missing but only four rounds have
        // passed.
        let block_time = era6_start + era_duration * 2;
        let now = block_time + min_round_length * 4;
        let block = create_block(
            block_time,
            EraId::from(6),
            105,
            false,
            verifiable_chunked_hash_activation,
        );
        assert!(is_current_era_given_current_timestamp(
            &block,
            &trusted_switch_block_info5,
            &chainspec,
            now
        ));

        // If both criteria are satisfied, the era could have ended.
        let block_time = era6_start + era_duration * 2;
        let now = block_time + min_round_length * 5;
        let block = create_block(
            block_time,
            EraId::from(6),
            105,
            false,
            verifiable_chunked_hash_activation,
        );
        assert!(!is_current_era_given_current_timestamp(
            &block,
            &trusted_switch_block_info5,
            &chainspec,
            now
        ));
    }

    #[test]
    fn test_check_block_version() {
        let mut rng = TestRng::new();
        let mut chainspec = Chainspec::from_resources("local");
        let v1_2_0 = ProtocolVersion::from_parts(1, 2, 0);
        let v1_3_0 = ProtocolVersion::from_parts(1, 3, 0);

        // `verifiable_chunked_hash_activation` can be chosen arbitrarily
        let verifiable_chunked_hash_activation = EraId::from(rng.gen_range(0..=10));
        let header = Block::random_with_specifics(
            &mut rng,
            EraId::from(6),
            101,
            v1_3_0,
            false,
            verifiable_chunked_hash_activation,
        )
        .take_header();

        // The new block's protocol version is the current one, 1.3.0.
        chainspec.protocol_config.version = v1_3_0;
        check_block_version(&header, &chainspec).expect("versions are valid");

        // If the current version is only 1.2.0 but the block's is 1.3.0, we have to upgrade.
        chainspec.protocol_config.version = v1_2_0;
        match check_block_version(&header, &chainspec) {
            Err(Error::RetrievedBlockHeaderFromFutureVersion {
                current_version,
                block_header_with_future_version,
            }) => {
                assert_eq!(v1_2_0, current_version);
                assert_eq!(header, *block_header_with_future_version);
            }
            result => panic!("expected future block version error, got {:?}", result),
        }
    }
}
