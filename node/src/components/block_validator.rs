//! Block validator
//!
//! The block validator checks whether all the deploys included in the block payload exist, either
//! locally or on the network.
//!
//! When multiple requests are made to validate the same block payload, they will eagerly return
//! true if valid, but only fail if all sources have been exhausted. This is only relevant when
//! calling for validation of the same proposed block multiple times at the same time.

mod config;
mod event;
mod state;
#[cfg(test)]
mod tests;

use std::{collections::HashMap, sync::Arc};

use datasize::DataSize;
use tracing::{debug, error, warn};

use casper_types::Timestamp;

use crate::{
    components::{
        consensus::{ClContext, ProposedBlock},
        fetcher::{self, EmptyValidationMetadata, FetchResult, FetchedData},
        Component,
    },
    effect::{
        requests::{BlockValidationRequest, FetcherRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects, Responder,
    },
    types::{
        ApprovalsHash, Chainspec, Deploy, DeployHashWithApprovals, DeployId, DeployOrTransferHash,
        NodeId,
    },
    NodeRng,
};
pub use config::Config;
pub(crate) use event::Event;
use state::{AddResponderResult, BlockValidationState, MaybeStartFetching};

const COMPONENT_NAME: &str = "block_validator";

impl ProposedBlock<ClContext> {
    fn timestamp(&self) -> Timestamp {
        self.context().timestamp()
    }

    fn deploys(&self) -> &Vec<DeployHashWithApprovals> {
        self.value().deploys()
    }

    fn transfers(&self) -> &Vec<DeployHashWithApprovals> {
        self.value().transfers()
    }
}

/// The return type of trying to handle a validation request as an already-existing request.
enum MaybeHandled {
    /// The request is already being handled - return the wrapped effects and finish.
    Handled(Effects<Event>),
    /// The request is new - it still needs to be handled.
    NotHandled(BlockValidationRequest),
}

#[derive(DataSize, Debug)]
pub(crate) struct BlockValidator {
    /// Chainspec loaded for deploy validation.
    #[data_size(skip)]
    chainspec: Arc<Chainspec>,
    config: Config,
    /// State of validation of a specific block.
    validation_states: HashMap<ProposedBlock<ClContext>, BlockValidationState>,
}

impl BlockValidator {
    /// Creates a new block validator instance.
    pub(crate) fn new(chainspec: Arc<Chainspec>, config: Config) -> Self {
        BlockValidator {
            chainspec,
            config,
            validation_states: HashMap::new(),
        }
    }

    /// If the request is already being handled, we record the new info and return effects.  If not,
    /// the request is returned for processing as a new request.
    fn try_handle_as_existing_request<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        request: BlockValidationRequest,
    ) -> MaybeHandled
    where
        REv: From<Event> + From<FetcherRequest<Deploy>> + Send,
    {
        if let Some(state) = self.validation_states.get_mut(&request.block) {
            let BlockValidationRequest {
                block,
                sender,
                responder,
            } = request;
            debug!(%sender, %block, "already validating proposed block");
            match state.add_responder(responder) {
                AddResponderResult::Added => {}
                AddResponderResult::ValidationCompleted {
                    responder,
                    response_to_send,
                } => {
                    debug!(%response_to_send, "proposed block validation already completed");
                    return MaybeHandled::Handled(responder.respond(response_to_send).ignore());
                }
            }
            state.add_holder(sender);

            let effects = match state.start_fetching() {
                MaybeStartFetching::Start {
                    holder,
                    missing_deploys,
                } => fetch_deploys(effect_builder, holder, missing_deploys),
                MaybeStartFetching::Ongoing => {
                    debug!("ongoing fetches while validating proposed block - noop");
                    Effects::new()
                }
                MaybeStartFetching::Unable => {
                    debug!("no new info while validating proposed block - responding `false`");
                    respond(false, state.take_responders())
                }
                MaybeStartFetching::ValidationSucceeded | MaybeStartFetching::ValidationFailed => {
                    // If validation is already completed, we should have exited in the
                    // `AddResponderResult::ValidationCompleted` branch above.
                    error!("proposed block validation already completed - noop");
                    Effects::new()
                }
            };
            MaybeHandled::Handled(effects)
        } else {
            MaybeHandled::NotHandled(request)
        }
    }

    fn handle_new_request<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        BlockValidationRequest {
            block,
            sender,
            responder,
        }: BlockValidationRequest,
    ) -> Effects<Event>
    where
        REv: From<Event> + From<FetcherRequest<Deploy>> + Send,
    {
        debug!(%sender, %block, "validating new proposed block");
        debug_assert!(!self.validation_states.contains_key(&block));
        let (mut state, maybe_responder) =
            BlockValidationState::new(&block, sender, responder, self.chainspec.as_ref());
        let effects = match state.start_fetching() {
            MaybeStartFetching::Start {
                holder,
                missing_deploys,
            } => fetch_deploys(effect_builder, holder, missing_deploys),
            MaybeStartFetching::ValidationSucceeded => {
                debug!("no deploys - block validation complete");
                debug_assert!(maybe_responder.is_some());
                respond(true, maybe_responder)
            }
            MaybeStartFetching::ValidationFailed => {
                debug_assert!(maybe_responder.is_some());
                respond(false, maybe_responder)
            }
            MaybeStartFetching::Ongoing | MaybeStartFetching::Unable => {
                // This `MaybeStartFetching` variant should never be returned here.
                error!(%state, "invalid state while handling new block validation");
                debug_assert!(false, "invalid state {}", state);
                respond(false, state.take_responders())
            }
        };
        self.validation_states.insert(block, state);
        self.purge_oldest_complete();
        effects
    }

    fn purge_oldest_complete(&mut self) {
        let mut completed_times: Vec<_> = self
            .validation_states
            .values()
            .filter_map(BlockValidationState::block_timestamp_if_completed)
            .collect();
        // Sort from newest (highest timestamp) to oldest.
        completed_times.sort_unstable_by(|lhs, rhs| rhs.cmp(lhs));

        // Normally we'll only need to remove a maximum of a single entry, but loop until we don't
        // exceed the completed limit to cover any edge cases.
        let max_completed_entries = self.config.max_completed_entries as usize;
        while completed_times.len() > max_completed_entries {
            self.validation_states.retain(|_block, state| {
                if completed_times.len() <= max_completed_entries {
                    return true;
                }
                if state.block_timestamp_if_completed().as_ref() == completed_times.last() {
                    debug!(
                        %state,
                        num_completed_remaining = (completed_times.len() - 1),
                        "purging completed block validation state"
                    );
                    let _ = completed_times.pop();
                    return false;
                }
                true
            });
        }
    }

    fn handle_deploy_fetched<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        dt_hash: DeployOrTransferHash,
        result: FetchResult<Deploy>,
    ) -> Effects<Event>
    where
        REv: From<Event> + From<FetcherRequest<Deploy>> + Send,
    {
        match &result {
            Ok(FetchedData::FromPeer { peer, .. }) => {
                debug!(%dt_hash, %peer, "fetched deploy from peer")
            }
            Ok(FetchedData::FromStorage { .. }) => debug!(%dt_hash, "fetched deploy locally"),
            Err(error) => warn!(%dt_hash, %error, "could not fetch deploy"),
        }
        match result {
            Ok(FetchedData::FromStorage { item }) | Ok(FetchedData::FromPeer { item, .. }) => {
                if item.deploy_or_transfer_hash() != dt_hash {
                    warn!(
                        deploy = %item,
                        expected_deploy_or_transfer_hash = %dt_hash,
                        actual_deploy_or_transfer_hash = %item.deploy_or_transfer_hash(),
                        "deploy has incorrect deploy-or-transfer hash"
                    );
                    // Hard failure - change state to Invalid.
                    let responders = self
                        .validation_states
                        .values_mut()
                        .flat_map(|state| state.try_mark_invalid(&dt_hash));
                    return respond(false, responders);
                }
                let deploy_footprint = match item.footprint() {
                    Ok(footprint) => footprint,
                    Err(error) => {
                        warn!(
                            deploy = %item,
                            %dt_hash,
                            %error,
                            "could not convert deploy",
                        );
                        // Hard failure - change state to Invalid.
                        let responders = self
                            .validation_states
                            .values_mut()
                            .flat_map(|state| state.try_mark_invalid(&dt_hash));
                        return respond(false, responders);
                    }
                };

                let mut effects = Effects::new();
                for state in self.validation_states.values_mut() {
                    let responders = state.try_add_deploy_footprint(&dt_hash, &deploy_footprint);
                    if !responders.is_empty() {
                        let is_valid = matches!(state, BlockValidationState::Valid(_));
                        effects.extend(respond(is_valid, responders));
                    }
                }
                effects
            }
            Err(error) => {
                match error {
                    fetcher::Error::Absent { peer, .. }
                    | fetcher::Error::Rejected { peer, .. }
                    | fetcher::Error::TimedOut { peer, .. } => {
                        // Soft failure - just mark the holder as failed and see if we can start
                        // fetching using a different holder.
                        let mut effects = Effects::new();
                        self.validation_states.values_mut().for_each(|state| {
                            state.try_mark_holder_failed(&peer);
                            match state.start_fetching() {
                                MaybeStartFetching::Start {
                                    holder,
                                    missing_deploys,
                                } => {
                                    debug!(
                                        %holder,
                                        missing_deploys_len = missing_deploys.len(),
                                        "fetching missing deploys from different peer"
                                    );
                                    effects.extend(fetch_deploys(
                                        effect_builder,
                                        holder,
                                        missing_deploys,
                                    ))
                                }
                                MaybeStartFetching::Unable => {
                                    debug!(
                                        "exhausted peers while validating proposed block - \
                                        responding `false`"
                                    );
                                    effects.extend(respond(false, state.take_responders()));
                                }
                                MaybeStartFetching::Ongoing
                                | MaybeStartFetching::ValidationSucceeded
                                | MaybeStartFetching::ValidationFailed => {}
                            }
                        });
                        effects
                    }
                    fetcher::Error::CouldNotConstructGetRequest { .. }
                    | fetcher::Error::ValidationMetadataMismatch { .. } => {
                        // Hard failure - change state to Invalid.
                        let responders = self
                            .validation_states
                            .values_mut()
                            .flat_map(|state| state.try_mark_invalid(&dt_hash));
                        respond(false, responders)
                    }
                }
            }
        }
    }
}

