use crate::{
    components::{fetcher, fetcher::Fetcher, Component},
    effect::{announcements::DeployAcceptorAnnouncement, EffectBuilder, Effects},
    reactor,
    reactor::participating::ParticipatingEvent,
    types::{
        Block, BlockAdded, BlockAndDeploys, BlockHeader, BlockHeaderWithMetadata,
        BlockHeadersBatch, BlockSignatures, BlockWithMetadata, Chainspec, Deploy,
        FinalizedApprovalsWithId, SyncLeap, TrieOrChunk,
    },
    FetcherConfig, NodeRng,
};
use datasize::DataSize;
use prometheus::Registry;

#[derive(DataSize, Debug)]
pub(super) struct Fetchers {
    deploy_fetcher: Fetcher<Deploy>,
    block_by_hash_fetcher: Fetcher<Block>,
    block_header_by_hash_fetcher: Fetcher<BlockHeader>,
    trie_or_chunk_fetcher: Fetcher<TrieOrChunk>,
    block_by_height_fetcher: Fetcher<BlockWithMetadata>,
    block_header_and_finality_signatures_by_height_fetcher: Fetcher<BlockHeaderWithMetadata>,
    block_and_deploys_fetcher: Fetcher<BlockAndDeploys>,
    finalized_approvals_fetcher: Fetcher<FinalizedApprovalsWithId>,
    block_headers_batch_fetcher: Fetcher<BlockHeadersBatch>,
    finality_signatures_fetcher: Fetcher<BlockSignatures>,
    sync_leap_fetcher: Fetcher<SyncLeap>,
    block_added_fetcher: Fetcher<BlockAdded>,
}

impl Fetchers {
    pub(super) fn new(
        config: &FetcherConfig,
        chainspec: &Chainspec,
        metrics_registry: &Registry,
    ) -> Result<Self, prometheus::Error> {
        Ok(Fetchers {
            // WE NEED THESE:
            deploy_fetcher: Fetcher::new("deploy", config, metrics_registry)?,
            block_by_hash_fetcher: Fetcher::new("block", config, metrics_registry)?,
            block_header_by_hash_fetcher: Fetcher::new("block_header", config, metrics_registry)?,
            trie_or_chunk_fetcher: Fetcher::new("trie_or_chunk", config, metrics_registry)?,
            sync_leap_fetcher: Fetcher::new_with_metadata(
                "sync_leap_fetcher",
                config,
                metrics_registry,
                chainspec.highway_config.finality_threshold_fraction,
            )?,
            block_added_fetcher: Fetcher::new("block_added_fetcher", config, metrics_registry)?,

            // MAYBE WE NEED THESE
            block_and_deploys_fetcher: Fetcher::new("block_and_deploys", config, metrics_registry)?,
            finalized_approvals_fetcher: Fetcher::new(
                "finalized_approvals",
                config,
                metrics_registry,
            )?,
            finality_signatures_fetcher: Fetcher::new(
                "finality_signatures",
                config,
                metrics_registry,
            )?,
            block_headers_batch_fetcher: Fetcher::new(
                "block_headers_batch",
                config,
                metrics_registry,
            )?,

            // WOULD LIKE TO DELETE THESE
            block_by_height_fetcher: Fetcher::new("block_by_height", config, metrics_registry)?,
            block_header_and_finality_signatures_by_height_fetcher: Fetcher::new(
                "block_header_by_height",
                config,
                metrics_registry,
            )?,
        })
    }

