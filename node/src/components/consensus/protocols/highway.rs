use std::{
    any::Any,
    collections::{BTreeMap, HashMap},
    fmt::Debug,
    rc::Rc,
};

use datasize::DataSize;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use tracing::{info, trace};

use crate::{
    components::consensus::{
        candidate_block::CandidateBlock,
        consensus_protocol::{BlockContext, ConsensusProtocol, ConsensusProtocolResult},
        highway_core::{
            active_validator::Effect as AvEffect,
            finality_detector::FinalityDetector,
            highway::{Dependency, Highway, Params, PreValidatedVertex, ValidVertex, Vertex},
            validators::Validators,
            Weight,
        },
        traits::{Context, NodeIdT, ValidatorSecret},
    },
    crypto::{
        asymmetric_key::{self, PublicKey, SecretKey, Signature},
        hash::{self, Digest},
    },
    types::{CryptoRngCore, Timestamp},
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
}

impl<I: NodeIdT, C: Context> HighwayProtocol<I, C> {
    pub(crate) fn new(
        instance_id: C::InstanceId,
        validators: Validators<C::ValidatorId>,
        params: Params,
        ftt: Weight,
    ) -> Self {
        HighwayProtocol {
            vertex_deps: BTreeMap::new(),
            pending_values: HashMap::new(),
            finality_detector: FinalityDetector::new(ftt),
            highway: Highway::new(instance_id, validators, params),
            vertices_to_be_added_later: BTreeMap::new(),
        }
    }

    pub(crate) fn activate_validator(
        &mut self,
        our_id: C::ValidatorId,
        secret: C::ValidatorSecret,
        timestamp: Timestamp,
    ) -> Vec<CpResult<I, C>> {
        let av_effects = self.highway.activate_validator(our_id, secret, timestamp);
        self.process_av_effects(av_effects)
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
            AvEffect::NewVertex(vv) => self.process_new_vertex(vv.into()),
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
        rng: &mut dyn CryptoRngCore,
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
        rng: &mut dyn CryptoRngCore,
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
                            if let Some(value) = vv.inner().value().cloned() {
                                // It's a block: Request validation before adding it to the state.
                                self.pending_values
                                    .entry(value.clone())
                                    .or_default()
                                    .push(vv);
                                results.push(ConsensusProtocolResult::ValidateConsensusValue(
                                    sender, value,
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

    fn add_valid_vertex(
        &mut self,
        vv: ValidVertex<C>,
        rng: &mut dyn CryptoRngCore,
        now: Timestamp,
    ) -> Vec<CpResult<I, C>> {
        let start_time = Timestamp::now();
        let av_effects = self.highway.add_valid_vertex(vv.clone(), rng, now);
        let elapsed = start_time.elapsed();
        trace!(%elapsed, "added valid vertex");
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

type CpResult<I, C> =
    ConsensusProtocolResult<I, <C as Context>::ConsensusValue, <C as Context>::ValidatorId>;

impl<I, C> ConsensusProtocol<I, C::ConsensusValue, C::ValidatorId> for HighwayProtocol<I, C>
where
    I: NodeIdT,
    C: Context + 'static,
{
    fn handle_message(
        &mut self,
        sender: I,
        msg: Vec<u8>,
        evidence_only: bool,
        rng: &mut dyn CryptoRngCore,
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
            Ok(HighwayMessage::RequestDependency(dep)) => {
                if let Some(vv) = self.highway.get_dependency(&dep) {
                    let msg = HighwayMessage::NewVertex(vv.into());
                    let serialized_msg =
                        bincode::serialize(&msg).expect("should serialize message");
                    // TODO: Should this be done via a gossip service?
                    vec![ConsensusProtocolResult::CreatedTargetedMessage(
                        serialized_msg,
                        sender,
                    )]
                } else {
                    info!(?dep, ?sender, "requested dependency doesn't exist");
                    vec![]
                }
            }
        }
    }

    fn handle_timer(
        &mut self,
        timestamp: Timestamp,
        rng: &mut dyn CryptoRngCore,
    ) -> Vec<CpResult<I, C>> {
        let effects = self.highway.handle_timer(timestamp, rng);
        let mut results = self.process_av_effects(effects);
        results.extend(self.add_past_due_stored_vertices(timestamp, rng));
        results
    }

    fn propose(
        &mut self,
        value: C::ConsensusValue,
        block_context: BlockContext,
        rng: &mut dyn CryptoRngCore,
    ) -> Vec<CpResult<I, C>> {
        let effects = self.highway.propose(value, block_context, rng);
        self.process_av_effects(effects)
    }

    fn resolve_validity(
        &mut self,
        value: &C::ConsensusValue,
        valid: bool,
        rng: &mut dyn CryptoRngCore,
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

    fn deactivate_validator(&mut self) {
        self.highway.deactivate_validator()
    }

    fn has_evidence(&self, vid: &C::ValidatorId) -> bool {
        self.highway.has_evidence(vid)
    }

    fn request_evidence(&self, sender: I, vid: &C::ValidatorId) -> Vec<CpResult<I, C>> {
        self.highway
            .validators()
            .get_index(vid)
            .and_then(|vidx| self.highway.get_dependency(&Dependency::Evidence(vidx)))
            .map(|vv| {
                let msg = HighwayMessage::NewVertex(vv.into());
                let serialized_msg = bincode::serialize(&msg).expect("should serialize message");
                ConsensusProtocolResult::CreatedTargetedMessage(serialized_msg, sender)
            })
            .into_iter()
            .collect()
    }

    fn faulty_validators(&self) -> Vec<&C::ValidatorId> {
        self.highway.faulty_validators().collect()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub(crate) struct HighwaySecret {
    secret_key: Rc<SecretKey>,
    public_key: PublicKey,
}

impl HighwaySecret {
    pub(crate) fn new(secret_key: Rc<SecretKey>, public_key: PublicKey) -> Self {
        Self {
            secret_key,
            public_key,
        }
    }
}

impl ValidatorSecret for HighwaySecret {
    type Hash = Digest;
    type Signature = Signature;

    fn sign(&self, hash: &Digest, rng: &mut dyn CryptoRngCore) -> Signature {
        asymmetric_key::sign(hash, self.secret_key.as_ref(), &self.public_key, rng)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct HighwayContext;

impl Context for HighwayContext {
    type ConsensusValue = CandidateBlock;
    type ValidatorId = PublicKey;
    type ValidatorSecret = HighwaySecret;
    type Signature = Signature;
    type Hash = Digest;
    type InstanceId = Digest;

    fn hash(data: &[u8]) -> Digest {
        hash::hash(data)
    }

    fn verify_signature(hash: &Digest, public_key: &PublicKey, signature: &Signature) -> bool {
        if let Err(error) = asymmetric_key::verify(hash, signature, public_key) {
            info!(%error, %signature, %public_key, %hash, "failed to validate signature");
            return false;
        }
        true
    }
}
