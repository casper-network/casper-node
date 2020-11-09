mod round_success_meter;

use std::{
    any::Any,
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Debug,
};

use casper_execution_engine::shared::motes::Motes;
use datasize::DataSize;
use itertools::Itertools;
use num_traits::AsPrimitive;
use serde::{Deserialize, Serialize};
use tracing::{info, trace};

use self::round_success_meter::RoundSuccessMeter;
use casper_types::{auction::BLOCK_REWARD, U512};

use crate::{
    components::{
        chainspec_loader::Chainspec,
        consensus::{
            consensus_protocol::{BlockContext, ConsensusProtocol, ConsensusProtocolResult},
            highway_core::{
                active_validator::Effect as AvEffect,
                finality_detector::FinalityDetector,
                highway::{
                    Dependency, GetDepOutcome, Highway, Params, PreValidatedVertex, ValidVertex,
                    Vertex,
                },
                validators::Validators,
            },
            traits::{Context, NodeIdT},
        },
    },
    types::Timestamp,
    NodeRng,
};

#[derive(DataSize, Debug)]
pub(crate) struct HighwayProtocol<I, C>
where
    C: Context,
{
    /// Incoming vertices we can't add yet because they are still missing a dependency.
    vertex_deps: BTreeMap<Dependency<C>, Vec<(I, PreValidatedVertex<C>)>>,
    /// Incoming blocks we can't add yet because we are waiting for validation.
    pending_values: HashMap<C::ConsensusValue, Vec<ValidVertex<C>>>,
    finality_detector: FinalityDetector<C>,
    highway: Highway<C>,
    /// The vertices that are scheduled to be processed at a later time.  The keys of this
    /// `BTreeMap` are timestamps when the corresponding vector of vertices will be added.
    vertices_to_be_added_later: BTreeMap<Timestamp, Vec<(I, PreValidatedVertex<C>)>>,
    /// A tracker for whether we are keeping up with the current round exponent or not.
    round_success_meter: RoundSuccessMeter<C>,
}