impl<REv> Component<REv> for BlockValidator
where
    REv: From<Event>
        + From<BlockValidationRequest>
        + From<FetcherRequest<Deploy>>
        + From<StorageRequest>
        + Send,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Request(request) => {
                match self.try_handle_as_existing_request(effect_builder, request) {
                    MaybeHandled::Handled(effects) => effects,
                    MaybeHandled::NotHandled(request) => {
                        self.handle_new_request(effect_builder, request)
                    }
                }
            }
            Event::DeployFetched { dt_hash, result } => {
                self.handle_deploy_fetched(effect_builder, dt_hash, result)
            }
        }
    }

    fn name(&self) -> &str {
        COMPONENT_NAME
    }
}

fn fetch_deploys<REv>(
    effect_builder: EffectBuilder<REv>,
    holder: NodeId,
    missing_deploys: HashMap<DeployOrTransferHash, ApprovalsHash>,
) -> Effects<Event>
where
    REv: From<Event> + From<FetcherRequest<Deploy>> + Send,
{
    missing_deploys
        .into_iter()
        .flat_map(|(dt_hash, approvals_hash)| {
            let deploy_id = DeployId::new(dt_hash.into(), approvals_hash);
            effect_builder
                .fetch::<Deploy>(deploy_id, holder, Box::new(EmptyValidationMetadata))
                .event(move |result| Event::DeployFetched { dt_hash, result })
        })
        .collect()
}

fn respond(
    is_valid: bool,
    responders: impl IntoIterator<Item = Responder<bool>>,
) -> Effects<Event> {
    responders
        .into_iter()
        .flat_map(|responder| responder.respond(is_valid).ignore())
        .collect()
}
