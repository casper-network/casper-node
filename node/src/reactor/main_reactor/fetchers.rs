use datasize::DataSize;
use prometheus::Registry;

use crate::{
    components::{fetcher, fetcher::Fetcher, Component},
    effect::{announcements::DeployAcceptorAnnouncement, EffectBuilder, Effects},
    reactor,
    reactor::main_reactor::MainEvent,
    types::{
        ApprovalsHashes, Block, BlockExecutionResultsOrChunk, BlockHeader, Deploy,
        FinalitySignature, LegacyDeploy, SyncLeap, TrieOrChunk,
    },
    FetcherConfig, NodeRng,
};

#[derive(DataSize, Debug)]
pub(super) struct Fetchers {
    sync_leap_fetcher: Fetcher<SyncLeap>,
    block_fetcher: Fetcher<Block>,
    block_header_by_hash_fetcher: Fetcher<BlockHeader>,
    approvals_hashes_fetcher: Fetcher<ApprovalsHashes>,
    finality_signature_fetcher: Fetcher<FinalitySignature>,
    legacy_deploy_fetcher: Fetcher<LegacyDeploy>,
    deploy_fetcher: Fetcher<Deploy>,
    trie_or_chunk_fetcher: Fetcher<TrieOrChunk>,
    block_execution_results_or_chunk_fetcher: Fetcher<BlockExecutionResultsOrChunk>,
}

impl Fetchers {
    pub(super) fn new(
        config: &FetcherConfig,
        metrics_registry: &Registry,
    ) -> Result<Self, prometheus::Error> {
        Ok(Fetchers {
            sync_leap_fetcher: Fetcher::new("sync_leap_fetcher", config, metrics_registry)?,
            block_header_by_hash_fetcher: Fetcher::new("block_header", config, metrics_registry)?,
            approvals_hashes_fetcher: Fetcher::new("approvals_hashes", config, metrics_registry)?,
            finality_signature_fetcher: Fetcher::new(
                "finality_signature_fetcher",
                config,
                metrics_registry,
            )?,
            legacy_deploy_fetcher: Fetcher::new("legacy_deploy", config, metrics_registry)?,
            block_fetcher: Fetcher::new("block", config, metrics_registry)?,
            deploy_fetcher: Fetcher::new("deploy", config, metrics_registry)?,
            trie_or_chunk_fetcher: Fetcher::new("trie_or_chunk", config, metrics_registry)?,
            block_execution_results_or_chunk_fetcher: Fetcher::new(
                "block_execution_results_or_chunk_fetcher",
                config,
                metrics_registry,
            )?,
        })
    }

    pub(super) fn dispatch_fetcher_event(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
        event: MainEvent,
    ) -> Effects<MainEvent> {
        match event {
            MainEvent::BlockFetcher(event) => reactor::wrap_effects(
                MainEvent::BlockFetcher,
                self.block_fetcher.handle_event(effect_builder, rng, event),
            ),
            MainEvent::BlockFetcherRequest(request) => reactor::wrap_effects(
                MainEvent::BlockFetcher,
                self.block_fetcher
                    .handle_event(effect_builder, rng, request.into()),
            ),
            MainEvent::SyncLeapFetcher(event) => reactor::wrap_effects(
                MainEvent::SyncLeapFetcher,
                self.sync_leap_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::SyncLeapFetcherRequest(request) => reactor::wrap_effects(
                MainEvent::SyncLeapFetcher,
                self.sync_leap_fetcher
                    .handle_event(effect_builder, rng, request.into()),
            ),
            MainEvent::BlockHeaderFetcher(event) => reactor::wrap_effects(
                MainEvent::BlockHeaderFetcher,
                self.block_header_by_hash_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::BlockHeaderFetcherRequest(request) => reactor::wrap_effects(
                MainEvent::BlockHeaderFetcher,
                self.block_header_by_hash_fetcher
                    .handle_event(effect_builder, rng, request.into()),
            ),
            MainEvent::ApprovalsHashesFetcher(event) => reactor::wrap_effects(
                MainEvent::ApprovalsHashesFetcher,
                self.approvals_hashes_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::ApprovalsHashesFetcherRequest(request) => reactor::wrap_effects(
                MainEvent::ApprovalsHashesFetcher,
                self.approvals_hashes_fetcher
                    .handle_event(effect_builder, rng, request.into()),
            ),
            MainEvent::FinalitySignatureFetcher(event) => reactor::wrap_effects(
                MainEvent::FinalitySignatureFetcher,
                self.finality_signature_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::FinalitySignatureFetcherRequest(request) => reactor::wrap_effects(
                MainEvent::FinalitySignatureFetcher,
                self.finality_signature_fetcher
                    .handle_event(effect_builder, rng, request.into()),
            ),
            MainEvent::LegacyDeployFetcher(event) => reactor::wrap_effects(
                MainEvent::LegacyDeployFetcher,
                self.legacy_deploy_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::LegacyDeployFetcherRequest(request) => reactor::wrap_effects(
                MainEvent::LegacyDeployFetcher,
                self.legacy_deploy_fetcher
                    .handle_event(effect_builder, rng, request.into()),
            ),
            MainEvent::DeployFetcher(event) => reactor::wrap_effects(
                MainEvent::DeployFetcher,
                self.deploy_fetcher.handle_event(effect_builder, rng, event),
            ),
            MainEvent::DeployFetcherRequest(request) => reactor::wrap_effects(
                MainEvent::DeployFetcher,
                self.deploy_fetcher
                    .handle_event(effect_builder, rng, request.into()),
            ),
            MainEvent::TrieOrChunkFetcher(event) => reactor::wrap_effects(
                MainEvent::TrieOrChunkFetcher,
                self.trie_or_chunk_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::TrieOrChunkFetcherRequest(request) => reactor::wrap_effects(
                MainEvent::TrieOrChunkFetcher,
                self.trie_or_chunk_fetcher
                    .handle_event(effect_builder, rng, request.into()),
            ),
            MainEvent::BlockExecutionResultsOrChunkFetcher(event) => reactor::wrap_effects(
                MainEvent::BlockExecutionResultsOrChunkFetcher,
                self.block_execution_results_or_chunk_fetcher.handle_event(
                    effect_builder,
                    rng,
                    event,
                ),
            ),
            MainEvent::BlockExecutionResultsOrChunkFetcherRequest(request) => {
                reactor::wrap_effects(
                    MainEvent::BlockExecutionResultsOrChunkFetcher,
                    self.block_execution_results_or_chunk_fetcher.handle_event(
                        effect_builder,
                        rng,
                        request.into(),
                    ),
                )
            }

            // MISC DISPATCHING
            MainEvent::DeployAcceptorAnnouncement(
                DeployAcceptorAnnouncement::AcceptedNewDeploy { deploy, source },
            ) => {
                let mut effects = reactor::wrap_effects(
                    MainEvent::LegacyDeployFetcher,
                    self.legacy_deploy_fetcher.handle_event(
                        effect_builder,
                        rng,
                        fetcher::Event::GotRemotely {
                            item: Box::new(LegacyDeploy::from((*deploy).clone())),
                            source: source.clone(),
                        },
                    ),
                );
                effects.extend(reactor::wrap_effects(
                    MainEvent::DeployFetcher,
                    self.deploy_fetcher.handle_event(
                        effect_builder,
                        rng,
                        fetcher::Event::GotRemotely {
                            item: deploy,
                            source,
                        },
                    ),
                ));
                effects
            }
            // allow non-fetcher events to fall thru
            _ => Effects::new(),
        }
    }
}
