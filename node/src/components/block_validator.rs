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
    convert::Infallible,
    fmt::Debug,
};

use datasize::DataSize;
use derive_more::{Display, From};
use smallvec::{smallvec, SmallVec};

use crate::{
    components::Component,
    effect::{
        requests::{BlockValidationRequest, FetcherRequest},
        EffectBuilder, EffectExt, EffectOptionExt, Effects, Responder,
    },
    types::{BlockLike, CryptoRngCore, Deploy, DeployHash},
};
use keyed_counter::KeyedCounter;

/// Block validator component event.
#[derive(Debug, From, Display)]
pub enum Event<T, I> {
    /// A request made of the block validator component.
    #[from]
    Request(BlockValidationRequest<T, I>),

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
#[derive(DataSize, Debug)]
pub(crate) struct BlockValidationState<T> {
    /// The deploys that have not yet been "crossed off" the list of potential misses.
    missing_deploys: HashSet<DeployHash>,
    /// A list of responders that are awaiting an answer.
    responders: SmallVec<[Responder<(bool, T)>; 2]>,
}

/// Block validator.
#[derive(DataSize, Debug, Default)]
pub(crate) struct BlockValidator<T, I> {
    /// State of validation of a specific block.
    validation_states: HashMap<T, BlockValidationState<T>>,

    /// Number of requests for a specific deploy hash still in flight.
    in_flight: KeyedCounter<DeployHash>,

    _marker: std::marker::PhantomData<I>,
}

impl<T, I> BlockValidator<T, I> {
    /// Creates a new block validator instance.
    pub(crate) fn new() -> Self {
        BlockValidator {
            validation_states: Default::default(),
            in_flight: Default::default(),
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T, I, REv> Component<REv> for BlockValidator<T, I>
where
    T: BlockLike + Send + Clone + 'static,
    I: Clone + Send + 'static,
    REv: From<Event<T, I>>
        + From<BlockValidationRequest<T, I>>
        + From<FetcherRequest<I, Deploy>>
        + Send,
{
    type Event = Event<T, I>;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut dyn CryptoRngCore,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Request(BlockValidationRequest {
                block,
                sender,
                responder,
            }) => {
                if block.deploys().is_empty() {
                    // If there are no deploys, return early.
                    let mut effects = Effects::new();
                    effects.extend(responder.respond((true, block)).ignore());
                    return effects;
                }
                // No matter the current state, we will request the deploys inside this protoblock
                // for now. Duplicate requests must still be answered, but are
                // de-duplicated by the fetcher.
                let effects = block
                    .deploys()
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
                match self.validation_states.entry(block) {
                    Entry::Occupied(mut entry) => {
                        // The entry already exists. We register ourselves as someone interested in
                        // the ultimate validation result.
                        entry.get_mut().responders.push(responder);
                    }
                    Entry::Vacant(entry) => {
                        // Our entry is vacant - create an entry to track the state.
                        let missing_deploys: HashSet<DeployHash> =
                            entry.key().deploys().iter().cloned().collect();

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
                        state.responders.drain(..).for_each(|responder| {
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
                        // This validation state contains a failed deploy hash, it can never
                        // succeed.
                        state.responders.drain(..).for_each(|responder| {
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
