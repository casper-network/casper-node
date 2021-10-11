use std::{collections::BTreeMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use num::rational::Ratio;
use tracing::{info, trace, warn};

use casper_execution_engine::{
    shared::{newtypes::Blake2bHash, stored_value::StoredValue},
    storage::trie::Trie,
};
use casper_types::{EraId, Key, PublicKey, U512};

use crate::{
    components::{
        consensus,
        consensus::check_sufficient_finality_signatures,
        contract_runtime::ExecutionPreState,
        fetcher::{FetchResult, FetchedData, FetcherError},
        linear_chain_sync::error::{LinearChainSyncError, SignatureValidationError},
    },
    crypto::hash::Digest,
    effect::{requests::FetcherRequest, EffectBuilder},
    reactor::joiner::JoinerEvent,
    types::{
        Block, BlockHash, BlockHeader, BlockHeaderWithMetadata, BlockSignatures, BlockWithMetadata,
        Chainspec, Deploy, DeployHash, FinalizedBlock, Item, NodeConfig, NodeId, TimeDiff,
        Timestamp,
    },
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
        for peer in effect_builder.get_peers_in_random_order().await {
            trace!(
                "Attempting to fetch {:?} with id {:?} from {:?}",
                T::TAG,
                id,
                peer
            );
            match effect_builder.fetch::<T, NodeId>(id, peer).await {
                Ok(fetched_data @ FetchedData::FromStorage { .. }) => {
                    trace!(
                        "Did not get {:?} with id {:?} from {:?}, got from storage instead",
                        T::TAG,
                        id,
                        peer
                    );
                    return Ok(fetched_data);
                }
                Ok(fetched_data @ FetchedData::FromPeer { .. }) => {
                    trace!("Fetched {:?} with id {:?} from {:?}", T::TAG, id, peer);
                    return Ok(fetched_data);
                }
                Err(FetcherError::Absent { .. }) => {
                    warn!(
                        ?id,
                        tag = ?T::TAG,
                        ?peer,
                        "Fast sync could not fetch; trying next peer",
                    )
                }
                Err(FetcherError::TimedOut { .. }) => {
                    warn!(
                        ?id,
                        tag = ?T::TAG,
                        ?peer,
                        "Peer timed out",
                    );
                }
                Err(error @ FetcherError::CouldNotConstructGetRequest { .. }) => return Err(error),
            }
        }
        tokio::time::sleep(SLEEP_DURATION_SO_WE_DONT_SPAM).await
    }
}

