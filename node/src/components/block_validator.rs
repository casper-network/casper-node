//! Block validator
//!
//! The block validator checks whether all the deploys included in the proto block exist, either
//! locally or on the network.
//!
//! When multiple requests are made to validate the same proto block, they will eagerly return true
//! if valid, but only fail if all sources have been exhausted. This is only relevant when calling
//! for validation of the same protoblock multiple times at the same time.

mod keyed_counter;

use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    fmt::Debug,
};

use derive_more::{Display, From};
use rand::Rng;
use smallvec::{smallvec, SmallVec};

use crate::{
    components::Component,
    effect::{
        requests::{BlockValidationRequest, FetcherRequest},
        EffectBuilder, EffectExt, EffectOptionExt, Effects, Responder,
    },
    types::{Deploy, DeployHash, ProtoBlock},
};
use keyed_counter::KeyedCounter;

/// Block validator component event.
#[derive(Debug, From, Display)]
pub enum Event<I> {
    /// A request made of the block validator component.
    #[from]
    Request(BlockValidationRequest<I>),

    /// A deploy has been successfully found.
    #[display(fmt = "deploy {} found", _0)]
    DeployFound(DeployHash),

    /// A request to find a specific deploy, potentially from a peer, failed.
    #[display(fmt = "deploy {} missing", _0)]
    DeployMissing(DeployHash),
}

/// State of the current process of block validation.
///
/// Tracks whether or not there are deploys still missing and who is interested in the final result.
#[derive(Debug)]
struct BlockValidationState {
    /// The deploys that have not yet been "crossed off" the list of potential misses.
    missing_deploys: HashSet<DeployHash>,
    /// A list of responders that are awaiting an answer.
    responders: SmallVec<[Responder<(bool, ProtoBlock)>; 2]>,
}

/// Block validator.
#[derive(Debug, Default)]
pub(crate) struct BlockValidator<I> {
    /// State of validation of a specific block.
    validation_states: HashMap<ProtoBlock, BlockValidationState>,

    /// Number of requests for a specific deploy hash still in flight.
    in_flight: KeyedCounter<DeployHash>,

    _marker: std::marker::PhantomData<I>,
}

impl<I> BlockValidator<I> {
    /// Creates a new block validator instance.
    pub(crate) fn new() -> Self {
        BlockValidator {
            validation_states: Default::default(),
            in_flight: Default::default(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<I, REv> Component<REv> for BlockValidator<I>
where
    I: Clone + Send + 'static,
    REv: From<Event<I>> + From<BlockValidationRequest<I>> + From<FetcherRequest<I, Deploy>> + Send,
{
    type Event = Event<I>;

    fn handle_event<R: Rng + ?Sized>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Request(BlockValidationRequest {
                proto_block,
                sender,
                responder,
            }) => {
                // No matter the current state, we will request the deploys inside this protoblock
                // for now. Duplicate requests must still be answered, but are hopefully
                // deduplicated by the fetcher.
                let effects = proto_block
                    .deploys
                    .iter()
                    .flat_map(|deploy_hash| {
                        // For every request, increase the number of in-flight...
                        self.in_flight.inc(deploy_hash);

                        // ...then request it.
                        let dh_found = *deploy_hash;
                        let dh_not_found = *deploy_hash;
                        effect_builder
                            .fetch_deploy(*deploy_hash, sender.clone())
                            .option(
                                move |_value| Event::DeployFound(dh_found),
                                move || Event::DeployMissing(dh_not_found),
                            )
                    })
                    .collect();

                // TODO: Clean this up to use `or_insert_with_key` once
                // https://github.com/rust-lang/rust/issues/71024 is stabilized.
                match self.validation_states.entry(proto_block) {
                    Entry::Occupied(mut entry) => {
                        // The entry already exists. We register ourselves as someone interested in
                        // the ultimate validation result.
                        entry.get_mut().responders.push(responder);
                    }
                    Entry::Vacant(entry) => {
                        // Our entry is vacant - create an entry to track the state.
                        let missing_deploys: HashSet<DeployHash> =
                            entry.key().deploys.iter().cloned().collect();

                        entry.insert(BlockValidationState {
                            missing_deploys,
                            responders: smallvec![responder],
                        });
                    }
                }

                effects
            }

            Event::DeployFound(deploy_hash) => {
                // We successfully found a hash. Decrease the number of outstanding requests.
                self.in_flight.dec(&deploy_hash);

                // Our first pass updates all validation states, crossing off the found deploy.
                for state in self.validation_states.values_mut() {
                    state.missing_deploys.remove(&deploy_hash);
                }

                let mut effects = Effects::new();
                // Now we remove all states that have finished and notify the requestors.
                self.validation_states.retain(|key, state| {
                    if state.missing_deploys.is_empty() {
                        // This one is done and valid.
                        state
                            .responders
                            .drain(0..(state.responders.len()))
                            .for_each(|responder| {
                                effects.extend(responder.respond((true, key.clone())).ignore());
                            });
                        false
                    } else {
                        true
                    }
                });

                effects
            }

            Event::DeployMissing(deploy_hash) => {
                // A deploy failed to fetch. If there is still hope (i.e. other outstanding
                // requests), we just ignore this little accident.
                if self.in_flight.dec(&deploy_hash) != 0 {
                    return Effects::new();
                }

                // Otherwise notify everyone still waiting on it that all is lost.

                let mut effects = Effects::new();

                self.validation_states.retain(|key, state| {
                    if state.missing_deploys.contains(&deploy_hash) {
                        // This validation state contains a failed deploy hash, it can never succeed.
                        state
                            .responders
                            .drain(0..(state.responders.len()))
                            .for_each(|responder| {
                                effects.extend(responder.respond((false, key.clone())).ignore());
                            });
                        false
                    } else {
                        true
                    }
                });

                effects
            }
        }
    }
}
