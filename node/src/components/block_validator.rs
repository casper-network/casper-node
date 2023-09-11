//! Block validator
//!
//! The block validator checks whether all the deploys included in the block payload exist, either
//! locally or on the network.
//!
//! When multiple requests are made to validate the same block payload, they will eagerly return
//! true if valid, but only fail if all sources have been exhausted. This is only relevant when
//! calling for validation of the same proposed block multiple times at the same time.

mod keyed_counter;
#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fmt::Debug,
    sync::Arc,
};

use datasize::DataSize;
use derive_more::{Display, From};
use itertools::Itertools;
use smallvec::{smallvec, SmallVec};
use tracing::{debug, error, info, trace, warn};

use casper_types::{EraId, Timestamp};

use crate::{
    components::{
        consensus::{ClContext, ProposedBlock},
        fetcher::{EmptyValidationMetadata, FetchItem, FetchedData},
        Component,
    },
    effect::{
        announcements::FatalAnnouncement,
        requests::{BlockValidationRequest, FetcherRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects, Responder,
    },
    fatal,
    types::{
        appendable_block::AppendableBlock, Approval, BlockWithMetadata, Chainspec, Deploy,
        DeployFootprint, DeployHash, DeployHashWithApprovals, DeployOrTransferHash,
        FinalitySignature, FinalitySignatureId, LegacyDeploy, NodeId, RewardedSignatures,
        SingleBlockRewardedSignatures, ValidatorMatrix,
    },
    NodeRng,
};
use keyed_counter::KeyedCounter;

const COMPONENT_NAME: &str = "block_validator";

impl ProposedBlock<ClContext> {
    fn timestamp(&self) -> Timestamp {
        self.context().timestamp()
    }

    fn deploy_hashes(&self) -> impl Iterator<Item = &DeployHash> + '_ {
        self.value().deploy_hashes()
    }

    fn transfer_hashes(&self) -> impl Iterator<Item = &DeployHash> + '_ {
        self.value().transfer_hashes()
    }

    fn deploys_and_transfers_iter(
        &self,
    ) -> impl Iterator<Item = (DeployOrTransferHash, BTreeSet<Approval>)> + '_ {
        let deploys = self.value().deploys().iter().map(|dwa| {
            (
                DeployOrTransferHash::Deploy(*dwa.deploy_hash()),
                dwa.approvals().clone(),
            )
        });
        let transfers = self.value().transfers().iter().map(|dwa| {
            (
                DeployOrTransferHash::Transfer(*dwa.deploy_hash()),
                dwa.approvals().clone(),
            )
        });
        deploys.chain(transfers)
    }
}

/// Block validator component event.
#[derive(Debug, From, Display)]
pub(crate) enum Event {
    /// A request made of the block validator component.
    #[from]
    Request(BlockValidationRequest),

    /// Past blocks with metadata relevant to the finality signatures included in the proposed
    /// block have been read from storage.
    #[display(fmt = "past blocks read from storage")]
    GotPastBlocksWithMetadata {
        past_blocks_with_metadata: Vec<Option<BlockWithMetadata>>,
        proposed_block: ProposedBlock<ClContext>,
        sender: NodeId,
    },

    /// A block at the given height was executed and stored.
    #[display(fmt = "block {} executed and stored", _0)]
    BlockStored(u64),

    /// A deploy has been successfully found.
    #[display(fmt = "{} found", dt_hash)]
    DeployFound {
        dt_hash: DeployOrTransferHash,
        deploy_footprint: Box<DeployFootprint>,
    },

    /// A request to find a specific deploy, potentially from a peer, failed.
    #[display(fmt = "{} missing", _0)]
    DeployMissing(DeployOrTransferHash),

    /// Deploy was invalid. Unable to convert to a deploy type.
    #[display(fmt = "{} invalid", _0)]
    CannotConvertDeploy(DeployOrTransferHash),

    /// A finality signature was correctly fetched.
    #[display(fmt = "{} found", _1)]
    SignatureFound(ProposedBlock<ClContext>, Box<FinalitySignatureId>),

    /// A finality signature couldn't be found or was invalid.
    #[display(fmt = "{} invalid or missing", _1)]
    SignatureError(ProposedBlock<ClContext>, Box<FinalitySignatureId>),
}

