use std::{fmt::Debug, time::Duration};

use rand::seq::SliceRandom;
use tracing::info;

use casper_execution_engine::{
    shared::{newtypes::Blake2bHash, stored_value::StoredValue},
    storage::trie::Trie,
};
use casper_types::Key;

use crate::{
    components::{
        consensus::era_supervisor, fetcher::FetchResult,
        linear_chain_fast_sync::error::FastSyncError,
    },
    effect::{
        requests::{ContractRuntimeRequest, FetcherRequest, NetworkInfoRequest},
        EffectBuilder,
    },
    types::{BlockHash, BlockHeader, BlockHeaderAndMetadata, Chainspec, Item},
};

const TIMEOUT_DURATION: Duration = Duration::from_millis(2000);
const RETRY_COUNT: u8 = 5;

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

/// Fetch a block header from the network using its block hash.
async fn fetch_block_header_by_block_hash<REv, I>(
    effect_builder: EffectBuilder<REv>,
    block_hash: BlockHash,
) -> Result<Box<BlockHeader>, FastSyncError>
where
    REv: From<FetcherRequest<I, BlockHeader>> + From<NetworkInfoRequest<I>>,
    I: Debug + Clone + Send + 'static,
{
    for _ in 0..RETRY_COUNT {
        for peer in get_and_shuffle_network_peers(effect_builder).await {
            match effect_builder
                .fetch_block_header::<I>(block_hash, peer.clone())
                .await
            {
                Some(FetchResult::FromStorage(block_header)) => return Ok(block_header),
                Some(FetchResult::FromPeer(block_header, peer)) => {
                    if block_hash != block_header.hash() {
                        // effect_builder.announce_disconnect_from_peer(peer).await
                        todo!("disconnect from peer");
                    } else {
                        return Ok(block_header);
                    }
                }
                None => {
                    info!(
                        "Could not retrieve block header with hash {} from peer {:?}",
                        block_hash, &peer
                    )
                }
            }
        }
        tokio::time::delay_for(TIMEOUT_DURATION).await;
    }
    Err(FastSyncError::RanOutOfHeaderByHashFetchRetries { block_hash })
}

/// Fetch a block header from the network using its block hash.
async fn fetch_block_header_by_height<REv, I>(
    effect_builder: EffectBuilder<REv>,
    height: u64,
) -> Result<Box<BlockHeaderAndMetadata>, FastSyncError>
where
    REv: From<FetcherRequest<I, BlockHeaderAndMetadata>> + From<NetworkInfoRequest<I>>,
    I: Debug + Clone + Send + 'static,
{
    for _ in 0..RETRY_COUNT {
        for peer in get_and_shuffle_network_peers(effect_builder).await {
            match effect_builder
                .fetch_block_header_by_height::<I>(height, peer.clone())
                .await
            {
                Some(FetchResult::FromStorage(block_header_with_metadata)) => {
                    return Ok(block_header_with_metadata)
                }
                Some(FetchResult::FromPeer(block_header_with_metadata, peer)) => {
                    // TODO: check finality signatures
                    if block_header_with_metadata.block_header.height() != height {
                        // effect_builder.announce_disconnect_from_peer(peer).await
                        todo!("disconnect from peer")
                    } else {
                        return Ok(block_header_with_metadata);
                    }
                }
                None => {
                    info!(
                        "Could not retrieve block at height {} from peer {:?}",
                        height, peer
                    )
                }
            }
        }
        tokio::time::delay_for(TIMEOUT_DURATION).await;
    }
    Err(FastSyncError::RanOutOfHeaderByHeightFetchRetries { height })
}

