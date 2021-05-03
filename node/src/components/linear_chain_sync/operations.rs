use std::{
    collections::BTreeMap,
    fmt::Debug,
    ops::{Add, Div},
    time::Duration,
};

use num::rational::Ratio;
use rand::seq::SliceRandom;
use tracing::{info, trace, warn};

use casper_execution_engine::{
    shared::{newtypes::Blake2bHash, stored_value::StoredValue},
    storage::trie::Trie,
};
use casper_types::{EraId, Key, PublicKey, U512};

use crate::{
    components::{
        consensus,
        fetcher::{FetchedData, FetcherError},
        linear_chain_sync::error::{FinalitySignatureError, LinearChainSyncError},
    },
    effect::{
        requests::{ContractRuntimeRequest, FetcherRequest, NetworkInfoRequest, StorageRequest},
        EffectBuilder,
    },
    types::{BlockHash, BlockHeader, BlockHeaderWithMetadata, BlockSignatures, Chainspec, Item},
};

const TIMEOUT_DURATION: Duration = Duration::from_millis(100);

/// Gets the currently connected peers from the networking component and shuffles them.
async fn get_and_shuffle_network_peers<REv, I>(effect_builder: EffectBuilder<REv>) -> Vec<I>
where
    REv: From<NetworkInfoRequest<I>>,
    I: Send + 'static,
{
    let mut vector_of_peers: Vec<I> = effect_builder
        .network_peers::<I>()
        .await
        .into_iter()
        .map(|(peer, _)| peer)
        .collect();
    let mut rng = rand::thread_rng();
    vector_of_peers.shuffle(&mut rng);
    vector_of_peers
}

/// Fetches an item. Keeps retrying to fetch until it is successful. Assumes no integrity check is
/// necessary for the item. Not suited to fetching a block header or block by height, which require
/// verification with finality signatures.
async fn fetch_retry_forever<T, REv, I>(
    effect_builder: EffectBuilder<REv>,
    id: T::Id,
) -> Result<Box<T>, FetcherError<T, I>>
where
    T: Item + 'static,
    REv: From<FetcherRequest<I, T>> + From<NetworkInfoRequest<I>>,
    I: Eq + Debug + Clone + Send + 'static,
{
    loop {
        for peer in get_and_shuffle_network_peers(effect_builder).await {
            trace!(
                "Attempting to fetch {:?} with id {:?} from {:?}",
                T::TAG,
                id,
                peer
            );
            match effect_builder.fetch::<T, I>(id, peer.clone()).await {
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
                Err(FetcherError::AbsentFromPeer { .. }) => {
                    warn!(
                        ?id,
                        tag = ?T::TAG,
                        ?peer,
                        "Fast sync could not fetch; trying next peer",
                    )
                }
                Err(error) => return Err(error),
            }
        }
        tokio::time::sleep(TIMEOUT_DURATION).await
    }
}

/// Fetches and stores a block header from the network
async fn fetch_and_store_block_header<REv, I>(
    effect_builder: EffectBuilder<REv>,
    block_hash: BlockHash,
) -> Result<Box<BlockHeader>, FetcherError<BlockHeader, I>>
where
    REv: From<FetcherRequest<I, BlockHeader>> + From<NetworkInfoRequest<I>> + From<StorageRequest>,
    I: Eq + Debug + Clone + Send + 'static,
{
    let block_header =
        fetch_retry_forever::<BlockHeader, REv, I>(effect_builder, block_hash).await?;
    effect_builder
        .put_block_header_to_storage(block_header.clone())
        .await;
    Ok(block_header)
}

/// Get trusted block headers; falls back to genesis if none are available
async fn get_trusted_validator_weights<REv, I>(
    effect_builder: EffectBuilder<REv>,
    chainspec: &Chainspec,
    trusted_header: &BlockHeader,
) -> Result<BTreeMap<PublicKey, U512>, LinearChainSyncError<I>>
where
    REv: From<FetcherRequest<I, BlockHeader>> + From<NetworkInfoRequest<I>> + From<StorageRequest>,
    I: Eq + Debug + Clone + Send + 'static,
{
    // If we are right after genesis, use the genesis validators.
    if trusted_header.era_id() == EraId::new(0) {
        let genesis_validator_weights: BTreeMap<PublicKey, U512> = chainspec
            .network_config
            .accounts_config
            .accounts()
            .iter()
            .filter_map(|account_config| {
                if account_config.is_genesis_validator() {
                    Some((
                        account_config.public_key(),
                        account_config.bonded_amount().value(),
                    ))
                } else {
                    None
                }
            })
            .collect();
        return Ok(genesis_validator_weights);
    }

    // Check that we are not restarting right after an emergency restart, which is too early
    // Consider emitting a switch block after an emergency restart to make this simpler...
    let maybe_last_emergency_restart_era_id = chainspec.protocol_config.last_emergency_restart;

    let min_era = maybe_last_emergency_restart_era_id.unwrap_or_else(|| EraId::new(0));
    if min_era >= trusted_header.era_id() {
        return Err(LinearChainSyncError::TrustedHeaderEraTooEarly {
            trusted_header: Box::new(trusted_header.clone()),
            maybe_last_emergency_restart_era_id,
        });
    };

    // Fetch each parent hash one by one until we have the trusted validator weights
    let mut current_header_to_walk_back_from = Box::new(trusted_header.clone());
    loop {
        if let Some(trusted_validator_weights) =
            current_header_to_walk_back_from.next_era_validator_weights()
        {
            return Ok(trusted_validator_weights.clone());
        }
        current_header_to_walk_back_from = fetch_and_store_block_header(
            effect_builder,
            *current_header_to_walk_back_from.parent_hash(),
        )
        .await?;
    }
}

