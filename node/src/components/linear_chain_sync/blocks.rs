use tracing::warn;

use crate::{
    components::{fetcher::FetchResult, linear_chain_sync::event::BlockByHeightResult},
    contract_runtime::{
        announcements, BlockAndExecutionEffects, ContractRuntimeAnnouncement, ExecutionPreState,
    },
    effect::{
        announcements::ControlAnnouncement,
        requests::{ContractRuntimeRequest, StorageRequest},
        EffectBuilder, EffectOptionExt, Effects,
    },
    fatal,
    types::{Block, BlockByHeight, BlockHash, Deploy, FinalizedBlock},
};

use super::{event::BlockByHashResult, Event, ReactorEventT};

pub(super) fn fetch_block_by_hash<I: Clone + Send + 'static, REv>(
    effect_builder: EffectBuilder<REv>,
    peer: I,
    block_hash: BlockHash,
) -> Effects<Event<I>>
where
    REv: ReactorEventT<I>,
{
    let cloned = peer.clone();
    effect_builder.fetch_block(block_hash, peer).map_or_else(
        move |fetch_result| match fetch_result {
            FetchResult::FromStorage(block) => {
                Event::GetBlockHashResult(block_hash, BlockByHashResult::FromStorage(block))
            }
            FetchResult::FromPeer(block, peer) => {
                Event::GetBlockHashResult(block_hash, BlockByHashResult::FromPeer(block, peer))
            }
        },
        move || Event::GetBlockHashResult(block_hash, BlockByHashResult::Absent(cloned)),
    )
}

pub(super) fn fetch_block_at_height<I: Send + Clone + 'static, REv>(
    effect_builder: EffectBuilder<REv>,
    peer: I,
    block_height: u64,
) -> Effects<Event<I>>
where
    REv: ReactorEventT<I>,
{
    let cloned = peer.clone();
    effect_builder
        .fetch_block_by_height(block_height, peer.clone())
        .map_or_else(
            move |fetch_result| match fetch_result {
                FetchResult::FromPeer(result, _) => match *result {
                    BlockByHeight::Absent(ret_height) => {
                        warn!(
                            "Fetcher returned result for invalid height. Expected {}, got {}",
                            block_height, ret_height
                        );
                        Event::GetBlockHeightResult(block_height, BlockByHeightResult::Absent(peer))
                    }
                    BlockByHeight::Block(block) => Event::GetBlockHeightResult(
                        block_height,
                        BlockByHeightResult::FromPeer(block, peer),
                    ),
                },
                FetchResult::FromStorage(result) => match *result {
                    BlockByHeight::Absent(_) => {
                        // Fetcher should try downloading the block from a peer
                        // when it can't find it in the storage.
                        panic!("Should not return `Absent` in `FromStorage`.")
                    }
                    BlockByHeight::Block(block) => Event::GetBlockHeightResult(
                        block_height,
                        BlockByHeightResult::FromStorage(block),
                    ),
                },
            },
            move || Event::GetBlockHeightResult(block_height, BlockByHeightResult::Absent(cloned)),
        )
}

pub(super) async fn execute_block<REv>(
    effect_builder: EffectBuilder<REv>,
    block_to_execute: Block,
    initial_execution_pre_state: ExecutionPreState,
) where
    REv: From<StorageRequest>
        + From<ControlAnnouncement>
        + From<ContractRuntimeRequest>
        + From<ContractRuntimeAnnouncement>,
{
    let protocol_version = block_to_execute.protocol_version();
    let execution_pre_state =
        if block_to_execute.height() == initial_execution_pre_state.next_block_height() {
            initial_execution_pre_state
        } else {
            match effect_builder
                .get_block_at_height_from_storage(block_to_execute.height() - 1)
                .await
            {
                None => {
                    fatal!(
                        effect_builder,
                        "Could not get block at height {}",
                        block_to_execute.height() - 1
                    )
                    .await;
                    return;
                }
                Some(parent_block) => ExecutionPreState::from(parent_block.header()),
            }
        };
    let finalized_block = FinalizedBlock::from(block_to_execute);

    // Get the deploy hashes for the block.
    let deploy_hashes = finalized_block.deploy_hashes().to_vec();

    // Get all deploys in order they appear in the finalized block.
    let mut deploys: Vec<Deploy> = Vec::with_capacity(deploy_hashes.len());
    for maybe_deploy in effect_builder.get_deploys_from_storage(deploy_hashes).await {
        if let Some(deploy) = maybe_deploy {
            deploys.push(deploy)
        } else {
            fatal!(
                effect_builder,
                "Could not fetch deploys for finalized block: {:?}",
                finalized_block
            )
            .await;
            return;
        }
    }

    // Get the transfer hashes for the block.
    let transfer_hashes = finalized_block.transfer_hashes().to_vec();

    // Get all deploys in order they appear in the finalized block.
    let mut transfers: Vec<Deploy> = Vec::with_capacity(transfer_hashes.len());
    for maybe_transfer in effect_builder
        .get_deploys_from_storage(transfer_hashes)
        .await
    {
        if let Some(transfer) = maybe_transfer {
            transfers.push(transfer)
        } else {
            fatal!(
                effect_builder,
                "Could not fetch deploys for finalized block: {:?}",
                finalized_block
            )
            .await;
            return;
        }
    }

    let BlockAndExecutionEffects {
        block,
        execution_results,
        ..
    } = match effect_builder
        .execute_finalized_block(
            protocol_version,
            execution_pre_state,
            finalized_block,
            deploys,
            transfers,
        )
        .await
    {
        Ok(child_block) => child_block,
        Err(error) => {
            fatal!(effect_builder, "Fatal error: {}", error).await;
            return;
        }
    };
    announcements::linear_chain_block(effect_builder, block, execution_results).await;
}
