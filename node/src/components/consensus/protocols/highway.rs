mod round_success_meter;
#[cfg(test)]
mod tests;

use std::{
    any::Any,
    collections::{BTreeMap, HashMap, HashSet},
    fmt::Debug,
    iter,
    path::PathBuf,
};

use datasize::DataSize;
use itertools::Itertools;
use num_traits::AsPrimitive;
use serde::{Deserialize, Serialize};
use tracing::{error, info, trace, warn};

use self::round_success_meter::RoundSuccessMeter;
use casper_types::{auction::BLOCK_REWARD, U512};

use crate::{
    components::consensus::{
        config::ProtocolConfig,
        consensus_protocol::{BlockContext, ConsensusProtocol, ProtocolOutcome},
        highway_core::{
            active_validator::Effect as AvEffect,
            finality_detector::FinalityDetector,
            highway::{
                Dependency, GetDepOutcome, Highway, Params, PreValidatedVertex, ValidVertex, Vertex,
            },
            validators::Validators,
        },
        traits::{ConsensusValueT, Context, NodeIdT},
    },
    types::Timestamp,
    NodeRng,
};

/// Never allow more than this many units in a piece of evidence for conflicting endorsements,
/// even if eras are longer than this.
const MAX_ENDORSEMENT_EVIDENCE_LIMIT: u64 = 10000;

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
        validator_stakes: BTreeMap<C::ValidatorId, U512>,
        slashed: &HashSet<C::ValidatorId>,
        protocol_config: &ProtocolConfig,
        prev_cp: Option<&dyn ConsensusProtocol<I, C>>,
        start_time: Timestamp,
        seed: u64,
    ) -> Box<dyn ConsensusProtocol<I, C>> {
        let sum_stakes: U512 = validator_stakes.iter().map(|(_, stake)| *stake).sum();
        assert!(
            !sum_stakes.is_zero(),
            "cannot start era with total weight 0"
        );
        // For Highway, we need u64 weights. Scale down by  sum / u64::MAX,  rounded up.
        // If we round up the divisor, the resulting sum is guaranteed to be  <= u64::MAX.
        let scaling_factor = (sum_stakes + U512::from(u64::MAX) - 1) / U512::from(u64::MAX);
        let scale_stake = |(key, stake): (C::ValidatorId, U512)| {
            (key, AsPrimitive::<u64>::as_(stake / scaling_factor))
        };
        let mut validators: Validators<C::ValidatorId> =
            validator_stakes.into_iter().map(scale_stake).collect();

        for vid in slashed {
            validators.ban(vid);
        }

        // TODO: Apply all upgrades with a height less than or equal to the start height.
        let highway_config = &protocol_config.highway_config;

        let total_weight = u128::from(validators.total_weight());
        let ftt_fraction = highway_config.finality_threshold_fraction;
        let ftt = ((total_weight * *ftt_fraction.numer() as u128 / *ftt_fraction.denom() as u128)
            as u64)
            .into();

        let init_round_exp = prev_cp
            .and_then(|cp| cp.as_any().downcast_ref::<HighwayProtocol<I, C>>())
            .and_then(|highway_proto| highway_proto.median_round_exp())
            .unwrap_or(highway_config.minimum_round_exponent);

        info!(
            %init_round_exp,
            "initializing Highway instance",
        );

        // Allow about as many units as part of evidence for conflicting endorsements as we expect
        // a validator to create during an era. After that, they can endorse two conflicting forks
        // without getting slashed.
        let min_round_len = 1 << highway_config.minimum_round_exponent;
        let min_rounds_per_era = highway_config
            .minimum_era_height
            .max(1 + highway_config.era_duration.millis() / min_round_len);
        let endorsement_evidence_limit =
            (2 * min_rounds_per_era).min(MAX_ENDORSEMENT_EVIDENCE_LIMIT);

        let params = Params::new(
            seed,
            BLOCK_REWARD,
            (highway_config.reduced_reward_multiplier * BLOCK_REWARD).to_integer(),
            highway_config.minimum_round_exponent,
            highway_config.maximum_round_exponent,
            init_round_exp,
            highway_config.minimum_era_height,
            start_time,
            start_time + highway_config.era_duration,
            endorsement_evidence_limit,
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

    fn process_av_effects<E>(&mut self, av_effects: E) -> Vec<ProtocolOutcome<I, C>>
    where
        E: IntoIterator<Item = AvEffect<C>>,
    {
        av_effects
            .into_iter()
            .flat_map(|effect| self.process_av_effect(effect))
            .collect()
    }

    fn process_av_effect(&mut self, effect: AvEffect<C>) -> Vec<ProtocolOutcome<I, C>> {
        match effect {
            AvEffect::NewVertex(vv) => {
                self.calculate_round_exponent(&vv);
                self.process_new_vertex(vv.into())
            }
            AvEffect::ScheduleTimer(timestamp) => vec![ProtocolOutcome::ScheduleTimer(timestamp)],
            AvEffect::RequestNewBlock {
                block_context,
                fork_choice,
            } => {
                let past_values = self.non_finalized_values(fork_choice).cloned().collect();
                vec![ProtocolOutcome::CreateNewBlock {
                    block_context,
                    past_values,
                }]
            }
            AvEffect::WeAreFaulty(fault) => {
                error!("this validator is faulty: {:?}", fault);
                vec![ProtocolOutcome::WeAreFaulty]
            }
        }
    }

    fn process_new_vertex(&mut self, v: Vertex<C>) -> Vec<ProtocolOutcome<I, C>> {
        let mut results = Vec::new();
        if let Vertex::Evidence(ev) = &v {
            let v_id = self
                .highway
                .validators()
                .id(ev.perpetrator())
                .expect("validator not found")
                .clone();
            results.push(ProtocolOutcome::NewEvidence(v_id));
        }
        let msg = HighwayMessage::NewVertex(v);
        results.push(ProtocolOutcome::CreatedGossipMessage(
            bincode::serialize(&msg).expect("should serialize message"),
        ));
        results.extend(self.detect_finality());
        results
    }

    fn detect_finality(&mut self) -> impl Iterator<Item = ProtocolOutcome<I, C>> + '_ {
        self.finality_detector
            .run(&self.highway)
            .expect("too many faulty validators")
            .map(ProtocolOutcome::FinalizedBlock)
    }

    /// Store a (pre-validated) vertex which will be added later.  This creates a timer to be sent
    /// to the reactor. The vertex be added using `Self::add_vertices` when that timer goes off.
    fn store_vertex_for_addition_later(
        &mut self,
        future_timestamp: Timestamp,
        sender: I,
        pvv: PreValidatedVertex<C>,
    ) -> Vec<ProtocolOutcome<I, C>> {
        self.vertices_to_be_added_later
            .entry(future_timestamp)
            .or_insert_with(Vec::new)
            .push((sender, pvv));
        vec![ProtocolOutcome::ScheduleTimer(future_timestamp)]
    }

    /// Call `Self::add_vertices` on any vertices in `vertices_to_be_added_later` which are
    /// scheduled for after the given `transpired_timestamp`.  In general the specified
    /// `transpired_timestamp` is approximately `Timestamp::now()`.  Vertices keyed by timestamps
    /// chronologically before `transpired_timestamp` should all be added.
    fn add_past_due_stored_vertices(
        &mut self,
        timestamp: Timestamp,
        rng: &mut NodeRng,
    ) -> Vec<ProtocolOutcome<I, C>> {
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
    ) -> Vec<ProtocolOutcome<I, C>> {
        // TODO: Is there a danger that this takes too much time, and starves other
        // components and events? Consider replacing the loop with a "callback" effect:
        // Instead of handling `HighwayMessage::NewVertex(v)` directly, return a
        // `EnqueueVertex(v)` that causes the reactor to call us with an
        // `Event::NewVertex(v)`, and call `add_vertex` when handling that event. For each
        // returned vertex that needs to be requeued, also return an `EnqueueVertex`
        // effect.
        let mut results = Vec::new();
        let mut faulty_senders = HashSet::new();
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
                    results.push(ProtocolOutcome::CreatedTargetedMessage(
                        bincode::serialize(&msg).expect("should serialize message"),
                        sender,
                    ));
                } else {
                    // If unit is sent by a doppelganger, deactivate this instance of an active
                    // validator. Continue processing the unit so that it can be added to the state.
                    if self.highway.is_doppelganger_vertex(pvv.inner()) {
                        error!(
                            "received vertex from a doppelganger. \
                            Are you running multiple nodes with the same validator key?",
                        );
                        self.deactivate_validator();
                        results.push(ProtocolOutcome::DoppelgangerDetected);
                    }
                    match self.highway.validate_vertex(pvv) {
                        Ok(vv) => {
                            let vertex = vv.inner();
                            match (vertex.value().cloned(), vertex.timestamp()) {
                                (Some(value), Some(timestamp)) if value.needs_validation() => {
                                    // Request validation before adding it to the state.
                                    self.pending_values
                                        .entry(value.clone())
                                        .or_default()
                                        .push(vv);
                                    results.push(ProtocolOutcome::ValidateConsensusValue(
                                        sender, value, timestamp,
                                    ));
                                }
                                _ => {
                                    // Either consensus value doesn't need validation or it's not a
                                    // proposal. We can add it
                                    // to the state.
                                    let now = Timestamp::now();
                                    results.extend(self.add_valid_vertex(vv, rng, now));
                                    state_changed = true;
                                }
                            }
                        }
                        Err((pvv, err)) => {
                            info!(?pvv, ?err, "invalid vertex");
                            // drop all the vertices that might have depended on this one
                            faulty_senders
                                .extend(self.drop_dependent_vertices(vec![pvv.inner().id()]));
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
        results.extend(faulty_senders.into_iter().map(ProtocolOutcome::Disconnect));
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
            // unwraps are safe, as if value is `Some`, this is already a unit
            trace!(
                now = Timestamp::now().millis(),
                timestamp = vv.inner().timestamp().unwrap().millis(),
                "adding proposal to protocol state",
            );
            self.round_success_meter.new_proposal(
                vv.inner().unit_hash().unwrap(),
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
    ) -> Vec<ProtocolOutcome<I, C>> {
        // Check whether we should change the round exponent.
        // It's important to do it before the vertex is added to the state - this way if the last
        // round has finished, we now have all the vertices from that round in the state, and no
        // newer ones.
        self.calculate_round_exponent(&vv);
        let av_effects = self.highway.add_valid_vertex(vv.clone(), rng, now);
        let mut results = self.process_av_effects(av_effects);
        let msg = HighwayMessage::NewVertex(vv.into());
        results.push(ProtocolOutcome::CreatedGossipMessage(
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

    fn drop_dependent_vertices(&mut self, mut vertices: Vec<Dependency<C>>) -> HashSet<I> {
        let mut senders = HashSet::new();
        while !vertices.is_empty() {
            let (new_vertices, new_senders) = self.do_drop_dependent_vertices(vertices);
            vertices = new_vertices;
            senders.extend(new_senders);
        }
        senders
    }

    fn do_drop_dependent_vertices(
        &mut self,
        vertices: Vec<Dependency<C>>,
    ) -> (Vec<Dependency<C>>, HashSet<I>) {
        // collect the vertices that depend on the ones we got in the argument and their senders
        let (senders, mut dropped_vertices): (HashSet<I>, Vec<Dependency<C>>) = vertices
            .into_iter()
            // filtering by is_unit, so that we don't drop vertices depending on invalid evidence
            // or endorsements - we can still get valid ones from someone else and eventually
            // satisfy the dependency
            .filter(|dep| dep.is_unit())
            .flat_map(|vertex| self.vertex_deps.remove(&vertex))
            .flatten()
            .map(|(sender, pvv)| (sender, pvv.inner().id()))
            .unzip();

        dropped_vertices.extend(self.drop_by_senders(senders.clone()));
        (dropped_vertices, senders)
    }

    fn drop_by_senders(&mut self, senders: HashSet<I>) -> Vec<Dependency<C>> {
        let mut dropped_vertices = Vec::new();
        let mut keys_to_drop = Vec::new();
        for (key, dependent_vertices) in self.vertex_deps.iter_mut() {
            dependent_vertices.retain(|(sender, pvv)| {
                if senders.contains(sender) {
                    // remember the deps we drop
                    dropped_vertices.push(pvv.inner().id());
                    false
                } else {
                    true
                }
            });
            if dependent_vertices.is_empty() {
                keys_to_drop.push(key.clone());
            }
        }
        for key in keys_to_drop {
            self.vertex_deps.remove(&key);
        }
        dropped_vertices
    }

    /// Returns an iterator over all the values that are expected to become finalized, but are not
    /// finalized yet.
    pub(crate) fn non_finalized_values(
        &self,
        mut fork_choice: Option<C::Hash>,
    ) -> impl Iterator<Item = &C::ConsensusValue> {
        let last_finalized = self.finality_detector.last_finalized();
        iter::from_fn(move || {
            if fork_choice.as_ref() == last_finalized {
                return None;
            }
            let maybe_block = fork_choice.map(|bhash| self.highway.state().block(&bhash));
            let value = maybe_block.map(|block| &block.value);
            fork_choice = maybe_block.and_then(|block| block.parent().cloned());
            value
        })
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
    ) -> Vec<ProtocolOutcome<I, C>> {
        match bincode::deserialize(msg.as_slice()) {
            Err(err) => vec![ProtocolOutcome::InvalidIncomingMessage(
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
                // Keep track of whether the prevalidated vertex was from an equivocator
                let v_id = v.id();
                let pvv = match self.highway.pre_validate_vertex(v) {
                    Ok(pvv) => pvv,
                    Err((_, err)) => {
                        // drop the vertices that might have depended on this one
                        let faulty_senders = self.drop_dependent_vertices(vec![v_id]);
                        return iter::once(ProtocolOutcome::InvalidIncomingMessage(
                            msg,
                            sender,
                            err.into(),
                        ))
                        .chain(faulty_senders.into_iter().map(ProtocolOutcome::Disconnect))
                        .collect();
                    }
                };
                let is_faulty = match pvv.inner().signed_wire_unit() {
                    Some(signed_wire_unit) => self
                        .highway
                        .state()
                        .is_faulty(signed_wire_unit.wire_unit.creator),
                    None => false,
                };

                match pvv.timestamp() {
                    Some(timestamp) if timestamp > Timestamp::now() => {
                        // If it's not from an equivocator and from the future, add to queue
                        if !is_faulty {
                            self.store_vertex_for_addition_later(timestamp, sender, pvv)
                        } else {
                            vec![]
                        }
                    }
                    _ => {
                        // If it's not from an equivocator or it is a transitive dependency, add the
                        // vertex
                        if !is_faulty || self.vertex_deps.contains_key(&pvv.inner().id()) {
                            self.add_vertices(vec![(sender, pvv)], rng)
                        } else {
                            vec![]
                        }
                    }
                }
            }
            Ok(HighwayMessage::RequestDependency(dep)) => match self.highway.get_dependency(&dep) {
                GetDepOutcome::None => {
                    info!(?dep, ?sender, "requested dependency doesn't exist");
                    vec![]
                }
                GetDepOutcome::Evidence(vid) => vec![ProtocolOutcome::SendEvidence(sender, vid)],
                GetDepOutcome::Vertex(vv) => {
                    let msg = HighwayMessage::NewVertex(vv.into());
                    let serialized_msg =
                        bincode::serialize(&msg).expect("should serialize message");
                    // TODO: Should this be done via a gossip service?
                    vec![ProtocolOutcome::CreatedTargetedMessage(
                        serialized_msg,
                        sender,
                    )]
                }
            },
        }
    }

    fn handle_timer(
        &mut self,
        timestamp: Timestamp,
        rng: &mut NodeRng,
    ) -> Vec<ProtocolOutcome<I, C>> {
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
    ) -> Vec<ProtocolOutcome<I, C>> {
        let effects = self.highway.propose(value, block_context, rng);
        self.process_av_effects(effects)
    }

    fn resolve_validity(
        &mut self,
        value: &C::ConsensusValue,
        valid: bool,
        rng: &mut NodeRng,
    ) -> Vec<ProtocolOutcome<I, C>> {
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
            // Drop vertices dependent on the invalid value.
            let dropped_vertices = self.pending_values.remove(value);
            // recursively remove vertices depending on the dropped ones
            warn!(
                ?value,
                ?dropped_vertices,
                "consensus value is invalid; dropping dependent vertices"
            );
            let faulty_senders = self.drop_dependent_vertices(
                dropped_vertices
                    .into_iter()
                    .flatten()
                    .map(|vv| vv.inner().id())
                    .collect(),
            );
            faulty_senders
                .into_iter()
                .map(ProtocolOutcome::Disconnect)
                .collect()
        }
    }

    fn activate_validator(
        &mut self,
        our_id: C::ValidatorId,
        secret: C::ValidatorSecret,
        timestamp: Timestamp,
        unit_hash_file: Option<PathBuf>,
    ) -> Vec<ProtocolOutcome<I, C>> {
        let av_effects = self
            .highway
            .activate_validator(our_id, secret, timestamp, unit_hash_file);
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

    fn request_evidence(&self, sender: I, vid: &C::ValidatorId) -> Vec<ProtocolOutcome<I, C>> {
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
                        Some(ProtocolOutcome::CreatedTargetedMessage(
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
        !self.highway.state().is_empty()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn is_active(&self) -> bool {
        self.highway.is_active()
    }

    fn instance_id(&self) -> &C::InstanceId {
        self.highway.instance_id()
    }
}