/// The state of the process of validating cited finality signatures.
#[derive(DataSize, Debug)]
enum SignaturesValidationState {
    /// Not resolved yet.
    Pending,
    /// Needs blocks up to height `block_height` to be finalized, executed and stored before a
    /// decision can be made.
    AwaitingBlockStored { sender: NodeId, block_height: u64 },
    /// Signatures are valid.
    Valid,
    /// Signatures are invalid.
    Invalid,
    /// Fetching signatures.
    FetchingSignatures {
        missing_sigs: HashSet<FinalitySignatureId>,
        in_flight: KeyedCounter<FinalitySignatureId>,
    },
}

impl SignaturesValidationState {
    fn invalidate(&mut self) {
        if matches!(*self, SignaturesValidationState::Valid) {
            warn!("invalidating SignaturesValidationState::Valid");
        }
        debug!("switching to SignaturesValidationState::Invalid");
        *self = SignaturesValidationState::Invalid;
    }

    fn require_block(&mut self, sender: NodeId, block_height: u64) {
        match self {
            SignaturesValidationState::Pending => {
                debug!(
                    ?block_height,
                    "signature validation awaiting block being stored"
                );
                *self = SignaturesValidationState::AwaitingBlockStored {
                    sender,
                    block_height,
                };
            }
            current_state => {
                warn!(
                    ?current_state,
                    "trying to switch to SignaturesValidationState::AwaitingFinality"
                );
            }
        }
    }

    fn block_stored(&mut self, stored_block_height: u64) -> Option<NodeId> {
        match self {
            SignaturesValidationState::AwaitingBlockStored {
                sender,
                block_height,
            } => {
                if stored_block_height > *block_height {
                    debug!(%stored_block_height, awaiting_block_height = %block_height,
                        "stored higher block; switching back to Pending");
                    let sender = *sender;
                    *self = SignaturesValidationState::Pending;
                    return Some(sender);
                }
            }
            state => {
                debug!(
                    ?state,
                    "handling block finality while not in AwaitingFinality state"
                );
            }
        }
        None
    }

    fn require_signatures(
        &mut self,
        missing_sigs: HashSet<FinalitySignatureId>,
        in_flight: KeyedCounter<FinalitySignatureId>,
    ) {
        match self {
            SignaturesValidationState::Pending => {
                if missing_sigs.is_empty() {
                    debug!("no signatures required; switching to Valid");
                    *self = SignaturesValidationState::Valid;
                } else {
                    debug!(num_missing = %missing_sigs.len(), "switching to FetchingSignatures");
                    *self = SignaturesValidationState::FetchingSignatures {
                        missing_sigs,
                        in_flight,
                    };
                }
            }
            state => {
                warn!(
                    ?state,
                    "trying to switch to FetchingSignatures while not in Pending state"
                );
            }
        }
    }

    fn signature_fetched(&mut self, fetched_sig: FinalitySignatureId) {
        match self {
            SignaturesValidationState::FetchingSignatures {
                missing_sigs,
                in_flight,
            } => {
                missing_sigs.remove(&fetched_sig);
                in_flight.dec(&fetched_sig);
                debug!(?fetched_sig, num_missing = %missing_sigs.len(), "fetched a signature");
                if missing_sigs.is_empty() {
                    debug!("no more signatures missing, switching state to Valid");
                    *self = SignaturesValidationState::Valid;
                }
            }
            state => {
                warn!(
                    ?state,
                    "handling fetched signature while not in FetchingSignatures state"
                );
            }
        }
    }

    fn signature_fetch_failed(&mut self, failed_sig: FinalitySignatureId) {
        match self {
            SignaturesValidationState::FetchingSignatures {
                missing_sigs,
                in_flight,
            } => {
                let still_in_flight = in_flight.dec(&failed_sig);
                if still_in_flight != 0 {
                    debug!(
                        ?self,
                        ?failed_sig,
                        %still_in_flight,
                        "failed to fetch a finality signature"
                    );
                    return;
                }
                if missing_sigs.contains(&failed_sig) {
                    info!(
                        ?self,
                        ?failed_sig,
                        "failed to fetch a cited finality signature"
                    );
                    *self = SignaturesValidationState::Invalid;
                } else {
                    warn!(
                        ?self,
                        ?failed_sig,
                        "failed to fetched a signature that wasn't missing"
                    );
                }
            }
            state => {
                warn!(
                    ?state,
                    "handling failed signature fetch while not in FetchingSignatures state"
                );
            }
        }
    }
}