    pub(super) fn dispatch_fetcher_event(
        &mut self,
        effect_builder: EffectBuilder<ParticipatingEvent>,
        rng: &mut NodeRng,
        event: ParticipatingEvent,
    ) -> Effects<ParticipatingEvent> {
        match event {
            // BLOCK STUFF
            ParticipatingEvent::BlockFetcher(event) => reactor::wrap_effects(
                ParticipatingEvent::BlockFetcher,
                self.block_by_hash_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::BlockFetcherRequest(request) => reactor::wrap_effects(
                ParticipatingEvent::BlockFetcher,
                self.block_by_hash_fetcher
                    .handle_event(effect_builder, rng, request.into()),
            ),
            ParticipatingEvent::BlockAddedFetcher(event) => reactor::wrap_effects(
                ParticipatingEvent::BlockAddedFetcher,
                self.block_added_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::BlockAddedFetcherRequest(request) => reactor::wrap_effects(
                ParticipatingEvent::BlockAddedFetcher,
                self.block_added_fetcher
                    .handle_event(effect_builder, rng, request.into()),
            ),
            ParticipatingEvent::BlockHeaderFetcher(event) => reactor::wrap_effects(
                ParticipatingEvent::BlockHeaderFetcher,
                self.block_header_by_hash_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::BlockHeaderFetcherRequest(request) => reactor::wrap_effects(
                ParticipatingEvent::BlockHeaderFetcher,
                self.block_header_by_hash_fetcher
                    .handle_event(effect_builder, rng, request.into()),
            ),
            ParticipatingEvent::BlockAndDeploysFetcher(event) => reactor::wrap_effects(
                ParticipatingEvent::BlockAndDeploysFetcher,
                self.block_and_deploys_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::BlockAndDeploysFetcherRequest(request) => reactor::wrap_effects(
                ParticipatingEvent::BlockAndDeploysFetcher,
                self.block_and_deploys_fetcher
                    .handle_event(effect_builder, rng, request.into()),
            ),
            ParticipatingEvent::BlockHeadersBatchFetcher(event) => reactor::wrap_effects(
                ParticipatingEvent::BlockHeadersBatchFetcher,
                self.block_headers_batch_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::BlockHeadersBatchFetcherRequest(request) => reactor::wrap_effects(
                ParticipatingEvent::BlockHeadersBatchFetcher,
                self.block_headers_batch_fetcher
                    .handle_event(effect_builder, rng, request.into()),
            ),
            ParticipatingEvent::FinalitySignaturesFetcher(event) => reactor::wrap_effects(
                ParticipatingEvent::FinalitySignaturesFetcher,
                self.finality_signatures_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::FinalitySignaturesFetcherRequest(request) => reactor::wrap_effects(
                ParticipatingEvent::FinalitySignaturesFetcher,
                self.finality_signatures_fetcher
                    .handle_event(effect_builder, rng, request.into()),
            ),
            ParticipatingEvent::FinalitySignatureFetcher(_) => todo!(),
            ParticipatingEvent::FinalitySignatureFetcherRequest(_) => todo!(),

            // DEPLOY STUFF (NOTE: FINALIZED APPROVALS PERTAIN TO DEPLOYS AND ARE DIFFERENT FROM
            // FINALIZATION SIGNATURES)
            ParticipatingEvent::DeployFetcher(event) => reactor::wrap_effects(
                ParticipatingEvent::DeployFetcher,
                self.deploy_fetcher.handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::DeployFetcherRequest(request) => reactor::wrap_effects(
                ParticipatingEvent::DeployFetcher,
                self.deploy_fetcher
                    .handle_event(effect_builder, rng, request.into()),
            ),
            ParticipatingEvent::FinalizedApprovalsFetcher(event) => reactor::wrap_effects(
                ParticipatingEvent::FinalizedApprovalsFetcher,
                self.finalized_approvals_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::FinalizedApprovalsFetcherRequest(request) => reactor::wrap_effects(
                ParticipatingEvent::FinalizedApprovalsFetcher,
                self.finalized_approvals_fetcher
                    .handle_event(effect_builder, rng, request.into()),
            ),

            // CATCHING UP STUFF
            ParticipatingEvent::SyncLeapFetcher(event) => reactor::wrap_effects(
                ParticipatingEvent::SyncLeapFetcher,
                self.sync_leap_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::SyncLeapFetcherRequest(request) => reactor::wrap_effects(
                ParticipatingEvent::SyncLeapFetcher,
                self.sync_leap_fetcher
                    .handle_event(effect_builder, rng, request.into()),
            ),
            ParticipatingEvent::TrieOrChunkFetcher(event) => reactor::wrap_effects(
                ParticipatingEvent::TrieOrChunkFetcher,
                self.trie_or_chunk_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::TrieOrChunkFetcherRequest(request) => reactor::wrap_effects(
                ParticipatingEvent::TrieOrChunkFetcher,
                self.trie_or_chunk_fetcher
                    .handle_event(effect_builder, rng, request.into()),
            ),

            // MISC DISPATCHING
            ParticipatingEvent::DeployAcceptorAnnouncement(
                DeployAcceptorAnnouncement::AcceptedNewDeploy { deploy, source },
            ) => reactor::wrap_effects(
                ParticipatingEvent::DeployFetcher,
                self.deploy_fetcher.handle_event(
                    effect_builder,
                    rng,
                    fetcher::Event::GotRemotely {
                        item: deploy,
                        source,
                    },
                ),
            ),

            // TODO: KILL BY HEIGHT FETCHING WITH FIRE
            ParticipatingEvent::BlockByHeightFetcher(event) => reactor::wrap_effects(
                ParticipatingEvent::BlockByHeightFetcher,
                self.block_by_height_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::BlockByHeightFetcherRequest(request) => reactor::wrap_effects(
                ParticipatingEvent::BlockByHeightFetcher,
                self.block_by_height_fetcher
                    .handle_event(effect_builder, rng, request.into()),
            ),
            ParticipatingEvent::BlockHeaderByHeightFetcher(event) => reactor::wrap_effects(
                ParticipatingEvent::BlockHeaderByHeightFetcher,
                self.block_header_and_finality_signatures_by_height_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            ParticipatingEvent::BlockHeaderByHeightFetcherRequest(request) => {
                reactor::wrap_effects(
                    ParticipatingEvent::BlockHeaderByHeightFetcher,
                    self.block_header_and_finality_signatures_by_height_fetcher
                        .handle_event(effect_builder, rng, request.into()),
                )
            }

            // YEAH YEAH, I KNOW
            _ => Effects::new(),
        }
    }
}
