use crate::types::FinalitySignature;
use crate::{
    components::{fetcher, fetcher::Fetcher, Component},
    effect::{announcements::DeployAcceptorAnnouncement, EffectBuilder, Effects},
    reactor,
    reactor::main_reactor::MainEvent,
    types::{
        Block, BlockAdded, BlockAndDeploys, BlockDeployApprovals, BlockHeader,
        BlockHeaderWithMetadata, BlockHeadersBatch, BlockSignatures, BlockWithMetadata, Chainspec,
        Deploy, SyncLeap, TrieOrChunk,
    },
    FetcherConfig, NodeRng,
};
use datasize::DataSize;
use prometheus::Registry;

#[derive(DataSize, Debug)]
pub(super) struct Fetchers {
    sync_leap_fetcher: Fetcher<SyncLeap>,
    block_header_by_hash_fetcher: Fetcher<BlockHeader>,
    block_added_fetcher: Fetcher<BlockAdded>,
    finality_signature_fetcher: Fetcher<FinalitySignature>,
    deploy_fetcher: Fetcher<Deploy>,
    trie_or_chunk_fetcher: Fetcher<TrieOrChunk>,
}

impl Fetchers {
    pub(super) fn new(
        config: &FetcherConfig,
        chainspec: &Chainspec,
        metrics_registry: &Registry,
    ) -> Result<Self, prometheus::Error> {
        Ok(Fetchers {
            sync_leap_fetcher: Fetcher::new_with_metadata(
                "sync_leap_fetcher",
                config,
                metrics_registry,
                chainspec.highway_config.finality_threshold_fraction,
            )?,
            block_header_by_hash_fetcher: Fetcher::new("block_header", config, metrics_registry)?,
            block_added_fetcher: Fetcher::new("block_added_fetcher", config, metrics_registry)?,
            finality_signature_fetcher: Fetcher::new(
                "finality_signature_fetcher",
                config,
                metrics_registry,
            )?,
            deploy_fetcher: Fetcher::new("deploy", config, metrics_registry)?,
            trie_or_chunk_fetcher: Fetcher::new("trie_or_chunk", config, metrics_registry)?,
        })
    }

    pub(super) fn dispatch_fetcher_event(
        &mut self,
        effect_builder: EffectBuilder<MainEvent>,
        rng: &mut NodeRng,
        event: MainEvent,
    ) -> Effects<MainEvent> {
        match event {
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
            MainEvent::BlockAddedFetcher(event) => reactor::wrap_effects(
                MainEvent::BlockAddedFetcher,
                self.block_added_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            MainEvent::BlockAddedFetcherRequest(request) => reactor::wrap_effects(
                MainEvent::BlockAddedFetcher,
                self.block_added_fetcher
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

            // MISC DISPATCHING
            MainEvent::DeployAcceptorAnnouncement(
                DeployAcceptorAnnouncement::AcceptedNewDeploy { deploy, source },
            ) => reactor::wrap_effects(
                MainEvent::DeployFetcher,
                self.deploy_fetcher.handle_event(
                    effect_builder,
                    rng,
                    fetcher::Event::GotRemotely {
                        item: deploy,
                        source,
                    },
                ),
            ),
            // allow non-fetcher events to fall thru
            _ => Effects::new(),
        }
    }
}
