use std::{collections::BTreeMap, sync::Arc, time::Duration};

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
        contract_runtime::{EraValidatorsRequest, ExecutionPreState},
        fetcher::{FetchedData, FetcherError},
        linear_chain_sync::error::{LinearChainSyncError, SignatureValidationError},
    },
    crypto::hash::Digest,
    effect::{requests::FetcherRequest, EffectBuilder},
    reactor::joiner::JoinerEvent,
    types::{
        Block, BlockHash, BlockHeader, BlockHeaderWithMetadata, BlockSignatures, BlockWithMetadata,
        Chainspec, Deploy, DeployHash, FinalizedBlock, Item, NodeConfig, NodeId, Timestamp,
    },
};

const SLEEP_DURATION_SO_WE_DONT_SPAM: Duration = Duration::from_millis(100);

/// Fetches an item. Keeps retrying to fetch until it is successful. Assumes no integrity check is
/// necessary for the item. Not suited to fetching a block header or block by height, which require
/// verification with finality signatures.
async fn fetch_retry_forever<T>(
    effect_builder: EffectBuilder<JoinerEvent>,
    id: T::Id,
) -> Result<Box<T>, FetcherError<T, NodeId>>
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
                Ok(FetchedData::FromStorage { item }) => {
                    trace!(
                        "Did not get {:?} with id {:?} from {:?}, got from storage instead",
                        T::TAG,
                        id,
                        peer
                    );
                    return Ok(item);
                }
                Ok(FetchedData::FromPeer { item, .. }) => {
                    trace!("Fetched {:?} with id {:?} from {:?}", T::TAG, id, peer);
                    return Ok(item);
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
                Err(error) => return Err(error),
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
    let block_header = fetch_retry_forever::<BlockHeader>(effect_builder, block_hash).await?;
    effect_builder
        .put_block_header_to_storage(block_header.clone())
        .await;
    Ok(block_header)
}