/// State of the current process of block validation.
///
/// Tracks whether or not there are deploys still missing and who is interested in the final result.
#[derive(DataSize, Debug)]
pub(crate) struct BlockValidationState {
    /// The era ID of the proposed block.
    proposed_block_era_id: EraId,
    /// The height of the proposed block.
    proposed_block_height: u64,
    /// Appendable block ensuring that the deploys satisfy the validity conditions.
    appendable_block: AppendableBlock,
    /// The set of approvals contains approvals from deploys that would be finalized with the
    /// block.
    missing_deploys: HashMap<DeployOrTransferHash, BTreeSet<Approval>>,
    /// The state of validation of cited finality signatures.
    signatures_validation_state: SignaturesValidationState,
    /// A list of responders that are awaiting an answer.
    responders: SmallVec<[Responder<bool>; 2]>,
}

impl BlockValidationState {
    fn respond<REv>(&mut self, value: bool) -> Effects<REv> {
        self.responders
            .drain(..)
            .flat_map(|responder| responder.respond(value).ignore())
            .collect()
    }

    fn is_valid(&self) -> bool {
        self.missing_deploys.is_empty()
            && matches!(
                self.signatures_validation_state,
                SignaturesValidationState::Valid
            )
    }

    fn is_invalid(&self) -> bool {
        matches!(
            self.signatures_validation_state,
            SignaturesValidationState::Invalid
        )
    }
}

#[derive(DataSize, Debug)]
pub(crate) struct BlockValidator {
    /// Chainspec loaded for deploy validation.
    #[data_size(skip)]
    chainspec: Arc<Chainspec>,
    #[data_size(skip)]
    validator_matrix: ValidatorMatrix,
    /// State of validation of a specific block.
    validation_states: HashMap<ProposedBlock<ClContext>, BlockValidationState>,
    /// Number of requests for a specific deploy hash still in flight.
    in_flight: KeyedCounter<DeployHash>,
}

impl BlockValidator {
    /// Creates a new block validator instance.
    pub(crate) fn new(chainspec: Arc<Chainspec>, validator_matrix: ValidatorMatrix) -> Self {
        BlockValidator {
            chainspec,
            validator_matrix,
            validation_states: HashMap::new(),
            in_flight: KeyedCounter::default(),
        }
    }

    /// Prints a log message about an invalid block with duplicated deploys.
    fn log_block_with_replay(&self, sender: NodeId, block: &ProposedBlock<ClContext>) {
        let mut deploy_counts = BTreeMap::new();
        for (dt_hash, _) in block.deploys_and_transfers_iter() {
            *deploy_counts.entry(dt_hash).or_default() += 1;
        }
        let duplicates = deploy_counts
            .into_iter()
            .filter_map(|(dt_hash, count): (DeployOrTransferHash, usize)| {
                (count > 1).then(|| format!("{} * {}", count, dt_hash))
            })
            .join(", ");
        info!(
            peer_id=?sender, %duplicates,
            "received invalid block containing duplicated deploys"
        );
    }