impl<I: NodeIdT, C: Context + 'static> HighwayProtocol<I, C> {
    /// Creates a new boxed `HighwayProtocol` instance.
    pub(crate) fn new_boxed(
        instance_id: C::InstanceId,
        validator_stakes: Vec<(C::ValidatorId, Motes)>,
        slashed: &HashSet<C::ValidatorId>,
        chainspec: &Chainspec,
        prev_cp: Option<&dyn ConsensusProtocol<I, C>>,
        start_time: Timestamp,
        seed: u64,
    ) -> Box<dyn ConsensusProtocol<I, C>> {
        let sum_stakes: Motes = validator_stakes.iter().map(|(_, stake)| *stake).sum();
        assert!(
            !sum_stakes.value().is_zero(),
            "cannot start era with total weight 0"
        );
        // For Highway, we need u64 weights. Scale down by  sum / u64::MAX,  rounded up.
        // If we round up the divisor, the resulting sum is guaranteed to be  <= u64::MAX.
        let scaling_factor = (sum_stakes.value() + U512::from(u64::MAX) - 1) / U512::from(u64::MAX);
        let scale_stake = |(key, stake): (C::ValidatorId, Motes)| {
            (key, AsPrimitive::<u64>::as_(stake.value() / scaling_factor))
        };
        let mut validators: Validators<C::ValidatorId> =
            validator_stakes.into_iter().map(scale_stake).collect();

        for vid in slashed {
            validators.ban(vid);
        }

        // TODO: Apply all upgrades with a height less than or equal to the start height.
        let highway_config = &chainspec.genesis.highway_config;

        let total_weight = u128::from(validators.total_weight());
        let ftt_percent = u128::from(highway_config.finality_threshold_percent);
        let ftt = ((total_weight * ftt_percent / 100) as u64).into();

        let init_round_exp = prev_cp
            .and_then(|cp| cp.as_any().downcast_ref::<HighwayProtocol<I, C>>())
            .and_then(|highway_proto| highway_proto.median_round_exp())
            .unwrap_or(highway_config.minimum_round_exponent);

        info!(
            %init_round_exp,
            "initializing Highway instance",
        );

        let params = Params::new(
            seed,
            BLOCK_REWARD,
            BLOCK_REWARD / 5, // TODO: Make reduced block reward configurable?
            highway_config.minimum_round_exponent,
            highway_config.maximum_round_exponent,
            init_round_exp,
            highway_config.minimum_era_height,
            start_time,
            start_time + highway_config.era_duration,
        );

        let min_round_exp = params.min_round_exp();
        let max_round_exp = params.max_round_exp();
        let round_exp = params.init_round_exp();
        let start_timestamp = params.start_timestamp();
        Box::new(HighwayProtocol {
            vertex_deps: BTreeMap::new(),
            pending_values: HashMap::new(),
            finality_detector: FinalityDetector::new(ftt),
            highway: Highway::new(instance_id, validators, params),
            vertices_to_be_added_later: BTreeMap::new(),
            round_success_meter: RoundSuccessMeter::new(
                round_exp,
                min_round_exp,
                max_round_exp,
                start_timestamp,
            ),
        })
    }

    fn process_av_effects<E>(&mut self, av_effects: E) -> Vec<CpResult<I, C>>
    where
        E: IntoIterator<Item = AvEffect<C>>,
    {
        av_effects
            .into_iter()
            .flat_map(|effect| self.process_av_effect(effect))
            .collect()
    }

    fn process_av_effect(&mut self, effect: AvEffect<C>) -> Vec<CpResult<I, C>> {
        match effect {
            AvEffect::NewVertex(vv) => {
                self.calculate_round_exponent(&vv);
                self.process_new_vertex(vv.into())
            }
            AvEffect::ScheduleTimer(timestamp) => {
                vec![ConsensusProtocolResult::ScheduleTimer(timestamp)]
            }
            AvEffect::RequestNewBlock(block_context) => {
                vec![ConsensusProtocolResult::CreateNewBlock { block_context }]
            }
            AvEffect::WeEquivocated(evidence) => {
                panic!("this validator equivocated: {:?}", evidence);
            }
        }
    }

    fn process_new_vertex(&mut self, v: Vertex<C>) -> Vec<CpResult<I, C>> {
        let mut results = Vec::new();
        if let Vertex::Evidence(ev) = &v {
            let v_id = self
                .highway
                .validators()
                .id(ev.perpetrator())
                .expect("validator not found")
                .clone();
            results.push(ConsensusProtocolResult::NewEvidence(v_id));
        }
        let msg = HighwayMessage::NewVertex(v);
        results.push(ConsensusProtocolResult::CreatedGossipMessage(
            bincode::serialize(&msg).expect("should serialize message"),
        ));
        results.extend(self.detect_finality());
        results
    }

    fn detect_finality(&mut self) -> impl Iterator<Item = CpResult<I, C>> + '_ {
        self.finality_detector
            .run(&self.highway)
            .expect("too many faulty validators")
            .map(ConsensusProtocolResult::FinalizedBlock)
    }

    /// Store a (pre-validated) vertex which will be added later.  This creates a timer to be sent
    /// to the reactor. The vertex be added using `Self::add_vertices` when that timer goes off.
    fn store_vertex_for_addition_later(
        &mut self,
        future_timestamp: Timestamp,
        sender: I,
        pvv: PreValidatedVertex<C>,
    ) -> Vec<CpResult<I, C>> {
        self.vertices_to_be_added_later
            .entry(future_timestamp)
            .or_insert_with(Vec::new)
            .push((sender, pvv));
        vec![ConsensusProtocolResult::ScheduleTimer(future_timestamp)]
    }

    /// Call `Self::add_vertices` on any vertices in `vertices_to_be_added_later` which are
    /// scheduled for after the given `transpired_timestamp`.  In general the specified
    /// `transpired_timestamp` is approximately `Timestamp::now()`.  Vertices keyed by timestamps
    /// chronologically before `transpired_timestamp` should all be added.
    fn add_past_due_stored_vertices(
        &mut self,
        timestamp: Timestamp,
        rng: &mut NodeRng,
    ) -> Vec<CpResult<I, C>> {
        let mut results = vec![];
        let past_due_timestamps: Vec<Timestamp> = self
            .vertices_to_be_added_later
            .range(..=timestamp) // Inclusive range
            .map(|(past_due_timestamp, _)| past_due_timestamp.to_owned())
            .collect();
        for past_due_timestamp in past_due_timestamps {
            if let Some(vertices_to_add) =
                self.vertices_to_be_added_later.remove(&past_due_timestamp)
            {
                results.extend(self.add_vertices(vertices_to_add, rng))
            }
        }
        results
    }

    /// Adds the given vertices to the protocol state, if possible, or requests missing
    /// dependencies or validation. Recursively adds everything that is unblocked now.
    fn add_vertices(
        &mut self,
        mut pvvs: Vec<(I, PreValidatedVertex<C>)>,
        rng: &mut NodeRng,
    ) -> Vec<CpResult<I, C>> {
        // TODO: Is there a danger that this takes too much time, and starves other
        // components and events? Consider replacing the loop with a "callback" effect:
        // Instead of handling `HighwayMessage::NewVertex(v)` directly, return a
        // `EnqueueVertex(v)` that causes the reactor to call us with an
        // `Event::NewVertex(v)`, and call `add_vertex` when handling that event. For each
        // returned vertex that needs to be requeued, also return an `EnqueueVertex`
        // effect.
        let mut results = Vec::new();
        while !pvvs.is_empty() {
            let mut state_changed = false;
            for (sender, pvv) in pvvs.drain(..) {
                if self.highway.has_vertex(pvv.inner()) {
                    continue; // Vertex is already in the protocol state. Ignore.
                } else if let Some(dep) = self.highway.missing_dependency(&pvv) {
                    // Store it in the map and request the missing dependency from the sender.
                    self.vertex_deps
                        .entry(dep.clone())
                        .or_default()
                        .push((sender.clone(), pvv));
                    let msg = HighwayMessage::RequestDependency(dep);
                    results.push(ConsensusProtocolResult::CreatedTargetedMessage(
                        bincode::serialize(&msg).expect("should serialize message"),
                        sender,
                    ));
                } else {
                    match self.highway.validate_vertex(pvv) {
                        Ok(vv) => {
                            let vertex = vv.inner();
                            if let (Some(value), Some(timestamp)) =
                                (vertex.value().cloned(), vertex.timestamp())
                            {
                                // It's a block: Request validation before adding it to the state.
                                self.pending_values
                                    .entry(value.clone())
                                    .or_default()
                                    .push(vv);
                                results.push(ConsensusProtocolResult::ValidateConsensusValue(
                                    sender, value, timestamp,
                                ));
                            } else {
                                // It's not a block: Add it to the state.
                                let now = Timestamp::now();
                                results.extend(self.add_valid_vertex(vv, rng, now));
                                state_changed = true;
                            }
                        }
                        Err((pvv, err)) => {
                            info!(?pvv, ?err, "invalid vertex");
                            // TODO: Disconnect from senders!
                        }
                    }
                }
            }
            if state_changed {
                // If we added new vertices to the state, check whether any dependencies we were
                // waiting for are now satisfied, and try adding the pending vertices as well.
                pvvs.extend(self.remove_satisfied_deps());
                // Check whether any new blocks were finalized.
                results.extend(self.detect_finality());
            }
        }
        results
    }

    fn calculate_round_exponent(&mut self, vv: &ValidVertex<C>) {
        let new_round_exp = self
            .round_success_meter
            .calculate_new_exponent(self.highway.state());
        // If the vertex contains a proposal, register it in the success meter.
        // It's important to do this _after_ the calculation above - otherwise we might try to
        // register the proposal before the meter is aware that a new round has started, and it
        // will reject the proposal.
        if vv.is_proposal() {
            // unwraps are safe, as if value is `Some`, this is already a vote
            trace!(
                now = Timestamp::now().millis(),
                timestamp = vv.inner().timestamp().unwrap().millis(),
                "adding proposal to protocol state",
            );
            self.round_success_meter.new_proposal(
                vv.inner().vote_hash().unwrap(),
                vv.inner().timestamp().unwrap(),
            );
        }
        self.highway.set_round_exp(new_round_exp);
    }

    fn add_valid_vertex(
        &mut self,
        vv: ValidVertex<C>,
        rng: &mut NodeRng,
        now: Timestamp,
    ) -> Vec<CpResult<I, C>> {
        // Check whether we should change the round exponent.
        // It's important to do it before the vertex is added to the state - this way if the last
        // round has finished, we now have all the vertices from that round in the state, and no
        // newer ones.
        self.calculate_round_exponent(&vv);
        let av_effects = self.highway.add_valid_vertex(vv.clone(), rng, now);
        let mut results = self.process_av_effects(av_effects);
        let msg = HighwayMessage::NewVertex(vv.into());
        results.push(ConsensusProtocolResult::CreatedGossipMessage(
            bincode::serialize(&msg).expect("should serialize message"),
        ));
        results
    }

    fn remove_satisfied_deps(&mut self) -> impl Iterator<Item = (I, PreValidatedVertex<C>)> + '_ {
        let satisfied_deps = self
            .vertex_deps
            .keys()
            .filter(|dep| self.highway.has_dependency(dep))
            .cloned()
            .collect_vec();
        satisfied_deps
            .into_iter()
            .flat_map(move |dep| self.vertex_deps.remove(&dep).unwrap())
    }

    /// Returns the median round exponent of all the validators that haven't been observed to be
    /// malicious, as seen by the current panorama.
    /// Returns `None` if there are no correct validators in the panorama.
    pub(crate) fn median_round_exp(&self) -> Option<u8> {
        self.highway.state().median_round_exp()
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
enum HighwayMessage<C: Context> {
    NewVertex(Vertex<C>),
    RequestDependency(Dependency<C>),
}
type CpResult<I, C> = ConsensusProtocolResult<I, C>;

impl<I, C> ConsensusProtocol<I, C> for HighwayProtocol<I, C>
where
    I: NodeIdT,
    C: Context + 'static,
{
    fn handle_message(
        &mut self,
        sender: I,
        msg: Vec<u8>,
        evidence_only: bool,
        rng: &mut NodeRng,
    ) -> Vec<CpResult<I, C>> {
        match bincode::deserialize(msg.as_slice()) {
            Err(err) => vec![ConsensusProtocolResult::InvalidIncomingMessage(
                msg,
                sender,
                err.into(),
            )],
            Ok(HighwayMessage::NewVertex(v))
                if self.highway.has_vertex(&v) || (evidence_only && !v.is_evidence()) =>
            {
                vec![]
            }
            Ok(HighwayMessage::NewVertex(v)) => {
                let pvv = match self.highway.pre_validate_vertex(v) {
                    Ok(pvv) => pvv,
                    Err((_, err)) => {
                        // TODO: Disconnect from senders.
                        return vec![ConsensusProtocolResult::InvalidIncomingMessage(
                            msg,
                            sender,
                            err.into(),
                        )];
                    }
                };
                match pvv.timestamp() {
                    Some(timestamp) if timestamp > Timestamp::now() => {
                        self.store_vertex_for_addition_later(timestamp, sender, pvv)
                    }
                    _ => self.add_vertices(vec![(sender, pvv)], rng),
                }
            }
            Ok(HighwayMessage::RequestDependency(dep)) => match self.highway.get_dependency(&dep) {
                GetDepOutcome::None => {
                    info!(?dep, ?sender, "requested dependency doesn't exist");
                    vec![]
                }
                GetDepOutcome::Evidence(vid) => {
                    vec![ConsensusProtocolResult::SendEvidence(sender, vid)]
                }
                GetDepOutcome::Vertex(vv) => {
                    let msg = HighwayMessage::NewVertex(vv.into());
                    let serialized_msg =
                        bincode::serialize(&msg).expect("should serialize message");
                    // TODO: Should this be done via a gossip service?
                    vec![ConsensusProtocolResult::CreatedTargetedMessage(
                        serialized_msg,
                        sender,
                    )]
                }
            },
        }
    }

    fn handle_timer(&mut self, timestamp: Timestamp, rng: &mut NodeRng) -> Vec<CpResult<I, C>> {
        let effects = self.highway.handle_timer(timestamp, rng);
        let mut results = self.process_av_effects(effects);
        results.extend(self.add_past_due_stored_vertices(timestamp, rng));
        results
    }

    fn propose(
        &mut self,
        value: C::ConsensusValue,
        block_context: BlockContext,
        rng: &mut NodeRng,
    ) -> Vec<CpResult<I, C>> {
        let effects = self.highway.propose(value, block_context, rng);
        self.process_av_effects(effects)
    }

    fn resolve_validity(
        &mut self,
        value: &C::ConsensusValue,
        valid: bool,
        rng: &mut NodeRng,
    ) -> Vec<CpResult<I, C>> {
        if valid {
            let mut results = self
                .pending_values
                .remove(value)
                .into_iter()
                .flatten()
                .flat_map(|vv| {
                    let now = Timestamp::now();
                    self.add_valid_vertex(vv, rng, now)
                })
                .collect_vec();
            let satisfied_pvvs = self.remove_satisfied_deps().collect();
            results.extend(self.add_vertices(satisfied_pvvs, rng));
            results.extend(self.detect_finality());
            results
        } else {
            // TODO: Slash proposer?
            // TODO: Drop dependent vertices? Or add timeout.
            // TODO: Disconnect from senders.
            vec![]
        }
    }

    fn activate_validator(
        &mut self,
        our_id: C::ValidatorId,
        secret: C::ValidatorSecret,
        timestamp: Timestamp,
    ) -> Vec<CpResult<I, C>> {
        let av_effects = self.highway.activate_validator(our_id, secret, timestamp);
        self.process_av_effects(av_effects)
    }

    fn deactivate_validator(&mut self) {
        self.highway.deactivate_validator()
    }

    fn has_evidence(&self, vid: &C::ValidatorId) -> bool {
        self.highway.has_evidence(vid)
    }

    fn mark_faulty(&mut self, vid: &C::ValidatorId) {
        self.highway.mark_faulty(vid);
    }

    fn request_evidence(&self, sender: I, vid: &C::ValidatorId) -> Vec<CpResult<I, C>> {
        self.highway
            .validators()
            .get_index(vid)
            .and_then(
                move |vidx| match self.highway.get_dependency(&Dependency::Evidence(vidx)) {
                    GetDepOutcome::None | GetDepOutcome::Evidence(_) => None,
                    GetDepOutcome::Vertex(vv) => {
                        let msg = HighwayMessage::NewVertex(vv.into());
                        let serialized_msg =
                            bincode::serialize(&msg).expect("should serialize message");
                        Some(ConsensusProtocolResult::CreatedTargetedMessage(
                            serialized_msg,
                            sender,
                        ))
                    }
                },
            )
            .into_iter()
            .collect()
    }

    fn validators_with_evidence(&self) -> Vec<&C::ValidatorId> {
        self.highway.validators_with_evidence().collect()
    }

    fn has_received_messages(&self) -> bool {
        self.highway.state().is_empty()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