/// Fetches and stores a block header from the network.
async fn fetch_and_store_block_header(
    effect_builder: EffectBuilder<JoinerEvent>,
    block_hash: BlockHash,
) -> Result<Box<BlockHeader>, LinearChainSyncError> {
    // Only genesis should have this as previous hash, so no block should ever have it...
    if block_hash == BlockHash::default() {
        return Err(LinearChainSyncError::NoSuchBlockHash {
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

/// Verifies finality signatures for a block header
fn validate_finality_signatures(
    block_header: &BlockHeader,
    trusted_key_block_info: &KeyBlockInfo,
    finality_threshold_fraction: Ratio<u64>,
    block_signatures: &BlockSignatures,
) -> Result<(), SignatureValidationError> {
    if block_header.era_id() != trusted_key_block_info.era_id {
        return Err(SignatureValidationError::HeaderIsInWrongEra {
            block_header: Box::new(block_header.clone()),
            trusted_key_block_info: Box::new(trusted_key_block_info.clone()),
        });
    }

    // Check the signatures' block hash is the header's block hash
    let block_hash = block_header.hash();
    if block_signatures.block_hash != block_hash {
        return Err(
            SignatureValidationError::SignaturesDoNotCorrespondToBlockHeader {
                block_header: Box::new(block_header.clone()),
                block_hash: Box::new(block_hash),
                block_signatures: Box::new(block_signatures.clone()),
            },
        );
    }

    // Cryptographically verify block signatures
    block_signatures.verify()?;

    check_sufficient_finality_signatures(
        &trusted_key_block_info.validator_weights,
        finality_threshold_fraction,
        block_signatures,
    )
    .map_err(Into::into)
}

/// Key block info used for verifying finality signatures.
///
/// Can come from either:
///   - A switch block
///   - The global state under a genesis block
///
/// If the data was scraped from genesis, then `era_id` is 0.
/// Otherwise if it came from a switch block it is that switch block's `era_id + 1`.
#[derive(Clone, Debug)]
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
    fn maybe_from_block_header(block_header: &BlockHeader) -> Option<KeyBlockInfo> {
        block_header
            .next_era_validator_weights()
            .map(|next_era_validator_weights| KeyBlockInfo {
                key_block_hash: block_header.hash(),
                validator_weights: next_era_validator_weights.clone(),
                era_start: block_header.timestamp(),
                height: block_header.height(),
                era_id: block_header.era_id() + 1,
            })
    }
}

/// Gets the trusted key block info for a trusted block header.
async fn get_trusted_key_block_info(
    effect_builder: EffectBuilder<JoinerEvent>,
    chainspec: &Chainspec,
    trusted_header: &BlockHeader,
) -> Result<KeyBlockInfo, LinearChainSyncError> {
    // If the trusted block's version is newer than ours we return an error
    if trusted_header.protocol_version() > chainspec.protocol_config.version {
        return Err(
            LinearChainSyncError::RetrievedBlockHeaderFromFutureVersion {
                current_version: chainspec.protocol_config.version,
                block_header_with_future_version: Box::new(trusted_header.clone()),
            },
        );
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
                return Err(LinearChainSyncError::TrustedHeaderEraTooEarly {
                    trusted_header: Box::new(trusted_header.clone()),
                    maybe_last_emergency_restart_era_id: chainspec
                        .protocol_config
                        .last_emergency_restart,
                })
            }
            _ => {}
        }

        if let Some(key_block_info) =
            KeyBlockInfo::maybe_from_block_header(&current_header_to_walk_back_from)
        {
            check_block_version(trusted_header, &key_block_info, chainspec)?;
            break Ok(key_block_info);
        }

        if current_header_to_walk_back_from.height() == 0 {
            break Err(
                LinearChainSyncError::HitGenesisBlockTryingToGetTrustedEraValidators {
                    trusted_header: trusted_header.clone(),
                },
            );
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
) -> Result<Option<Box<I>>, LinearChainSyncError>
where
    I: BlockOrHeaderWithMetadata,
    JoinerEvent: From<FetcherRequest<NodeId, I>>,
    LinearChainSyncError: From<FetcherError<I, NodeId>>,
{
    let height = parent_header.height() + 1;
    let mut peers = effect_builder.get_peers_in_random_order().await.into_iter();
    let item = loop {
        let peer = match peers.next() {
            Some(peer) => peer,
            None => return Ok(None),
        };
        match effect_builder.fetch::<I, NodeId>(height, peer).await {
            Ok(FetchedData::FromStorage { item }) => {
                if *item.header().parent_hash() != parent_header.hash() {
                    return Err(LinearChainSyncError::UnexpectedParentHash {
                        parent: Box::new(parent_header.clone()),
                        child: Box::new(item.header().clone()),
                    });
                }
                break item;
            }
            Ok(FetchedData::FromPeer { item, .. }) => {
                if *item.header().parent_hash() != parent_header.hash() {
                    warn!(
                        ?peer,
                        fetched_header = ?item.header(),
                        ?parent_header,
                        "received block with wrong parent from peer",
                    );
                    continue;
                }

                if let Err(error) = validate_finality_signatures(
                    item.header(),
                    trusted_key_block_info,
                    chainspec.highway_config.finality_threshold_fraction,
                    item.finality_signatures(),
                ) {
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
                warn!(height, tag = ?I::TAG, ?peer, "Block by height absent from peer");
                // If the peer we requested doesn't have the item, continue with the next peer
                continue;
            }
            Err(FetcherError::TimedOut { .. }) => {
                warn!(height, tag = ?I::TAG, ?peer, "Peer timed out");
                // Peer timed out fetching the item, continue with the next peer
                continue;
            }
            Err(error) => return Err(error.into()),
        }
    };

    if item.header().protocol_version() < parent_header.protocol_version() {
        return Err(LinearChainSyncError::LowerVersionThanParent {
            parent: Box::new(parent_header.clone()),
            child: Box::new(item.header().clone()),
        });
    }

    check_block_version(item.header(), trusted_key_block_info, chainspec)?;

    Ok(Some(item))
}

/// Compares the block's version with the current and parent version and returns an error if it is
/// too new or old.
fn check_block_version(
    header: &BlockHeader,
    trusted_key_block_info: &KeyBlockInfo,
    chainspec: &Chainspec,
) -> Result<(), LinearChainSyncError> {
    let current_version = chainspec.protocol_config.version;

    if header.protocol_version() > current_version {
        return Err(
            LinearChainSyncError::RetrievedBlockHeaderFromFutureVersion {
                current_version,
                block_header_with_future_version: Box::new(header.clone()),
            },
        );
    }

    if is_current_era(header, trusted_key_block_info, chainspec)
        && header.protocol_version() < current_version
    {
        return Err(LinearChainSyncError::CurrentBlockHeaderHasOldVersion {
            current_version,
            block_header_with_old_version: Box::new(header.clone()),
        });
    }

    Ok(())
}

/// Queries all of the peers for a trie, puts the trie found from the network in the trie-store, and
/// returns any outstanding descendant tries.
async fn fetch_trie_and_insert_into_trie_store(
    effect_builder: EffectBuilder<JoinerEvent>,
    trie_key: Blake2bHash,
) -> Result<Vec<Blake2bHash>, LinearChainSyncError> {
    let fetched_trie =
        fetch_retry_forever::<Trie<Key, StoredValue>>(effect_builder, trie_key).await?;
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

/// Synchronize the trie store under a given state root hash.
async fn sync_trie_store(
    effect_builder: EffectBuilder<JoinerEvent>,
    state_root_hash: Digest,
) -> Result<(), LinearChainSyncError> {
    info!(?state_root_hash, "syncing trie store",);
    let mut outstanding_trie_keys = vec![Blake2bHash::from(state_root_hash)];
    while let Some(trie_key) = outstanding_trie_keys.pop() {
        let missing_descendant_trie_keys =
            fetch_trie_and_insert_into_trie_store(effect_builder, trie_key).await?;
        outstanding_trie_keys.extend(missing_descendant_trie_keys);
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
) -> Result<(KeyBlockInfo, BlockHeader), LinearChainSyncError> {
    let mut trusted_key_block_info =
        get_trusted_key_block_info(effect_builder, chainspec, &trusted_block_header).await?;

    // Get the most recent header which has the same version as ours
    // We keep fetching by height until none of our peers have a block at that height and we are in
    // the current era.
    let mut most_recent_block_header = trusted_block_header;
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
                if let Some(key_block_info) =
                    KeyBlockInfo::maybe_from_block_header(&most_recent_block_header)
                {
                    trusted_key_block_info = key_block_info;
                }
            }
            // If we could not fetch, we can stop when the most recent header is in the current
            // era.
            None if is_current_era(
                &most_recent_block_header,
                &trusted_key_block_info,
                chainspec,
            ) =>
            {
                break
            }
            // Otherwise keep trying to fetch until we get a block with our version
            None => tokio::time::sleep(SLEEP_DURATION_SO_WE_DONT_SPAM).await,
        }
    }

    // Fetch and store all blocks that can contain not-yet-expired deploys. These are needed for
    // replay detection.
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

    // The era supervisor needs validator information from previous eras.
    // The number of previous eras is determined by a *delay* in which consensus participants become
    // bonded validators or unbond.
    let delay = consensus::bonded_eras(&chainspec.into());
    // The era supervisor requires at least to 3*delay + 1 eras back to be stored in the database.
    let historical_eras_needed = delay.saturating_mul(3).saturating_add(1);
    let earliest_era_needed_by_era_supervisor = most_recent_block_header
        .era_id()
        .saturating_sub(historical_eras_needed);
    {
        let mut current_walk_back_header = most_recent_block_header.clone();
        while current_walk_back_header.era_id() > earliest_era_needed_by_era_supervisor {
            current_walk_back_header = *fetch_and_store_block_header(
                effect_builder,
                *current_walk_back_header.parent_hash(),
            )
            .await?;
        }
    }

    // Synchronize the trie store for the most recent block header.
    sync_trie_store(effect_builder, *most_recent_block_header.state_root_hash()).await?;

    Ok((trusted_key_block_info, most_recent_block_header))
}

/// Downloads and saves the deploys and transfers for a block.
async fn sync_deploys_and_transfers_and_state(
    effect_builder: EffectBuilder<JoinerEvent>,
    block: &Block,
) -> Result<(), LinearChainSyncError> {
    for deploy_hash in block.deploy_hashes() {
        fetch_and_store_deploy(effect_builder, *deploy_hash).await?;
    }
    for transfer_hash in block.transfer_hashes() {
        fetch_and_store_deploy(effect_builder, *transfer_hash).await?;
    }
    sync_trie_store(effect_builder, *block.header().state_root_hash()).await?;
    Ok(())
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
) -> Result<(KeyBlockInfo, BlockHeader), LinearChainSyncError> {
    // Get the trusted block info. This will fail if we are trying to join with a trusted hash in
    // era 0.
    let mut trusted_key_block_info =
        get_trusted_key_block_info(effect_builder, chainspec, &trusted_block_header).await?;

    let trusted_block =
        *fetch_and_store_block_by_hash(effect_builder, trusted_block_header.hash()).await?;

    // Sync to genesis
    let mut walkback_block = trusted_block.clone();
    loop {
        sync_deploys_and_transfers_and_state(effect_builder, &walkback_block).await?;
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

    // Sync forward until we are at the current version.
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
        sync_deploys_and_transfers_and_state(effect_builder, &most_recent_block).await?;
        if let Some(key_block_info) =
            KeyBlockInfo::maybe_from_block_header(most_recent_block.header())
        {
            trusted_key_block_info = key_block_info;
        }
    }

    Ok((trusted_key_block_info, most_recent_block.take_header()))
}

/// Runs the fast synchronization task.
pub(crate) async fn run_fast_sync_task(
    effect_builder: EffectBuilder<JoinerEvent>,
    trusted_hash: BlockHash,
    chainspec: Arc<Chainspec>,
    node_config: NodeConfig,
) -> Result<BlockHeader, LinearChainSyncError> {
    // Fetch the trusted header
    let trusted_block_header = fetch_and_store_block_header(effect_builder, trusted_hash).await?;

    if trusted_block_header.protocol_version() > chainspec.protocol_config.version {
        return Err(
            LinearChainSyncError::RetrievedBlockHeaderFromFutureVersion {
                current_version: chainspec.protocol_config.version,
                block_header_with_future_version: trusted_block_header,
            },
        );
    }

    let era_duration: TimeDiff = std::cmp::max(
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
            "Timestamp of trusted hash is older than \
             era_duration * (unbonding_delay - auction_delay)"
        );
    }

    let maybe_last_emergency_restart_era_id = chainspec.protocol_config.last_emergency_restart;
    if let Some(last_emergency_restart_era) = maybe_last_emergency_restart_era_id {
        if last_emergency_restart_era > trusted_block_header.era_id() {
            return Err(
                LinearChainSyncError::TryingToJoinBeforeLastEmergencyRestartEra {
                    last_emergency_restart_era,
                    trusted_hash,
                    trusted_block_header,
                },
            );
        }
    }

    // If we are at an upgrade:
    // 1. Sync the trie store
    // 2. Get the trusted era validators from the last switch block
    // 3. Try to get the next block by height; if there is `None` then switch to the participating
    //    reactor.
    if trusted_block_header.is_switch_block()
        && trusted_block_header.era_id().successor()
            == chainspec.protocol_config.activation_point.era_id()
    {
        // TODO: handle emergency updates
        let trusted_key_block_info =
            get_trusted_key_block_info(effect_builder, &*chainspec, &trusted_block_header).await?;
        if fetch_and_store_next::<BlockHeaderWithMetadata>(
            effect_builder,
            &trusted_block_header,
            &trusted_key_block_info,
            &*chainspec,
        )
        .await?
        .is_none()
        {
            sync_trie_store(effect_builder, *trusted_block_header.state_root_hash()).await?;
            return Ok(*trusted_block_header);
        }
    }

    let (mut trusted_key_block_info, mut most_recent_block_header) = if node_config.archival_sync {
        archival_sync(effect_builder, *trusted_block_header, &chainspec).await?
    } else {
        fast_sync_to_most_recent(effect_builder, *trusted_block_header, &chainspec).await?
    };

    // Execute blocks to get to current.
    let mut execution_pre_state = ExecutionPreState::from(&most_recent_block_header);
    info!(
        era_id = ?most_recent_block_header.era_id(),
        height = most_recent_block_header.height(),
        now = %Timestamp::now(),
        block_timestamp = %most_recent_block_header.timestamp(),
        "Fetching and executing blocks to synchronize to current",
    );
    loop {
        let block = match fetch_and_store_next::<BlockWithMetadata>(
            effect_builder,
            &most_recent_block_header,
            &trusted_key_block_info,
            &*chainspec,
        )
        .await?
        {
            None => {
                if is_current_era(
                    &most_recent_block_header,
                    &trusted_key_block_info,
                    &chainspec,
                ) {
                    info!(
                        era = most_recent_block_header.era_id().value(),
                        height = most_recent_block_header.height(),
                        timestamp = %most_recent_block_header.timestamp(),
                        "Finished executing blocks; synchronized to current era",
                    );
                    break;
                } else {
                    tokio::time::sleep(SLEEP_DURATION_SO_WE_DONT_SPAM).await;
                    continue;
                }
            }
            Some(block_with_metadata) => block_with_metadata.block,
        };

        let mut deploys: Vec<Deploy> = Vec::with_capacity(block.deploy_hashes().len());
        for deploy_hash in block.deploy_hashes() {
            deploys.push(*fetch_and_store_deploy(effect_builder, *deploy_hash).await?);
        }
        let mut transfers: Vec<Deploy> = Vec::with_capacity(block.transfer_hashes().len());
        for transfer_hash in block.transfer_hashes() {
            transfers.push(*fetch_and_store_deploy(effect_builder, *transfer_hash).await?);
        }

        info!(
            era_id = ?block.header().era_id(),
            height = block.height(),
            now = %Timestamp::now(),
            block_timestamp = %block.timestamp(),
            "Executing block",
        );
        let block_and_execution_effects = effect_builder
            .execute_finalized_block(
                block.protocol_version(),
                execution_pre_state.clone(),
                FinalizedBlock::from(block.clone()),
                deploys,
                transfers,
            )
            .await?;

        if block != *block_and_execution_effects.block() {
            return Err(
                LinearChainSyncError::ExecutedBlockIsNotTheSameAsDownloadedBlock {
                    executed_block: Box::new(Block::from(block_and_execution_effects)),
                    downloaded_block: Box::new(block.clone()),
                },
            );
        }

        most_recent_block_header = block.take_header();
        execution_pre_state = ExecutionPreState::from(&most_recent_block_header);

        if let Some(key_block_info) =
            KeyBlockInfo::maybe_from_block_header(&most_recent_block_header)
        {
            trusted_key_block_info = key_block_info;
        }
    }

    info!(
        era_id = ?most_recent_block_header.era_id(),
        height = most_recent_block_header.height(),
        now = %Timestamp::now(),
        block_timestamp = %most_recent_block_header.timestamp(),
        "Finished synchronizing",
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

    use super::*;

    use casper_types::{EraId, ProtocolVersion, PublicKey, SecretKey};

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
            &chainspec,
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
            &chainspec,
            now
        ));

        // If both criteria are satisfied, the era could have ended.
        let block_time = era6_start + era_duration * 2;
        let now = block_time + min_round_length * 5;
        let block = create_block(block_time, EraId::from(6), 105, false);
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
        let v1_4_0 = ProtocolVersion::from_parts(1, 4, 0);

        let key_block =
            Block::random_with_specifics(&mut rng, EraId::from(5), 100, v1_2_0, true).take_header();
        let key_block_info = KeyBlockInfo::maybe_from_block_header(&key_block).unwrap();
        let header = Block::random_with_specifics(&mut rng, EraId::from(6), 101, v1_3_0, false)
            .take_header();

        // The new block's protocol version is the current one, 1.3.0.
        chainspec.protocol_config.version = v1_3_0;
        check_block_version(&header, &key_block_info, &chainspec).expect("versions are valid");

        // If the current version is only 1.2.0 but the block's is 1.3.0, we have to upgrade.
        chainspec.protocol_config.version = v1_2_0;
        match check_block_version(&header, &key_block_info, &chainspec) {
            Err(LinearChainSyncError::RetrievedBlockHeaderFromFutureVersion {
                current_version,
                block_header_with_future_version,
            }) => {
                assert_eq!(v1_2_0, current_version);
                assert_eq!(header, *block_header_with_future_version);
            }
            result => panic!("expected future block version error, got {:?}", result),
        }

        // If the current version is 1.4.0 but the current block's is 1.3.0, we have to downgrade.
        chainspec.protocol_config.version = v1_4_0;
        match check_block_version(&header, &key_block_info, &chainspec) {
            Err(LinearChainSyncError::CurrentBlockHeaderHasOldVersion {
                current_version,
                block_header_with_old_version,
            }) => {
                assert_eq!(v1_4_0, current_version);
                assert_eq!(header, *block_header_with_old_version);
            }
            result => panic!("expected old block version error, got {:?}", result),
        }

        // If the block is the last one of its era, we don't know whether it's the current block,
        // so we don't necessarily need to downgrade.
        let key_block =
            Block::random_with_specifics(&mut rng, EraId::from(5), 100, v1_2_0, true).take_header();
        let key_block_info = KeyBlockInfo::maybe_from_block_header(&key_block).unwrap();
        let last_block_height = key_block.height() + chainspec.core_config.minimum_era_height;
        let header =
            Block::random_with_specifics(&mut rng, EraId::from(6), last_block_height, v1_3_0, true)
                .take_header();
        chainspec.core_config.era_duration = 0.into();
        chainspec.protocol_config.version = v1_4_0;
        check_block_version(&header, &key_block_info, &chainspec).expect("versions are valid");
    }
}