    /// This function pairs the `SingleBlockRewardedSignatures` entries from `rewarded_signatures`
    /// with the relevant past blocks and their metadata. If a block for which some signatures are
    /// cited is missing, or if some signatures are double-cited, it will change the
    /// `block_validation_state` and return `None`.
    fn relevant_blocks_and_cited_signatures<'b, 'c>(
        block_validation_state: &'_ mut BlockValidationState,
        past_blocks_with_metadata: &'b [Option<BlockWithMetadata>],
        rewarded_signatures: &'c RewardedSignatures,
        sender: NodeId,
    ) -> Option<Vec<(&'b BlockWithMetadata, &'c SingleBlockRewardedSignatures)>> {
        let mut result = Vec::new();
        // Check whether we know all the blocks for which the proposed block cites some signatures,
        // and if no signatures are doubly cited.
        for ((past_block_height, signatures), maybe_block) in rewarded_signatures
            .iter_with_height(block_validation_state.proposed_block_height)
            .zip(past_blocks_with_metadata.iter().rev())
        {
            match maybe_block {
                None if signatures.has_some() => {
                    block_validation_state
                        .signatures_validation_state
                        .require_block(sender, past_block_height);

                    trace!(%past_block_height, "maybe_block = None if signatures.has_some() - returning");

                    return None;
                }
                None => {
                    // we have no block, but there are also no signatures cited for this block, so
                    // we can continue
                    trace!(%past_block_height, "maybe_block = None");
                }
                Some(block) => {
                    let padded_signatures = block
                        .block
                        .body()
                        .rewarded_signatures()
                        .clone()
                        .left_padded(
                            block_validation_state
                                .proposed_block_height
                                .saturating_sub(past_block_height)
                                as usize,
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
                        block_validation_state
                            .signatures_validation_state
                            .invalidate();
                        return None;
                    }
                    // everything is OK - save the block in the result
                    result.push((block, signatures));
                }
            }
        }
        Some(result)
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
                    .map(|metadata| metadata.block.header().era_id())
            })
            .collect()
    }

    fn handle_got_past_blocks_with_metadata<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        past_blocks_with_metadata: Vec<Option<BlockWithMetadata>>,
        proposed_block: ProposedBlock<ClContext>,
        sender: NodeId,
    ) -> Effects<Event>
    where
        REv: From<Event> + From<FetcherRequest<FinalitySignature>> + From<FatalAnnouncement> + Send,
    {
        let Some(block_validation_state) = self.validation_states.get_mut(&proposed_block)
        else {
            error!(
                "handle_got_past_blocks_with_metadata called for a block with no \
                    corresponding state!"
            );
            return Effects::new();
        };

        let rewarded_signatures = proposed_block.value().rewarded_signatures();

        let Some(blocks_and_signatures) =
            Self::relevant_blocks_and_cited_signatures(
                block_validation_state,
                &past_blocks_with_metadata,
                rewarded_signatures,
                sender,
            ) else {
                return Effects::new();
            };

        // If we're here, we have all the blocks necessary to proceed with validating finality
        // signatures.
        let era_ids_vec = Self::era_ids_vec(&past_blocks_with_metadata);

        // get the set of unique era ids that are present in the cited blocks
        let era_ids: BTreeSet<_> = era_ids_vec.iter().flatten().copied().collect();
        let validator_matrix = &self.validator_matrix;

        let validators: BTreeMap<_, BTreeSet<_>> = era_ids
            .into_iter()
            .filter_map(move |era_id| {
                validator_matrix
                    .validator_weights(era_id)
                    .map(|weights| (era_id, weights.into_validator_public_keys().collect()))
            })
            .collect();

        // This will be a set of signature IDs of the signatures included in the block, but not
        // found in metadata in storage.
        let mut missing_sigs = HashSet::new();
        for (block_with_metadata, single_block_rewarded_sigs) in blocks_and_signatures {
            let era_id = block_with_metadata.block.header().era_id();
            let Some(all_validators) = validators.get(&era_id) else {
                return fatal!(effect_builder, "couldn't get validators for {}", era_id).ignore();
            };
            let public_keys = single_block_rewarded_sigs
                .clone()
                .into_validator_set(all_validators.iter().cloned());
            let block_hash = *block_with_metadata.block.hash();
            let era_id = block_with_metadata.block.header().era_id();
            missing_sigs.extend(
                public_keys
                    .into_iter()
                    .filter(move |public_key| {
                        !block_with_metadata
                            .block_signatures
                            .proofs
                            .contains_key(public_key)
                    })
                    .map(move |public_key| FinalitySignatureId {
                        block_hash,
                        era_id,
                        public_key,
                    }),
            );
        }
        trace!(
            ?missing_sigs,
            "handle_got_past_blocks_with_metadata included_sigs"
        );

        let mut in_flight = KeyedCounter::default();
        let effects = missing_sigs
            .iter()
            .flat_map(|sig_id| {
                // For every request, increase the number of in-flight...
                in_flight.inc(sig_id);
                // ...then request it.
                fetch_signature(
                    effect_builder,
                    proposed_block.clone(),
                    Box::new(sig_id.clone()),
                    sender,
                )
            })
            .collect();

        block_validation_state
            .signatures_validation_state
            .require_signatures(missing_sigs, in_flight);

        effects
    }
}