/// Query all of the peers for a trie, put the trie found from the network
/// in the trie-store, and return any outstanding descendant tries.
async fn fetch_trie_and_insert_into_trie_store<REv, I>(
    effect_builder: EffectBuilder<REv>,
    trie_key: Blake2bHash,
) -> Result<Vec<Blake2bHash>, FastSyncError>
where
    REv: From<ContractRuntimeRequest>
        + From<FetcherRequest<I, Trie<Key, StoredValue>>>
        + From<NetworkInfoRequest<I>>,
    I: Clone + Debug + Send + 'static,
{
    for _ in 0..RETRY_COUNT {
        for peer in get_and_shuffle_network_peers(effect_builder).await {
            info!("Retrieving trie key {} from peer {:?}", trie_key, peer);
            let trie = match effect_builder.fetch_trie(trie_key, peer.clone()).await {
                Some(FetchResult::FromStorage(trie)) => trie,
                Some(FetchResult::FromPeer(trie, peer)) => {
                    if trie_key != trie.id() {
                        todo!("disconnect from peer");
                        // continue;
                    } else {
                        trie
                    }
                }
                None => {
                    info!(
                        "Could not retrieve trie with key {} from peer {:?}",
                        trie_key, peer
                    );
                    continue;
                }
            };
            let outstanding_tries = effect_builder
                .put_trie_and_find_missing_descendant_trie_keys(trie)
                .await?;
            return Ok(outstanding_tries);
        }
        tokio::time::delay_for(TIMEOUT_DURATION).await;
    }
    Err(FastSyncError::RanOutOfFetchTrieRetries { trie_key })
}

struct EraWalkBackInfo {
    earliest_era_number_to_get_header_for: u64,
    must_sync_as_low_as_possible: bool,
}

fn earliest_era_to_get_header_for(
    latest_trusted_hash_era_number: u64,
    chainspec: &Chainspec,
) -> EraWalkBackInfo {
    // The era supervisor needs validator information from previous eras it may potentially slash.
    // The number of previous eras is determined by a *delay* in which consensus participants become
    // bonded validators or unbond.

    let delay = era_supervisor::bonded_eras(&chainspec.into());

    // The era supervisor may slash up to 3*delay + 1 eras back
    let historical_eras_needed = delay.saturating_mul(3).saturating_add(1);

    // We have a candidate lower bound for how far back we need to query block headers for
    let era_number_lower_bound =
        latest_trusted_hash_era_number.saturating_sub(historical_eras_needed);

    // It is not always possible to get era validators from headers.
    // This can happen if there was a hard reset or we are at genesis.

    // In those cases, if the era_lower_bound is too low, we will need to sync the state trie
    // to the block right after the hard reset / genesis.

    if chainspec.protocol_config.hard_reset {
        // If there was a hard reset, we cannot sync at an era lower than the activation point.
        // Hence, we take the maximum of the activation point era and the
        let hard_reset_era_number =
            <u64>::from(chainspec.protocol_config.activation_point.era_id()).saturating_sub(1);
        let era_number_to_walk_back_to = era_number_lower_bound.max(hard_reset_era_number);
        EraWalkBackInfo {
            earliest_era_number_to_get_header_for: era_number_to_walk_back_to,
            // If the era number to walk back to is the hard reset era number, we will need to
            // sync the block right after the hard reset to get the era validators.
            must_sync_as_low_as_possible: era_number_to_walk_back_to == hard_reset_era_number,
        }
    } else {
        EraWalkBackInfo {
            earliest_era_number_to_get_header_for: era_number_lower_bound,
            must_sync_as_low_as_possible: false,
        }
    }
}

/// Run the fast synchronization task.
pub(crate) async fn run_fast_sync_task<REv, I>(
    effect_builder: EffectBuilder<REv>,
    trusted_hash: BlockHash,
) -> Result<(), FastSyncError>
where
    REv: From<ContractRuntimeRequest>
        + From<FetcherRequest<I, BlockHeader>>
        + From<FetcherRequest<I, Trie<Key, StoredValue>>>
        + From<NetworkInfoRequest<I>>,
    I: Debug + Clone + Send + 'static,
{
    let trusted_header =
        fetch_block_header_by_block_hash::<REv, I>(effect_builder, trusted_hash).await?;

    // TODO: walk back and get validators from switch block header

    // TODO: If the era_id is 0 and must_sync_as_low_as_possible, sync the block at height 0
    // Otherwise sync the first block with in that era

    // TODO: Check that trusted header has era > last_restart_or_genesis_era + 1
    // TODO: If latest header has version greater than ours, crash
    let mut outstanding_trie_keys = vec![Blake2bHash::from(*trusted_header.state_root_hash())];
    while let Some(trie_key) = outstanding_trie_keys.pop() {
        let missing_descendant_trie_keys =
            fetch_trie_and_insert_into_trie_store(effect_builder, trie_key).await?;
        outstanding_trie_keys.extend(missing_descendant_trie_keys);
    }
    Ok(())
}
