//! Block validator
//!
//! The block validator checks whether all the transactions included in the block payload exist,
//! either locally or on the network.
//!
//! When multiple requests are made to validate the same block payload, they will eagerly return
//! true if valid, but only fail if all sources have been exhausted. This is only relevant when
//! calling for validation of the same proposed block multiple times at the same time.

mod config;
mod event;
mod state;
#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::Arc,
};

use datasize::DataSize;
use tracing::{debug, error, trace, warn};

use casper_types::{
    Approval, ApprovalsHash, Chainspec, EraId, FinalitySignature, FinalitySignatureId, PublicKey,
    RewardedSignatures, SingleBlockRewardedSignatures, Timestamp, Transaction, TransactionHash,
    TransactionId,
};

use crate::{
    components::{
        consensus::{ClContext, ProposedBlock},
        fetcher::{self, EmptyValidationMetadata, FetchResult, FetchedData},
        Component,
    },
    effect::{
        announcements::FatalAnnouncement,
        requests::{BlockValidationRequest, FetcherRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects, Responder,
    },
    fatal,
    types::{BlockWithMetadata, NodeId, TransactionFootprint, ValidatorMatrix},
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

    /// How many transactions are being tracked?
    pub(crate) fn transaction_count(&self) -> usize {
        self.value().count(None)
    }

    pub(crate) fn all_transactions(
        &self,
    ) -> impl Iterator<Item = &(TransactionHash, BTreeSet<Approval>)> {
        self.value().all_transactions()
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
    /// Component configuration.
    config: Config,
    /// Chainspec loaded for transaction validation.
    #[data_size(skip)]
    chainspec: Arc<Chainspec>,
    /// Validator matrix.
    #[data_size(skip)]
    validator_matrix: ValidatorMatrix,
    /// State of validation of a specific block.
    validation_states: HashMap<ProposedBlock<ClContext>, BlockValidationState>,
    /// Requests awaiting storing of a block, keyed by the height of the block being awaited.
    requests_on_hold: BTreeMap<u64, Vec<BlockValidationRequest>>,
    /// The gas price for validation of proposed blocks.
    current_gas_price: u8,
}

impl BlockValidator {
    /// Creates a new block validator instance.
    pub(crate) fn new(
        chainspec: Arc<Chainspec>,
        validator_matrix: ValidatorMatrix,
        config: Config,
        current_gas_price: u8,
    ) -> Self {
        BlockValidator {
            chainspec,
            validator_matrix,
            config,
            validation_states: HashMap::new(),
            requests_on_hold: BTreeMap::new(),
            current_gas_price,
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
        REv: From<Event>
            + From<FetcherRequest<Transaction>>
            + From<FetcherRequest<FinalitySignature>>
            + Send,
    {
        if let Some(state) = self.validation_states.get_mut(&request.block) {
            let BlockValidationRequest {
                block,
                sender,
                responder,
                ..
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
                    missing_transactions,
                    missing_signatures,
                } => fetch_transactions_and_signatures(
                    effect_builder,
                    holder,
                    missing_transactions,
                    missing_signatures,
                ),
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
        request: BlockValidationRequest,
    ) -> Effects<Event>
    where
        REv: From<Event>
            + From<FetcherRequest<Transaction>>
            + From<FetcherRequest<FinalitySignature>>
            + From<StorageRequest>
            + From<FatalAnnouncement>
            + Send,
    {
        debug!(sender = %request.sender, block = %request.block, "validating new proposed block");
        debug_assert!(!self.validation_states.contains_key(&request.block));

        if request.block.value().rewarded_signatures().has_some() {
            // The block contains cited signatures - we have to read the relevant blocks and find
            // out who the validators are in order to decode the signature IDs
            let signature_rewards_max_delay =
                self.chainspec.core_config.signature_rewards_max_delay;
            let minimum_block_height = request
                .proposed_block_height
                .saturating_sub(signature_rewards_max_delay);

            debug!(
                proposed_block=?request.block,
                %minimum_block_height,
                proposed_block_height=%request.proposed_block_height,
                "block cites signatures, validation required - requesting past blocks from storage"
            );

            effect_builder
                .collect_past_blocks_with_metadata(
                    minimum_block_height..request.proposed_block_height,
                    false,
                )
                .event(
                    move |past_blocks_with_metadata| Event::GotPastBlocksWithMetadata {
                        past_blocks_with_metadata,
                        request,
                    },
                )
        } else {
            self.handle_new_request_with_signatures(effect_builder, request, HashSet::new())
        }
    }

    /// This function pairs the `SingleBlockRewardedSignatures` entries from `rewarded_signatures`
    /// with the relevant past blocks and their metadata. If a block for which some signatures are
    /// cited is missing, or if some signatures are double-cited, it will return `None`.
    fn relevant_blocks_and_cited_signatures<'b, 'c>(
        past_blocks_with_metadata: &'b [Option<BlockWithMetadata>],
        proposed_block_height: u64,
        rewarded_signatures: &'c RewardedSignatures,
    ) -> Result<Vec<(&'b BlockWithMetadata, &'c SingleBlockRewardedSignatures)>, Option<u64>> {
        let mut result = Vec::new();
        // Check whether we know all the blocks for which the proposed block cites some signatures,
        // and if no signatures are doubly cited.
        for ((past_block_height, signatures), maybe_block) in rewarded_signatures
            .iter_with_height(proposed_block_height)
            .zip(past_blocks_with_metadata.iter().rev())
        {
            match maybe_block {
                None if signatures.has_some() => {
                    trace!(%past_block_height, "maybe_block = None if signatures.has_some() - returning");

                    return Err(Some(past_block_height));
                }
                None => {
                    // we have no block, but there are also no signatures cited for this block, so
                    // we can continue
                    trace!(%past_block_height, "maybe_block = None");
                }
                Some(block) => {
                    let padded_signatures = block.block.rewarded_signatures().clone().left_padded(
                        proposed_block_height.saturating_sub(past_block_height) as usize,
                    );
                    trace!(
                        ?padded_signatures,
                        ?rewarded_signatures,
                        intersection = ?rewarded_signatures.intersection(&padded_signatures),
                        "maybe_block is Some"
                    );
                    if rewarded_signatures
                        .intersection(&padded_signatures)
                        .has_some()
                    {
                        // block cited a signature that has been cited before - it is invalid!
                        debug!(
                            %past_block_height,
                            "maybe_block is Some, nonzero intersection with previous"
                        );
                        return Err(None);
                    }
                    // everything is OK - save the block in the result
                    result.push((block, signatures));
                }
            }
        }
        Ok(result)
    }

    fn era_ids_vec(past_blocks_with_metadata: &[Option<BlockWithMetadata>]) -> Vec<Option<EraId>> {
        // This will create a vector of era ids for the past blocks corresponding to cited
        // signatures. The index of the entry in the vector will be the number of blocks in the
        // past relative to the current block, minus 1 (ie., 0 is the previous block, 1 is the one
        // before that, etc.) - these indices will correspond directly to the indices in
        // RewardedSignatures.
        past_blocks_with_metadata
            .iter()
            .rev()
            .map(|maybe_metadata| {
                maybe_metadata
                    .as_ref()
                    .map(|metadata| metadata.block.era_id())
            })
            .collect()
    }

    fn get_relevant_validators(
        &mut self,
        past_blocks_with_metadata: &[Option<BlockWithMetadata>],
    ) -> HashMap<EraId, BTreeSet<PublicKey>> {
        let era_ids_vec = Self::era_ids_vec(past_blocks_with_metadata);
        // get the set of unique era ids that are present in the cited blocks
        let era_ids: HashSet<_> = era_ids_vec.iter().flatten().copied().collect();
        let validator_matrix = &self.validator_matrix;

        era_ids
            .into_iter()
            .filter_map(move |era_id| {
                validator_matrix
                    .validator_weights(era_id)
                    .map(|weights| (era_id, weights.into_validator_public_keys().collect()))
            })
            .collect()
    }

    fn handle_got_past_blocks_with_metadata<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        past_blocks_with_metadata: Vec<Option<BlockWithMetadata>>,
        request: BlockValidationRequest,
    ) -> Effects<Event>
    where
        REv: From<Event>
            + From<FetcherRequest<Transaction>>
            + From<FetcherRequest<FinalitySignature>>
            + From<FatalAnnouncement>
            + Send,
    {
        let rewarded_signatures = request.block.value().rewarded_signatures();

        match Self::relevant_blocks_and_cited_signatures(
            &past_blocks_with_metadata,
            request.proposed_block_height,
            rewarded_signatures,
        ) {
            Ok(blocks_and_signatures) => {
                let validators = self.get_relevant_validators(&past_blocks_with_metadata);

                // This will be a set of signature IDs of the signatures included in the block, but
                // not found in metadata in storage.
                let mut missing_sigs = HashSet::new();

                for (block_with_metadata, single_block_rewarded_sigs) in blocks_and_signatures {
                    let era_id = block_with_metadata.block.era_id();
                    let Some(all_validators) = validators.get(&era_id) else {
                        return fatal!(effect_builder, "couldn't get validators for {}", era_id)
                            .ignore();
                    };
                    let public_keys = single_block_rewarded_sigs
                        .clone()
                        .to_validator_set(all_validators.iter().cloned());
                    let block_hash = *block_with_metadata.block.hash();
                    missing_sigs.extend(
                        public_keys
                            .into_iter()
                            .filter(move |public_key| {
                                !block_with_metadata
                                    .block_signatures
                                    .has_finality_signature(public_key)
                            })
                            .map(move |public_key| {
                                FinalitySignatureId::new(block_hash, era_id, public_key)
                            }),
                    );
                }

                trace!(
                    ?missing_sigs,
                    "handle_got_past_blocks_with_metadata missing_sigs"
                );

                self.handle_new_request_with_signatures(effect_builder, request, missing_sigs)
            }
            Err(Some(missing_block_height)) => {
                // We are missing some blocks necessary for unpacking signatures from storage - put
                // the request on hold for now.
                self.requests_on_hold
                    .entry(missing_block_height)
                    .or_default()
                    .push(request);
                Effects::new()
            }
            Err(None) => {
                // Rewarded signatures pre-validation failed
                respond(false, Some(request.responder))
            }
        }
    }

    fn handle_block_stored<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        stored_block_height: u64,
    ) -> Effects<Event>
    where
        REv: From<Event>
            + From<StorageRequest>
            + From<FetcherRequest<Transaction>>
            + From<FetcherRequest<FinalitySignature>>
            + From<FatalAnnouncement>
            + Send,
    {
        let mut pending_requests = vec![];

        while self
            .requests_on_hold
            .first_key_value()
            .map_or(false, |(height, _)| *height <= stored_block_height)
        {
            // unwrap is safe - we'd break the loop if there were no elements
            pending_requests.extend(self.requests_on_hold.pop_first().unwrap().1);
        }

        pending_requests
            .into_iter()
            .flat_map(|request| self.handle_new_request(effect_builder, request))
            .collect()
    }

    fn handle_new_request_with_signatures<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        BlockValidationRequest {
            block,
            sender,
            responder,
            ..
        }: BlockValidationRequest,
        missing_signatures: HashSet<FinalitySignatureId>,
    ) -> Effects<Event>
    where
        REv: From<Event>
            + From<FetcherRequest<Transaction>>
            + From<FetcherRequest<FinalitySignature>>
            + From<FatalAnnouncement>
            + Send,
    {
        if let Some(old_state) = self.validation_states.get_mut(&block) {
            // if we got two requests for the same block in quick succession, it is possible that
            // a state has been created and inserted for one of them while the other one was
            // awaiting the past blocks from storage; in such a case just save the holder and
            // responders, and return no effects, as all the fetching will have already been
            // started
            match old_state.add_responder(responder) {
                AddResponderResult::Added => {}
                AddResponderResult::ValidationCompleted {
                    responder,
                    response_to_send,
                } => {
                    debug!(%response_to_send, "proposed block validation already completed");
                    return responder.respond(response_to_send).ignore();
                }
            }
            old_state.add_holder(sender);
            return Effects::new();
        }

        let (mut state, maybe_responder) = BlockValidationState::new(
            &block,
            missing_signatures,
            sender,
            responder,
            self.current_gas_price,
            self.chainspec.as_ref(),
        );
        let effects = match state.start_fetching() {
            MaybeStartFetching::Start {
                holder,
                missing_transactions,
                missing_signatures,
            } => fetch_transactions_and_signatures(
                effect_builder,
                holder,
                missing_transactions,
                missing_signatures,
            ),
            MaybeStartFetching::ValidationSucceeded => {
                debug!("no transactions - block validation complete");
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

    fn update_era_price(&mut self, current_price: u8) {
        self.current_gas_price = current_price;
    }

    fn handle_transaction_fetched<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        transaction_hash: TransactionHash,
        result: FetchResult<Transaction>,
    ) -> Effects<Event>
    where
        REv: From<Event>
            + From<FetcherRequest<Transaction>>
            + From<FetcherRequest<FinalitySignature>>
            + Send,
    {
        match &result {
            Ok(FetchedData::FromPeer { peer, .. }) => {
                debug!(%transaction_hash, %peer, "fetched transaction from peer")
            }
            Ok(FetchedData::FromStorage { .. }) => {
                debug!(%transaction_hash, "fetched transaction locally")
            }
            Err(error) => warn!(%transaction_hash, %error, "could not fetch transaction"),
        }
        match result {
            Ok(FetchedData::FromStorage { item } | FetchedData::FromPeer { item, .. }) => {
                let item_hash = item.hash();
                if item_hash != transaction_hash {
                    // Hard failure - change state to Invalid.
                    let responders = self
                        .validation_states
                        .values_mut()
                        .flat_map(|state| state.try_mark_invalid(&transaction_hash));
                    return respond(false, responders);
                }
                let transaction_footprint = match TransactionFootprint::new(&self.chainspec, &item)
                {
                    Ok(footprint) => footprint,
                    Err(invalid_transaction_error) => {
                        warn!(
                            %transaction_hash, ?invalid_transaction_error,
                            "could not convert transaction",
                        );
                        // Hard failure - change state to Invalid.
                        let responders = self
                            .validation_states
                            .values_mut()
                            .flat_map(|state| state.try_mark_invalid(&transaction_hash));
                        return respond(false, responders);
                    }
                };

                let mut effects = Effects::new();
                for state in self.validation_states.values_mut() {
                    let responders = state
                        .try_add_transaction_footprint(&transaction_hash, &transaction_footprint);
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
                                    missing_transactions,
                                    missing_signatures,
                                } => {
                                    debug!(
                                        %holder,
                                        missing_transactions_len = missing_transactions.len(),
                                        "fetching missing transactions from different peer"
                                    );
                                    effects.extend(fetch_transactions_and_signatures(
                                        effect_builder,
                                        holder,
                                        missing_transactions,
                                        missing_signatures,
                                    ));
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
                            .flat_map(|state| state.try_mark_invalid(&transaction_hash));
                        respond(false, responders)
                    }
                }
            }
        }
    }

    fn handle_finality_signature_fetched<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        finality_signature_id: FinalitySignatureId,
        result: FetchResult<FinalitySignature>,
    ) -> Effects<Event>
    where
        REv: From<Event>
            + From<FetcherRequest<Transaction>>
            + From<FetcherRequest<FinalitySignature>>
            + Send,
    {
        match &result {
            Ok(FetchedData::FromPeer { peer, .. }) => {
                debug!(%finality_signature_id, %peer, "fetched finality signature from peer")
            }
            Ok(FetchedData::FromStorage { .. }) => {
                debug!(%finality_signature_id, "fetched finality signature locally")
            }
            Err(error) => {
                warn!(%finality_signature_id, %error, "could not fetch finality signature")
            }
        }
        match result {
            Ok(FetchedData::FromStorage { .. } | FetchedData::FromPeer { .. }) => {
                let mut effects = Effects::new();
                for state in self.validation_states.values_mut() {
                    let responders = state.try_add_signature(&finality_signature_id);
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
                                    missing_transactions,
                                    missing_signatures,
                                } => {
                                    debug!(
                                        %holder,
                                        missing_transactions_len = missing_transactions.len(),
                                        "fetching missing transactions and signatures from different \
                                        peer"
                                    );
                                    effects.extend(fetch_transactions_and_signatures(
                                        effect_builder,
                                        holder,
                                        missing_transactions,
                                        missing_signatures,
                                    ));
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
                        let responders = self.validation_states.values_mut().flat_map(|state| {
                            state.try_mark_invalid_signature(&finality_signature_id)
                        });
                        respond(false, responders)
                    }
                }
            }
        }
    }
}

fn fetch_transactions_and_signatures<REv>(
    effect_builder: EffectBuilder<REv>,
    holder: NodeId,
    missing_transactions: HashMap<TransactionHash, ApprovalsHash>,
    missing_signatures: HashSet<FinalitySignatureId>,
) -> Effects<Event>
where
    REv: From<Event>
        + From<FetcherRequest<Transaction>>
        + From<FetcherRequest<FinalitySignature>>
        + Send,
{
    let mut effects: Effects<Event> = Effects::new();
    for (transaction_hash, approvals_hash) in missing_transactions {
        let transaction_id = match transaction_hash {
            TransactionHash::Deploy(deploy_hash) => {
                TransactionId::new(deploy_hash.into(), approvals_hash)
            }
            TransactionHash::V1(v1_hash) => TransactionId::new(v1_hash.into(), approvals_hash),
        };
        effects.extend(
            effect_builder
                .fetch::<Transaction>(transaction_id, holder, Box::new(EmptyValidationMetadata))
                .event(move |result| Event::TransactionFetched {
                    transaction_hash,
                    result,
                }),
        );
    }

    for missing_signature in missing_signatures {
        effects.extend(
            effect_builder
                .fetch::<FinalitySignature>(
                    Box::new(missing_signature.clone()),
                    holder,
                    Box::new(EmptyValidationMetadata),
                )
                .event(move |result| Event::FinalitySignatureFetched {
                    finality_signature_id: Box::new(missing_signature),
                    result,
                }),
        )
    }

    effects
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

impl<REv> Component<REv> for BlockValidator
where
    REv: From<Event>
        + From<BlockValidationRequest>
        + From<FetcherRequest<Transaction>>
        + From<FetcherRequest<FinalitySignature>>
        + From<StorageRequest>
        + From<FatalAnnouncement>
        + Send,
{
    type Event = Event;

    fn name(&self) -> &str {
        COMPONENT_NAME
    }

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Request(request) => {
                debug!(block = ?request.block, "validating proposed block");
                match self.try_handle_as_existing_request(effect_builder, request) {
                    MaybeHandled::Handled(effects) => effects,
                    MaybeHandled::NotHandled(request) => {
                        self.handle_new_request(effect_builder, request)
                    }
                }
            }
            Event::GotPastBlocksWithMetadata {
                past_blocks_with_metadata,
                request,
            } => self.handle_got_past_blocks_with_metadata(
                effect_builder,
                past_blocks_with_metadata,
                request,
            ),
            Event::BlockStored(stored_block_height) => {
                self.handle_block_stored(effect_builder, stored_block_height)
            }
            Event::TransactionFetched {
                transaction_hash,
                result,
            } => self.handle_transaction_fetched(effect_builder, transaction_hash, result),
            Event::FinalitySignatureFetched {
                finality_signature_id,
                result,
            } => self.handle_finality_signature_fetched(
                effect_builder,
                *finality_signature_id,
                result,
            ),
            Event::UpdateEraGasPrice(_, current_price) => {
                self.update_era_price(current_price);
                Effects::new()
            }
        }
    }
}