/// Verifies finality signatures for a block header
fn validate_finality_signatures(
    block_header: &BlockHeader,
    trusted_validator_weights: &BTreeMap<PublicKey, U512>,
    finality_threshold_fraction: Ratio<u64>,
    block_signatures: &BlockSignatures,
) -> Result<(), FinalitySignatureError> {
    // Check the signatures' block hash is the header's block hash
    let block_hash = block_header.hash();
    if block_signatures.block_hash != block_hash {
        return Err(
            FinalitySignatureError::SignaturesDoNotCorrespondToBlockHeader {
                block_header: Box::new(block_header.clone()),
                block_hash: Box::new(block_hash),
                block_signatures: Box::new(block_signatures.clone()),
            },
        );
    }

    // Cryptographically verify block signatures
    block_signatures.verify()?;

    // Calculate the weight of the signatures
    let mut signature_weight: U512 = U512::zero();
    for (public_key, _) in block_signatures.proofs.iter() {
        match trusted_validator_weights.get(public_key) {
            None => {
                return Err(FinalitySignatureError::BogusValidator {
                    trusted_validator_weights: trusted_validator_weights.clone(),
                    block_signatures: Box::new(block_signatures.clone()),
                    bogus_validator_public_key: Box::new(public_key.clone()),
                })
            }
            Some(validator_weight) => {
                signature_weight += *validator_weight;
            }
        }
    }

    // Check the finality signatures have sufficient weight
    let total_weight: U512 = trusted_validator_weights
        .iter()
        .map(|(_, weight)| *weight)
        .sum();

    let lower_bound = finality_threshold_fraction.add(1).div(2);
    // Verify: signature_weight / total_weight >= lower_bound
    // Equivalent to the following
    if signature_weight * U512::from(*lower_bound.denom())
        <= total_weight * U512::from(*lower_bound.numer())
    {
        return Err(FinalitySignatureError::InsufficientWeightForFinality {
            trusted_validator_weights: trusted_validator_weights.clone(),
            block_signatures: Box::new(block_signatures.clone()),
            signature_weight: Box::new(signature_weight),
            total_validator_weight: Box::new(total_weight),
            finality_threshold_fraction,
        });
    }

    Ok(())
}

