use std::{fmt::Debug, time::Duration};

use rand::seq::SliceRandom;
use std::collections::BTreeMap;
use tracing::{trace, warn};

use casper_execution_engine::{
    shared::{newtypes::Blake2bHash, stored_value::StoredValue},
    storage::trie::Trie,
};
use casper_types::{EraId, Key, PublicKey, U512};

use crate::{
    components::{
        consensus,
        fetcher::{FetchedData, FetcherError},
        linear_chain_sync::error::LinearChainSyncError,
    },
    effect::{
        requests::{ContractRuntimeRequest, FetcherRequest, NetworkInfoRequest},
        EffectBuilder,
    },
    new_rng,
    types::{BlockHash, BlockHeader, BlockHeaderWithMetadata, Chainspec, Item},
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
    let mut rng = new_rng();
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
                        "Fast sync could not fetch {:?} with id {:?} from peer {:?}; trying next peer",
                        T::TAG,
                        id,
                        peer
                    )
                }
                Err(error) => return Err(error),
            }
        }
        tokio::time::sleep(TIMEOUT_DURATION).await
    }
}

/// Fetches a block header from the network by height.
async fn fetch_block_header_by_height<REv, I>(
    effect_builder: EffectBuilder<REv>,
    height: u64,
    trusted_validator_weights: &BTreeMap<PublicKey, U512>,
) -> Result<Option<Box<BlockHeaderWithMetadata>>, FetcherError<BlockHeaderWithMetadata, I>>
where
    REv: From<FetcherRequest<I, BlockHeaderWithMetadata>> + From<NetworkInfoRequest<I>>,
    I: Eq + Debug + Clone + Send + 'static,
{
    for peer in get_and_shuffle_network_peers(effect_builder).await {
        match effect_builder
            .fetch::<BlockHeaderWithMetadata, I>(height, peer.clone())
            .await
        {
            Ok(FetchedData::FromStorage { item }) => return Ok(Some(item)),
            Ok(FetchedData::FromPeer { item, .. }) => {
                // TODO: Validate item, ban peer if necessary
                // Compute the total weight of the validators
                let _total_weight: U512 = trusted_validator_weights
                    .iter()
                    .map(|(_, weight)| *weight)
                    .sum();
                return Ok(Some(item));
            }
            Err(FetcherError::AbsentFromPeer { .. }) => {
                warn!(
                    "Fast sync could not fetch {:?} with id {:?} from peer {:?}; trying next peer",
                    BlockHeaderWithMetadata::TAG,
                    height,
                    peer
                )
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
        + From<NetworkInfoRequest<I>>,
    I: Eq + Debug + Clone + Send + 'static,
{
    // Fetch the trusted header
    // TODO: save header after successful fetch
    let trusted_header =
        fetch_retry_forever::<BlockHeader, REv, I>(effect_builder, trusted_hash).await?;

    // Walk back to get validators from the last switch block header
    // First, check that we are at least one era past genesis or the last emergency restart
    let maybe_last_emergency_restart_era_id = chainspec.protocol_config.last_emergency_restart;
    let min_era = maybe_last_emergency_restart_era_id.unwrap_or_else(|| EraId::new(0));
    if min_era >= trusted_header.era_id() {
        return Err(LinearChainSyncError::TrustedHeaderEraTooEarly {
            trusted_header,
            maybe_last_emergency_restart_era_id,
        });
    };

    // Fetch each parent hash one by one until we have the trusted validator weights
    let mut current_walk_back_header = trusted_header.clone();
    let mut trusted_validator_weights = loop {
        if let Some(trusted_validator_weights) =
            current_walk_back_header.next_era_validator_weights()
        {
            break trusted_validator_weights.clone();
        }
        current_walk_back_header = fetch_retry_forever::<BlockHeader, REv, I>(
            effect_builder,
            *current_walk_back_header.parent_hash(),
        )
        .await?;
    };

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
        let maybe_fetched_block = fetch_block_header_by_height(
            effect_builder,
            most_recent_block_header.height() + 1,
            &trusted_validator_weights,
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

    match maybe_last_emergency_restart_era_id {
        Some(last_emergency_restart_era_id)
            if last_emergency_restart_era_id
                > most_recent_block_header
                    .era_id()
                    .saturating_sub(historical_eras_needed) =>
        {
            // Walk backwards until we have the first block after the emergency upgrade
            loop {
                let previous_block_header = fetch_retry_forever::<BlockHeader, REv, I>(
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

    // Use the state root to synchronize the trie

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