/// Fetches and stores a deploy.
async fn fetch_and_store_deploy(
    effect_builder: EffectBuilder<JoinerEvent>,
    deploy_or_transfer_hash: DeployHash,
) -> Result<Box<Deploy>, FetcherError<Deploy, NodeId>> {
    let deploy = fetch_retry_forever::<Deploy>(effect_builder, deploy_or_transfer_hash).await?;
    effect_builder.put_deploy_to_storage(deploy.clone()).await;
    Ok(deploy)
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

/// Fetches a block header from the network by height.
async fn fetch_and_store_block_header_by_height(
    effect_builder: EffectBuilder<JoinerEvent>,
    height: u64,
    trusted_key_block_info: &KeyBlockInfo,
    finality_threshold_fraction: Ratio<u64>,
) -> Result<Option<Box<BlockHeaderWithMetadata>>, FetcherError<BlockHeaderWithMetadata, NodeId>> {
    for peer in effect_builder.get_peers_in_random_order().await {
        match effect_builder
            .fetch::<BlockHeaderWithMetadata, NodeId>(height, peer)
            .await
        {
            Ok(FetchedData::FromStorage { item }) => return Ok(Some(item)),
            Ok(FetchedData::FromPeer { item, .. }) => {
                let BlockHeaderWithMetadata {
                    block_header,
                    block_signatures,
                } = *item.clone();

                if let Err(error) = validate_finality_signatures(
                    &block_header,
                    trusted_key_block_info,
                    finality_threshold_fraction,
                    &block_signatures,
                ) {
                    warn!(
                        ?error,
                        ?peer,
                        "Error validating finality signatures from peer.",
                    );
                    effect_builder.announce_disconnect_from_peer(peer).await;
                    continue;
                }

                // Store the block header
                effect_builder
                    .put_block_header_to_storage(Box::new(block_header.clone()))
                    .await;

                // Store the finality signatures
                effect_builder
                    .put_signatures_to_storage(block_signatures.clone())
                    .await;

                return Ok(Some(item));
            }
            Err(FetcherError::Absent { .. }) => {
                warn!(
                    height,
                    tag = ?BlockHeaderWithMetadata::TAG,
                    ?peer,
                    "Fast sync could not fetch",
                );
                // If the peer we requested doesn't have the item, continue with the next peer
                continue;
            }
            Err(FetcherError::TimedOut { .. }) => {
                warn!(
                    height,
                    tag = ?BlockHeaderWithMetadata::TAG,
                    ?peer,
                    "Peer timed out",
                );
                // Peer timed out fetching the item, continue with the next peer
                continue;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

/// Key block info used for verifying finality signatures.
///
/// Can come from either:
///   - A switch block
///   - The global state under a genesis block
///
/// If the data was scraped from genesis, then `era_id` is 0.
/// Otherwise if it came from a switch block it is that switch's block `era_id + 1`.
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

/// Get the trusted key block info for genesis.
async fn get_genesis_trusted_key_block_info(
    effect_builder: EffectBuilder<JoinerEvent>,
    mut trusted_header: BlockHeader,
) -> Result<KeyBlockInfo, LinearChainSyncError> {
    // Get the genesis block header
    while trusted_header.height() != 0 {
        let block =
            fetch_and_store_block_by_hash(effect_builder, *trusted_header.parent_hash()).await?;
        trusted_header = block.take_header();
    }

    // Otherwise, sync the global trie state under genesis to get the genesis validators
    let state_root_hash = trusted_header.state_root_hash();
    sync_trie_store(effect_builder, *state_root_hash).await?;
    let mut era_validators = effect_builder
        .get_era_validators_from_contract_runtime(EraValidatorsRequest::new(
            Blake2bHash::from(*state_root_hash),
            trusted_header.protocol_version(),
        ))
        .await?;
    let genesis_validators = match era_validators.remove(&EraId::new(0)) {
        Some(genesis_validators) => genesis_validators,
        None => {
            return Err(
                LinearChainSyncError::GenesisGlobalStateDidNotHaveGenesisValidators {
                    genesis_block_header: Box::new(trusted_header),
                    era_validators,
                },
            )
        }
    };

    Ok(KeyBlockInfo {
        key_block_hash: trusted_header.hash(),
        validator_weights: genesis_validators,
        era_start: trusted_header.timestamp(),
        height: 0,
        era_id: EraId::new(0),
    })
}

/// Get the trusted key block info for a trusted block header.
async fn get_trusted_key_block_info(
    effect_builder: EffectBuilder<JoinerEvent>,
    chainspec: &Chainspec,
    trusted_header: &BlockHeader,
) -> Result<KeyBlockInfo, LinearChainSyncError> {
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
            break Ok(key_block_info);
        }

        if current_header_to_walk_back_from.height() == 0 {
            return get_genesis_trusted_key_block_info(
                effect_builder,
                current_header_to_walk_back_from,
            )
            .await;
        }

        current_header_to_walk_back_from = *fetch_and_store_block_header(
            effect_builder,
            *current_header_to_walk_back_from.parent_hash(),
        )
        .await?;
    }
}

/// Fetches a block from the network by height.
async fn fetch_and_store_block_by_height(
    effect_builder: EffectBuilder<JoinerEvent>,
    height: u64,
    trusted_key_block_info: &KeyBlockInfo,
    finality_threshold_fraction: Ratio<u64>,
) -> Result<Option<Box<BlockWithMetadata>>, FetcherError<BlockWithMetadata, NodeId>> {
    for peer in effect_builder.get_peers_in_random_order().await {
        match effect_builder
            .fetch::<BlockWithMetadata, NodeId>(height, peer)
            .await
        {
            Ok(FetchedData::FromStorage { item }) => return Ok(Some(item)),
            Ok(FetchedData::FromPeer { item, .. }) => {
                let BlockWithMetadata {
                    block,
                    finality_signatures,
                } = &*item;

                if let Err(error) = validate_finality_signatures(
                    block.header(),
                    trusted_key_block_info,
                    finality_threshold_fraction,
                    finality_signatures,
                ) {
                    warn!(
                        ?error,
                        ?peer,
                        "Error validating finality signatures from peer.",
                    );
                    effect_builder.announce_disconnect_from_peer(peer).await;
                    continue;
                }

                // Store the block
                effect_builder
                    .put_block_to_storage(Box::new(block.clone()))
                    .await;

                // Store the finality signatures
                effect_builder
                    .put_signatures_to_storage(finality_signatures.clone())
                    .await;

                return Ok(Some(item));
            }
            Err(FetcherError::Absent { .. }) => {
                warn!(
                    height,
                    tag = ?BlockWithMetadata::TAG,
                    ?peer,
                    "Block by height absent from peer",
                );
                // If the peer we requested doesn't have the item, continue with the next peer
                continue;
            }
            Err(FetcherError::TimedOut { .. }) => {
                warn!(
                    height,
                    tag = ?BlockWithMetadata::TAG,
                    ?peer,
                    "Peer timed out",
                );
                // Peer timed out fetching the item, continue with the next peer
                continue;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

/// Queries all of the peers for a trie, puts the trie found from the network in the trie-store, and
/// returns any outstanding descendant tries.
async fn fetch_trie_and_insert_into_trie_store(
    effect_builder: EffectBuilder<JoinerEvent>,
    trie_key: Blake2bHash,
) -> Result<Vec<Blake2bHash>, LinearChainSyncError> {
    let trie = fetch_retry_forever::<Trie<Key, StoredValue>>(effect_builder, trie_key).await?;
    let outstanding_tries = effect_builder
        .put_trie_and_find_missing_descendant_trie_keys(trie)
        .await?;
    Ok(outstanding_tries)
}

/// Downloads and stores a block.
async fn fetch_and_store_block_by_hash(
    effect_builder: EffectBuilder<JoinerEvent>,
    block_hash: BlockHash,
) -> Result<Box<Block>, FetcherError<Block, NodeId>> {
    let block = fetch_retry_forever::<Block>(effect_builder, block_hash).await?;
    effect_builder.put_block_to_storage(block.clone()).await;
    Ok(block)
}

/// Synchronize the trie store under a given state root hash.
async fn sync_trie_store(
    effect_builder: EffectBuilder<JoinerEvent>,
    state_root_hash: Digest,
) -> Result<(), LinearChainSyncError> {
    info!(
        state_root_hash = ?state_root_hash,
        "Syncing trie store",
    );
    let mut outstanding_trie_keys = vec![Blake2bHash::from(state_root_hash)];
    while let Some(trie_key) = outstanding_trie_keys.pop() {
        let missing_descendant_trie_keys =
            fetch_trie_and_insert_into_trie_store(effect_builder, trie_key).await?;
        outstanding_trie_keys.extend(missing_descendant_trie_keys);
    }
    Ok(())
}

/// Fetches the current header and fast-syncs too it.
///
/// Performs the following:
///
///  1. Fetches the most recent block header.
///  2. Fetches deploys for replay attacks and historical switch block info.
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
    let current_version = chainspec.protocol_config.version;
    let mut most_recent_block_header = trusted_block_header;
    loop {
        // If we encounter a block header of a version which is newer than ours we return an error
        if most_recent_block_header.protocol_version() > current_version {
            return Err(
                LinearChainSyncError::RetrievedBlockHeaderFromFutureVersion {
                    current_version,
                    block_header_with_future_version: Box::new(most_recent_block_header),
                },
            );
        }
        let maybe_fetched_block = fetch_and_store_block_header_by_height(
            effect_builder,
            most_recent_block_header.height() + 1,
            &trusted_key_block_info,
            chainspec.highway_config.finality_threshold_fraction,
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
            // If we could not fetch, we can stop when the most recent header:
            // 1. has our protocol version
            // 2. is in the current era
            None if most_recent_block_header.protocol_version() == current_version
                && is_current_era(
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
    let mut trusted_key_block_info =
        get_genesis_trusted_key_block_info(effect_builder, trusted_block_header.clone()).await?;

    let genesis_block =
        *fetch_and_store_block_by_hash(effect_builder, trusted_key_block_info.key_block_hash)
            .await?;
    sync_deploys_and_transfers_and_state(effect_builder, &genesis_block).await?;

    let mut most_recent_block = genesis_block;
    while most_recent_block.header().protocol_version() < chainspec.protocol_config.version {
        let maybe_fetched_block_with_metadata = fetch_and_store_block_by_height(
            effect_builder,
            most_recent_block.header().height() + 1,
            &trusted_key_block_info,
            chainspec.highway_config.finality_threshold_fraction,
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

    let maybe_last_emergency_restart_era_id = chainspec.protocol_config.last_emergency_restart;
    match maybe_last_emergency_restart_era_id {
        Some(last_emergency_restart_era)
            if last_emergency_restart_era > trusted_block_header.era_id() =>
        {
            return Err(
                LinearChainSyncError::TryingToJoinBeforeLastEmergencyRestartEra {
                    last_emergency_restart_era,
                    trusted_hash,
                    trusted_block_header,
                },
            );
        }
        _ => {}
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
        let block = match fetch_and_store_block_by_height(
            effect_builder,
            most_recent_block_header.height() + 1,
            &trusted_key_block_info,
            chainspec.highway_config.finality_threshold_fraction,
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
        if block.protocol_version() > chainspec.protocol_config.version {
            return Err(
                LinearChainSyncError::RetrievedBlockHeaderFromFutureVersion {
                    current_version: chainspec.protocol_config.version,
                    block_header_with_future_version: Box::new(block.take_header()),
                },
            );
        }

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

    use casper_types::{EraId, PublicKey, SecretKey};

    use crate::{
        components::consensus::EraReport,
        crypto::AsymmetricKeyExt,
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
}