impl<REv> Component<REv> for BlockValidator
where
    REv: From<Event>
        + From<BlockValidationRequest>
        + From<FetcherRequest<LegacyDeploy>>
        + From<FetcherRequest<FinalitySignature>>
        + From<StorageRequest>
        + From<FatalAnnouncement>
        + Send,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        let mut effects = Effects::new();
        match event {
            Event::Request(BlockValidationRequest {
                proposed_block_era_id,
                proposed_block_height,
                block,
                sender,
                responder,
            }) => {
                if block.deploy_hashes().count()
                    > self.chainspec.deploy_config.block_max_deploy_count as usize
                {
                    return responder.respond(false).ignore();
                }
                if block.transfer_hashes().count()
                    > self.chainspec.deploy_config.block_max_transfer_count as usize
                {
                    return responder.respond(false).ignore();
                }

                let deploy_count = block.deploy_hashes().count() + block.transfer_hashes().count();

                // Collect the deploys in a map. If they are fewer now, then there was a duplicate!
                let block_deploys: HashMap<_, _> = block.deploys_and_transfers_iter().collect();
                if block_deploys.len() != deploy_count {
                    self.log_block_with_replay(sender, &block);
                    return responder.respond(false).ignore();
                }

                let block_timestamp = block.timestamp();
                let state =
                    self.validation_states
                        .entry(block.clone())
                        .or_insert(BlockValidationState {
                            proposed_block_era_id,
                            proposed_block_height,
                            appendable_block: AppendableBlock::new(
                                self.chainspec.deploy_config,
                                block_timestamp,
                            ),
                            missing_deploys: block_deploys.clone(),
                            signatures_validation_state: SignaturesValidationState::Pending,
                            responders: smallvec![],
                        });

                // We register ourselves as someone interested in the ultimate validation result.
                state.responders.push(responder);

                let signature_rewards_max_delay =
                    self.chainspec.core_config.signature_rewards_max_delay;
                let minimum_block_height =
                    proposed_block_height.saturating_sub(signature_rewards_max_delay);

                // If the block cites any finality signatures, start the signature validation
                // process.
                if block
                    .value()
                    .rewarded_signatures()
                    .iter()
                    .any(|sigs| sigs.has_some())
                {
                    debug!(
                        ?block, %minimum_block_height, %proposed_block_height,
                        "block cites signatures, validation required - requesting past blocks \
                        from storage"
                    );
                    let proposed_block = block.clone();
                    effects.extend(
                        effect_builder
                            .collect_past_blocks_with_metadata(
                                minimum_block_height..proposed_block_height,
                                false,
                            )
                            .event(move |past_blocks_with_metadata| {
                                Event::GotPastBlocksWithMetadata {
                                    past_blocks_with_metadata,
                                    proposed_block,
                                    sender,
                                }
                            }),
                    );
                } else {
                    // If no signatures are cited, the citation is automatically valid.
                    debug!("no signatures included in block, no validation of signatures required");
                    state
                        .signatures_validation_state
                        .require_signatures(HashSet::new(), KeyedCounter::default());
                }

                if state.is_valid() {
                    // If the state is already valid (because either we downloaded all the deploys
                    // and signatures already, or the block contains no deploys and no cited
                    // signatures), we can do an early return to caller.
                    let effects = state.respond(true);
                    self.validation_states.remove(&block);
                    return effects;
                }

                effects.extend(block_deploys.into_iter().flat_map(|(dt_hash, _)| {
                    // For every request, increase the number of in-flight...
                    self.in_flight.inc(&dt_hash.into());
                    // ...then request it.
                    fetch_deploy(effect_builder, dt_hash, sender)
                }));
            }
            Event::GotPastBlocksWithMetadata {
                past_blocks_with_metadata,
                proposed_block,
                sender,
            } => {
                effects.extend(self.handle_got_past_blocks_with_metadata(
                    effect_builder,
                    past_blocks_with_metadata,
                    proposed_block,
                    sender,
                ));
            }
            Event::BlockStored(block_height) => {
                // switch any signature validation states that are awaiting this block back to
                // Pending
                for (proposed_block, validation_state) in &mut self.validation_states {
                    if let Some(sender) = validation_state
                        .signatures_validation_state
                        .block_stored(block_height)
                    {
                        let proposed_block_height = validation_state.proposed_block_height;
                        let signature_rewards_max_delay =
                            self.chainspec.core_config.signature_rewards_max_delay;
                        let minimum_block_height =
                            proposed_block_height.saturating_sub(signature_rewards_max_delay);

                        let proposed_block = proposed_block.clone();
                        effects.extend(
                            effect_builder
                                .collect_past_blocks_with_metadata(
                                    minimum_block_height..proposed_block_height,
                                    false,
                                )
                                .event(move |past_blocks_with_metadata| {
                                    Event::GotPastBlocksWithMetadata {
                                        past_blocks_with_metadata,
                                        proposed_block,
                                        sender,
                                    }
                                }),
                        );
                    }
                }
            }
            Event::DeployFound {
                dt_hash,
                deploy_footprint,
            } => {
                // We successfully found a hash. Decrease the number of outstanding requests.
                self.in_flight.dec(&dt_hash.into());

                // If a deploy is received for a given block that makes that block invalid somehow,
                // mark it for removal.
                let mut invalid = Vec::new();

                // Our first pass updates all validation states, crossing off the found deploy.
                for (key, state) in self.validation_states.iter_mut() {
                    if let Some(approvals) = state.missing_deploys.remove(&dt_hash) {
                        // If the deploy is of the wrong type or would be invalid for this block,
                        // notify everyone still waiting on it that all is lost.
                        let add_result = match dt_hash {
                            DeployOrTransferHash::Deploy(hash) => {
                                state.appendable_block.add_deploy(
                                    DeployHashWithApprovals::new(hash, approvals.clone()),
                                    &deploy_footprint,
                                )
                            }
                            DeployOrTransferHash::Transfer(hash) => {
                                state.appendable_block.add_transfer(
                                    DeployHashWithApprovals::new(hash, approvals.clone()),
                                    &deploy_footprint,
                                )
                            }
                        };
                        if let Err(err) = add_result {
                            info!(block = ?key, %dt_hash, ?deploy_footprint, ?err, "block invalid");
                            invalid.push(key.clone());
                        }
                    }
                }

                // Now we remove all states that have finished and notify the requesters.
                self.validation_states.retain(|key, state| {
                    if invalid.contains(key) {
                        effects.extend(state.respond(false));
                        return false;
                    }
                    if state.is_valid() {
                        // This one is done and valid.
                        effects.extend(state.respond(true));
                        return false;
                    }
                    true
                });
            }
            Event::DeployMissing(dt_hash) => {
                info!(%dt_hash, "request to download deploy timed out");
                // A deploy failed to fetch. If there is still hope (i.e. other outstanding
                // requests), we just ignore this little accident.
                if self.in_flight.dec(&dt_hash.into()) != 0 {
                    return Effects::new();
                }

                self.validation_states.retain(|key, state| {
                    if !state.missing_deploys.contains_key(&dt_hash) {
                        return true;
                    }

                    // Notify everyone still waiting on it that all is lost.
                    info!(block = ?key, %dt_hash, "could not validate the deploy. block is invalid");
                    // This validation state contains a deploy hash we failed to fetch from all
                    // sources, it can never succeed.
                    effects.extend(state.respond(false));
                    false
                });
            }
            Event::CannotConvertDeploy(dt_hash) => {
                // Deploy is invalid. There's no point waiting for other in-flight requests to
                // finish.
                self.in_flight.dec(&dt_hash.into());

                self.validation_states.retain(|key, state| {
                    if state.missing_deploys.contains_key(&dt_hash) {
                        // Notify everyone still waiting on it that all is lost.
                        info!(
                            block = ?key, %dt_hash,
                            "could not convert deploy to deploy type. block is invalid"
                        );
                        // This validation state contains a failed deploy hash, it can never
                        // succeed.
                        effects.extend(state.respond(false));
                        false
                    } else {
                        true
                    }
                });
            }
            Event::SignatureFound(proposed_block, signature_id) => {
                if let Some(validation_state) = self.validation_states.get_mut(&proposed_block) {
                    validation_state
                        .signatures_validation_state
                        .signature_fetched(*signature_id);
                    if validation_state.is_valid() {
                        effects.extend(validation_state.respond(true));
                        self.validation_states.remove(&proposed_block);
                    }
                } else {
                    warn!(
                        ?signature_id,
                        ?proposed_block,
                        "downloaded a signature for a block with no validation state"
                    );
                }
            }
            Event::SignatureError(proposed_block, signature_id) => {
                if let Some(validation_state) = self.validation_states.get_mut(&proposed_block) {
                    validation_state
                        .signatures_validation_state
                        .signature_fetch_failed(*signature_id);
                    if validation_state.is_invalid() {
                        effects.extend(validation_state.respond(false));
                        self.validation_states.remove(&proposed_block);
                    }
                } else {
                    warn!(
                        ?signature_id,
                        ?proposed_block,
                        "got an error for a signature for a block with no validation state"
                    );
                }
            }
        }
        effects
    }

    fn name(&self) -> &str {
        COMPONENT_NAME
    }
}

