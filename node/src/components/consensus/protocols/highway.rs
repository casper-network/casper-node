use std::{
    any::Any,
    collections::{BTreeMap, HashMap},
    fmt::Debug,
    iter,
    rc::Rc,
};

use anyhow::Error;
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
        let msg = HighwayMessage::NewVertex(v);
        let serialized_msg = rmp_serde::to_vec(&msg).expect("should serialize message");
        let result = ConsensusProtocolResult::CreatedGossipMessage(serialized_msg);
        self.detect_finality().chain(iter::once(result)).collect()
    }

    fn detect_finality(&mut self) -> impl Iterator<Item = CpResult<I, C>> + '_ {
        self.finality_detector
            .run(&self.highway)
            .expect("too many faulty validators")
            .map(ConsensusProtocolResult::FinalizedBlock)
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
                        rmp_serde::to_vec(&msg).expect("should serialize message"),
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
                                results.extend(self.add_valid_vertex(vv, rng));
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
    ) -> Vec<CpResult<I, C>> {
        let start_time = Timestamp::now();
        let av_effects = self.highway.add_valid_vertex(vv.clone(), rng);
        let elapsed = start_time.elapsed();
        trace!(%elapsed, "added valid vertex");
        let mut results = self.process_av_effects(av_effects);
        let msg = HighwayMessage::NewVertex(vv.into());
        results.push(ConsensusProtocolResult::CreatedGossipMessage(
            rmp_serde::to_vec(&msg).expect("should serialize message"),
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
        rng: &mut dyn CryptoRngCore,
    ) -> Result<Vec<CpResult<I, C>>, Error> {
        match rmp_serde::from_read_ref(msg.as_slice()) {
            Err(err) => Ok(vec![ConsensusProtocolResult::InvalidIncomingMessage(
                msg,
                sender,
                err.into(),
            )]),
            Ok(HighwayMessage::NewVertex(ref v)) if self.highway.has_vertex(v) => Ok(vec![]),
            Ok(HighwayMessage::NewVertex(v)) => {
                match self.highway.pre_validate_vertex(v) {
                    Ok(pvv) => Ok(self.add_vertices(vec![(sender, pvv)], rng)),
                    Err((_, err)) => {
                        // TODO: Disconnect from senders.
                        Ok(vec![ConsensusProtocolResult::InvalidIncomingMessage(
                            msg,
                            sender,
                            err.into(),
                        )])
                    }
                }
            }
            Ok(HighwayMessage::RequestDependency(dep)) => {
                if let Some(vv) = self.highway.get_dependency(&dep) {
                    let msg = HighwayMessage::NewVertex(vv.into());
                    let serialized_msg = rmp_serde::to_vec(&msg).expect("should serialize message");
                    // TODO: Should this be done via a gossip service?
                    Ok(vec![ConsensusProtocolResult::CreatedTargetedMessage(
                        serialized_msg,
                        sender,
                    )])
                } else {
                    info!(?dep, ?sender, "requested dependency doesn't exist");
                    Ok(vec![])
                }
            }
        }
    }

    fn handle_timer(
        &mut self,
        timestamp: Timestamp,
        rng: &mut dyn CryptoRngCore,
    ) -> Result<Vec<CpResult<I, C>>, Error> {
        let effects = self.highway.handle_timer(timestamp, rng);
        Ok(self.process_av_effects(effects))
    }

    fn propose(
        &mut self,
        value: C::ConsensusValue,
        block_context: BlockContext,
        rng: &mut dyn CryptoRngCore,
    ) -> Result<Vec<CpResult<I, C>>, Error> {
        let effects = self.highway.propose(value, block_context, rng);
        Ok(self.process_av_effects(effects))
    }

    fn resolve_validity(
        &mut self,
        value: &C::ConsensusValue,
        valid: bool,
        rng: &mut dyn CryptoRngCore,
    ) -> Result<Vec<CpResult<I, C>>, Error> {
        if valid {
            let mut results = self
                .pending_values
                .remove(value)
                .into_iter()
                .flatten()
                .flat_map(|vv| self.add_valid_vertex(vv, rng))
                .collect_vec();
            let satisfied_pvvs = self.remove_satisfied_deps().collect();
            results.extend(self.add_vertices(satisfied_pvvs, rng));
            results.extend(self.detect_finality());
            Ok(results)
        } else {
            // TODO: Slash proposer?
            // TODO: Drop dependent vertices? Or add timeout.
            // TODO: Disconnect from senders.
            Ok(vec![])
        }
    }

    fn deactivate_validator(&mut self) {
        self.highway.deactivate_validator()
    }

    fn has_evidence(&self, vid: &C::ValidatorId) -> bool {
        self.highway.has_evidence(vid)
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
