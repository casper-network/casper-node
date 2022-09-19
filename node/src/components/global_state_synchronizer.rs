use std::collections::hash_map::Entry;
use std::collections::HashSet;
use std::{collections::HashMap, time::Duration};

use casper_hashing::{ChunkWithProofVerificationError, Digest};
use num_rational::Ratio;

use crate::components::fetcher::FetchResult;
use crate::components::Component;
use crate::types::TrieOrChunk;
use crate::{
    components::fetcher::{metrics::Metrics, FetchResponder, Fetcher, ItemFetcher},
    effect::{requests::StorageRequest, EffectBuilder, EffectExt, Effects},
    types::{BlockHash, FetcherItem, Item, NodeId},
    NodeRng,
};

#[derive(Clone, DataSize, Debug)]
pub(crate) struct Config {
    /// Maximum number of trie nodes to fetch in parallel.
    max_parallel_trie_fetches: u32,
}

pub(crate) struct SyncGlobalState {
    state_root_hash: Digest,
    peer: NodeId,
}

pub(crate) enum Event {
    Request(SyncGlobalState),
    FetchResult {
        state_root_hash: Digest,
        result: FetchResult<TrieOrChunk>,
    },
}

#[derive(Default)]
struct Foo {
    peers: HashSet<NodeId>,
    in_flight: usize,
}

impl Foo {
    fn new(peer: NodeId) -> Self {
        Self {
            peers: vec![peer],
            in_flight: 0,
        }
    }
}

pub(crate) struct GlobalStateSynchronizer {
    max_parallel_trie_fetches: usize,
    ops: HashMap<Digest, Foo>,
}

impl GlobalStateSynchronizer {
    fn new(config: Config) -> Self {
        Self {
            max_parallel_trie_fetches: config.max_parallel_trie_fetches,
            ops: HashMap::new(),
        }
    }

    fn handle_request<REv>(
        &mut self,
        request: SyncGlobalState,
        effect_builder: EffectBuilder<REv>,
    ) -> Effects<Event> {
        let entry = self.ops.entry(request.state_root_hash).or_default();
        if entry.peers.insert(request.peer) {
            if entry.in_flight < self.max_parallel_trie_fetches {
                entry.in_flight += 1;
                effect_builder
                    .fetch::<TrieOrChunk>(request.state_root_hash)
                    .event(|result| Event::FetchResult())
            }
        }
    }
}

impl<REv> Component<REv> for GlobalStateSynchronizer {
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Request(request) => self.handle_request(request, effect_builder),
            Event::FetchResult(_) => {}
        }
    }
}