/// Returns effects that fetch the deploy and validate it.
fn fetch_deploy<REv>(
    effect_builder: EffectBuilder<REv>,
    dt_hash: DeployOrTransferHash,
    sender: NodeId,
) -> Effects<Event>
where
    REv: From<Event> + From<FetcherRequest<LegacyDeploy>> + Send,
{
    async move {
        let deploy_hash: DeployHash = dt_hash.into();
        let deploy = match effect_builder
            .fetch::<LegacyDeploy>(deploy_hash, sender, Box::new(EmptyValidationMetadata))
            .await
        {
            Ok(FetchedData::FromStorage { item }) | Ok(FetchedData::FromPeer { item, .. }) => {
                Deploy::from(*item)
            }
            Err(fetcher_error) => {
                warn!(
                    "Could not fetch deploy with deploy hash {}: {}",
                    deploy_hash, fetcher_error
                );
                return Event::DeployMissing(dt_hash);
            }
        };
        if deploy.deploy_or_transfer_hash() != dt_hash {
            warn!(
                deploy = ?deploy,
                expected_deploy_or_transfer_hash = ?dt_hash,
                actual_deploy_or_transfer_hash = ?deploy.deploy_or_transfer_hash(),
                "Deploy has incorrect transfer hash"
            );
            return Event::CannotConvertDeploy(dt_hash);
        }
        match deploy.footprint() {
            Ok(deploy_footprint) => Event::DeployFound {
                dt_hash,
                deploy_footprint: Box::new(deploy_footprint),
            },
            Err(error) => {
                warn!(
                    deploy = ?deploy,
                    deploy_or_transfer_hash = ?dt_hash,
                    ?error,
                    "Could not convert deploy",
                );
                Event::CannotConvertDeploy(dt_hash)
            }
        }
    }
    .event(std::convert::identity)
}