/// Fetches a block header from the network by height.
async fn fetch_and_store_block_header_by_height<REv, I>(
    effect_builder: EffectBuilder<REv>,
    height: u64,
    trusted_validator_weights: &BTreeMap<PublicKey, U512>,
    finality_threshold_fraction: Ratio<u64>,
) -> Result<Option<Box<BlockHeaderWithMetadata>>, FetcherError<BlockHeaderWithMetadata, I>>
where
    REv: From<FetcherRequest<I, BlockHeaderWithMetadata>>
        + From<NetworkInfoRequest<I>>
        + From<StorageRequest>,
    I: Eq + Debug + Clone + Send + 'static,
{
    for peer in get_and_shuffle_network_peers(effect_builder).await {
        match effect_builder
            .fetch::<BlockHeaderWithMetadata, I>(height, peer.clone())
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
                    &trusted_validator_weights,
                    finality_threshold_fraction,
                    &block_signatures,
                ) {
                    warn!(
                        ?error,
                        ?peer,
                        "Error validating finality signatures from peer.",
                    );
                    // TODO: ban peer
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
            Err(FetcherError::AbsentFromPeer { .. }) => {
                warn!(
                    height,
                    tag = ?BlockHeaderWithMetadata::TAG,
                    ?peer,
                    "Fast sync could not fetch",
                );
                // If the peer we requested doesn't have the item, continue with the next peer
                continue;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

/// Queries all of the peers for a trie, puts the trie found from the network in the trie-store, and
/// returns any outstanding descendant tries.
async fn fetch_trie_and_insert_into_trie_store<REv, I>(
    effect_builder: EffectBuilder<REv>,
    trie_key: Blake2bHash,
) -> Result<Vec<Blake2bHash>, LinearChainSyncError<I>>
where
    REv: From<ContractRuntimeRequest>
        + From<FetcherRequest<I, Trie<Key, StoredValue>>>
        + From<NetworkInfoRequest<I>>,
    I: Eq + Clone + Debug + Send + 'static,
{
    let trie =
        fetch_retry_forever::<Trie<Key, StoredValue>, REv, I>(effect_builder, trie_key).await?;
    let outstanding_tries = effect_builder
        .put_trie_and_find_missing_descendant_trie_keys(trie)
        .await?;
    return Ok(outstanding_tries);
}

/// Runs the fast synchronization task.
pub(crate) async fn run_fast_sync_task<REv, I>(
    effect_builder: EffectBuilder<REv>,
    trusted_hash: BlockHash,
    chainspec: Chainspec,
) -> Result<(), LinearChainSyncError<I>>
where
    REv: From<ContractRuntimeRequest>
        + From<FetcherRequest<I, BlockHeader>>
        + From<FetcherRequest<I, BlockHeaderWithMetadata>>
        + From<FetcherRequest<I, Trie<Key, StoredValue>>>
        + From<NetworkInfoRequest<I>>
        + From<StorageRequest>,
    I: Eq + Debug + Clone + Send + 'static,
{
    // Fetch the trusted header
    let trusted_header = fetch_and_store_block_header(effect_builder, trusted_hash).await?;

    let mut trusted_validator_weights =
        get_trusted_validator_weights(effect_builder, &chainspec, &trusted_header).await?;

    // Get the most recent header which has the same version as ours
    // We keep fetching by height until none of our peers have a block at that height
    let mut most_recent_block_header = *trusted_header;
    loop {
        // If we encounter a block header of a version which is newer than ours we return an error
        if most_recent_block_header.protocol_version() > chainspec.protocol_config.version {
            return Err(
                LinearChainSyncError::RetrievedBlockHeaderFromFutureVersion {
                    current_version: chainspec.protocol_config.version,
                    block_header_with_future_version: Box::new(most_recent_block_header),
                },
            );
        }
        let maybe_fetched_block = fetch_and_store_block_header_by_height(
            effect_builder,
            most_recent_block_header.height() + 1,
            &trusted_validator_weights,
            chainspec.highway_config.finality_threshold_fraction,
        )
        .await?;
        match maybe_fetched_block {
            Some(more_recent_block_header_with_metadata) => {
                most_recent_block_header = more_recent_block_header_with_metadata.block_header;
                // If the new block is a switch block, update the validator weights
                if let Some(new_trusted_validator_weights) =
                    most_recent_block_header.next_era_validator_weights()
                {
                    trusted_validator_weights = new_trusted_validator_weights.clone();
                }
            }
            // If we could not fetch, we can stop if the most recent has our protocol version
            None if most_recent_block_header.protocol_version()
                == chainspec.protocol_config.version =>
            {
                break
            }
            // Otherwise keep trying to fetch until we get a block with our version
            None => {
                tokio::time::sleep(TIMEOUT_DURATION).await;
            }
        }
    }

    // Synchronize the trie store for the most recent block header.

    // Corner case: if an emergency restart happened recently, it is necessary to synchronize the
    // state for the block right after the emergency restart.  This is needed for the EraSupervisor.

    // The era supervisor needs validator information from previous eras it may potentially slash.
    // The number of previous eras is determined by a *delay* in which consensus participants become
    // bonded validators or unbond.
    let delay = consensus::bonded_eras(&(&chainspec).into());
    // The era supervisor requires at least to 3*delay + 1 eras back to be stored in the database.
    let historical_eras_needed = delay.saturating_mul(3).saturating_add(1);

    let maybe_last_emergency_restart_era_id = chainspec.protocol_config.last_emergency_restart;
    match maybe_last_emergency_restart_era_id {
        Some(last_emergency_restart_era_id)
            if last_emergency_restart_era_id
                > most_recent_block_header
                    .era_id()
                    .saturating_sub(historical_eras_needed) =>
        {
            // Walk backwards until we have the first block after the emergency upgrade
            loop {
                let previous_block_header = fetch_and_store_block_header(
                    effect_builder,
                    *most_recent_block_header.parent_hash(),
                )
                .await?;
                if previous_block_header.era_id() == last_emergency_restart_era_id.saturating_sub(1)
                    && previous_block_header.is_switch_block()
                {
                    break;
                }
                most_recent_block_header = *previous_block_header;
            }
        }
        _ => {}
    }

    // Use the state root to synchronize the trie.
    info!(
        state_root_hash = ?most_recent_block_header.state_root_hash(),
        "Fast syncing",
    );
    let mut outstanding_trie_keys = vec![Blake2bHash::from(
        *most_recent_block_header.state_root_hash(),
    )];
    while let Some(trie_key) = outstanding_trie_keys.pop() {
        let missing_descendant_trie_keys =
            fetch_trie_and_insert_into_trie_store(effect_builder, trie_key).await?;
        outstanding_trie_keys.extend(missing_descendant_trie_keys);
    }

    Ok(())
}