/// Returns effects that fetch the signature and validate it.
fn fetch_signature<REv>(
    effect_builder: EffectBuilder<REv>,
    proposed_block: ProposedBlock<ClContext>,
    signature_id: Box<FinalitySignatureId>,
    sender: NodeId,
) -> Effects<Event>
where
    REv: From<Event> + From<FetcherRequest<FinalitySignature>> + Send,
{
    async move {
        let signature = match effect_builder
            .fetch::<FinalitySignature>(
                signature_id.clone(),
                sender,
                Box::new(EmptyValidationMetadata),
            )
            .await
        {
            Ok(FetchedData::FromStorage { item }) | Ok(FetchedData::FromPeer { item, .. }) => *item,
            Err(fetcher_error) => {
                warn!(
                    "Could not fetch finality signature for block {} in era {} by {}: {}",
                    signature_id.block_hash,
                    signature_id.era_id,
                    signature_id.public_key,
                    fetcher_error
                );
                return Event::SignatureError(proposed_block, signature_id);
            }
        };
        if *signature.fetch_id() != *signature_id {
            warn!(
                signature = ?signature,
                expected_signature_id = ?signature_id,
                actual_signature_id = ?signature.fetch_id(),
                "Signature has an incorrect ID"
            );
            return Event::SignatureError(proposed_block, signature_id);
        }
        Event::SignatureFound(proposed_block, signature_id)
    }
    .event(std::convert::identity)
}
