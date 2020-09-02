use std::{fmt::Debug, iter, rc::Rc};

use anyhow::Error;
use rand::{CryptoRng, Rng};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{
    components::consensus::{
        consensus_protocol::{
            synchronizer::{DagSynchronizerState, SynchronizerEffect},
            BlockContext, ConsensusProtocol, ConsensusProtocolResult, ProtocolState, VertexTrait,
        },
        highway_core::{
            active_validator::Effect as AvEffect,
            finality_detector::{FinalityDetector, FinalityOutcome},
            highway::{Dependency, Highway, Params, PreValidatedVertex, Vertex},
            validators::Validators,
            Weight,
        },
        traits::{Context, NodeIdT, ValidatorSecret},
    },
    crypto::{
        asymmetric_key::{self, PublicKey, SecretKey, Signature},
        hash::{self, Digest},
    },
    types::{ProtoBlock, Timestamp},
};

impl<C: Context> VertexTrait for PreValidatedVertex<C> {
    type Id = Dependency<C>;
    type Value = C::ConsensusValue;

    fn id(&self) -> Dependency<C> {
        self.vertex().id()
    }

    fn value(&self) -> Option<&C::ConsensusValue> {
        self.vertex().value()
    }
}

impl<C: Context> ProtocolState for Highway<C> {
    type Error = String;
    type VId = Dependency<C>;
    type Value = C::ConsensusValue;
    type Vertex = PreValidatedVertex<C>;

    fn missing_dependency(&self, pvv: &Self::Vertex) -> Option<Dependency<C>> {
        self.missing_dependency(pvv)
    }

    fn get_vertex(&self, v: Dependency<C>) -> Result<Option<PreValidatedVertex<C>>, Self::Error> {
        Ok(self.get_dependency(&v).map(PreValidatedVertex::from))
    }
}

#[derive(Debug)]
pub(crate) struct HighwayProtocol<I, C: Context> {
    synchronizer: DagSynchronizerState<I, Highway<C>>,
    finality_detector: FinalityDetector<C>,
    highway: Highway<C>,
}

impl<I: NodeIdT, C: Context> HighwayProtocol<I, C> {
    pub(crate) fn new(
        instance_id: C::InstanceId,
        validators: Validators<C::ValidatorId>,
        params: Params,
        our_id: C::ValidatorId,
        secret: C::ValidatorSecret,
        ftt: Weight,
        timestamp: Timestamp,
    ) -> (Self, Vec<CpResult<I, C>>) {
        // TODO: We use the minimum as round exponent here, since it is meant to be optimal.
        // For adaptive round lengths we will probably want to use the most recent one from the
        // previous era instead.
        let round_exp = params.min_round_exp();
        let mut highway = Highway::new(instance_id, validators, params);
        let av_effects = highway.activate_validator(our_id, secret, round_exp, timestamp);
        let mut instance = HighwayProtocol {
            synchronizer: DagSynchronizerState::new(),
            finality_detector: FinalityDetector::new(ftt),
            highway,
        };
        let effects = instance.process_av_effects(av_effects);
        (instance, effects)
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
        self.detect_finality()
            .into_iter()
            .chain(iter::once(ConsensusProtocolResult::CreatedGossipMessage(
                serialized_msg,
            )))
            .collect()
    }

    fn detect_finality(&mut self) -> Option<CpResult<I, C>> {
        match self.finality_detector.run(&self.highway) {
            FinalityOutcome::None => None,
            FinalityOutcome::FttExceeded => panic!("Too many faulty validators"),
            FinalityOutcome::Finalized {
                value,
                new_equivocators,
                rewards,
                timestamp,
                height,
                terminal,
                proposer,
            } => Some(ConsensusProtocolResult::FinalizedBlock {
                value,
                new_equivocators,
                rewards,
                timestamp,
                height,
                switch_block: terminal,
                proposer,
            }),
        }
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

struct SynchronizerQueue<'a, I, C: Context> {
    vertex_queue: Vec<(I, PreValidatedVertex<C>)>,
    synchronizer_effects_queue: Vec<SynchronizerEffect<I, PreValidatedVertex<C>>>,
    results: Vec<CpResult<I, C>>,
    hw_proto: &'a mut HighwayProtocol<I, C>,
}

impl<'a, I, C: Context> SynchronizerQueue<'a, I, C>
where
    I: NodeIdT,
{
    fn new(hw_proto: &'a mut HighwayProtocol<I, C>) -> Self {
        Self {
            vertex_queue: vec![],
            synchronizer_effects_queue: vec![],
            results: vec![],
            hw_proto,
        }
    }

    fn with_vertices(mut self, vertices: Vec<(I, PreValidatedVertex<C>)>) -> Self {
        self.vertex_queue = vertices;
        self
    }

    fn with_synchronizer_effects(
        mut self,
        effects: Vec<SynchronizerEffect<I, PreValidatedVertex<C>>>,
    ) -> Self {
        self.synchronizer_effects_queue = effects;
        self
    }

    fn run<R: Rng + CryptoRng + ?Sized>(mut self, rng: &mut R) -> Vec<CpResult<I, C>> {
        loop {
            if let Some(effect) = self.synchronizer_effects_queue.pop() {
                self.process_synchronizer_effect(effect, rng);
            } else if let Some((sender, vertex)) = self.vertex_queue.pop() {
                self.process_vertex(sender, vertex);
            } else {
                return self.results;
            }
        }
    }

    fn process_vertex(&mut self, sender: I, vertex: PreValidatedVertex<C>) {
        let effects =
            self.hw_proto
                .synchronizer
                .synchronize_vertex(sender, vertex, &self.hw_proto.highway);
        self.synchronizer_effects_queue.extend(effects);
    }

    fn process_synchronizer_effect<R: Rng + CryptoRng + ?Sized>(
        &mut self,
        effect: SynchronizerEffect<I, PreValidatedVertex<C>>,
        rng: &mut R,
    ) {
        match effect {
            SynchronizerEffect::RequestVertex(sender, missing_vid) => {
                let msg = HighwayMessage::RequestDependency(missing_vid);
                let serialized_msg = rmp_serde::to_vec(&msg).expect("should serialize message");
                self.results
                    .push(ConsensusProtocolResult::CreatedTargetedMessage(
                        serialized_msg,
                        sender,
                    ));
            }
            SynchronizerEffect::Ready(pvv) => {
                let vv = match self.hw_proto.highway.validate_vertex(pvv) {
                    Ok(vv) => vv,
                    Err((pvv, err)) => {
                        info!(?pvv, ?err, "invalid vertex");
                        let _senders = self
                            .hw_proto
                            .synchronizer
                            .on_vertex_invalid(Vertex::from(pvv).id());
                        // TODO: Disconnect from senders!
                        return;
                    }
                };
                // TODO: Avoid cloning. (Serialize first?)
                let av_effects = self.hw_proto.highway.add_valid_vertex(vv.clone(), rng);
                self.results
                    .extend(self.hw_proto.process_av_effects(av_effects));
                let msg = HighwayMessage::NewVertex(vv.into());
                let serialized_msg = rmp_serde::to_vec(&msg).expect("should serialize message");
                self.results.extend(self.hw_proto.detect_finality());
                self.results
                    .push(ConsensusProtocolResult::CreatedGossipMessage(
                        serialized_msg,
                    ))
            }
            SynchronizerEffect::RequeueVertex(sender, vertex) => {
                self.vertex_queue.push((sender, vertex));
            }
            SynchronizerEffect::RequestConsensusValue(sender, value) => {
                self.results
                    .push(ConsensusProtocolResult::ValidateConsensusValue(
                        sender, value,
                    ));
            }
        }
    }
}

impl<I, C: Context, R: Rng + CryptoRng + ?Sized>
    ConsensusProtocol<I, C::ConsensusValue, C::ValidatorId, R> for HighwayProtocol<I, C>
where
    I: NodeIdT,
{
    fn handle_message(
        &mut self,
        sender: I,
        msg: Vec<u8>,
        rng: &mut R,
    ) -> Result<Vec<CpResult<I, C>>, Error> {
        match rmp_serde::from_read_ref(msg.as_slice()) {
            Err(err) => Ok(vec![ConsensusProtocolResult::InvalidIncomingMessage(
                msg,
                sender,
                err.into(),
            )]),
            Ok(HighwayMessage::NewVertex(ref v)) if self.highway.has_vertex(v) => Ok(vec![]),
            Ok(HighwayMessage::NewVertex(v)) => {
                let pvv = match self.highway.pre_validate_vertex(v) {
                    Ok(pvv) => pvv,
                    Err((vertex, err)) => {
                        let _senders = self.synchronizer.on_vertex_invalid(vertex.id());
                        // TODO: Disconnect from senders.
                        return Ok(vec![ConsensusProtocolResult::InvalidIncomingMessage(
                            msg,
                            sender,
                            err.into(),
                        )]);
                    }
                };
                // TODO: Is there a danger that this takes too much time, and starves other
                // components and events? Consider replacing the loop with a "callback" effect:
                // Instead of handling `HighwayMessage::NewVertex(v)` directly, return a
                // `EnqueueVertex(v)` that causes the reactor to call us with an
                // `Event::NewVertex(v)`, and call `add_vertex` when handling that event. For each
                // returned vertex that needs to be requeued, also return an `EnqueueVertex`
                // effect.
                Ok(SynchronizerQueue::new(self)
                    .with_vertices(vec![(sender, pvv)])
                    .run(rng))
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
        rng: &mut R,
    ) -> Result<Vec<CpResult<I, C>>, Error> {
        let effects = self.highway.handle_timer(timestamp, rng);
        Ok(self.process_av_effects(effects))
    }

    fn propose(
        &mut self,
        value: C::ConsensusValue,
        block_context: BlockContext,
        rng: &mut R,
    ) -> Result<Vec<CpResult<I, C>>, Error> {
        let effects = self.highway.propose(value, block_context, rng);
        Ok(self.process_av_effects(effects))
    }

    /// Marks `value` as valid.
    /// Calls the synchronizer that `value` dependency has been satisfied.
    fn resolve_validity(
        &mut self,
        value: &C::ConsensusValue,
        valid: bool,
        rng: &mut R,
    ) -> Result<Vec<CpResult<I, C>>, Error> {
        if valid {
            let effects = self.synchronizer.on_consensus_value_synced(value);
            Ok(SynchronizerQueue::new(self)
                .with_synchronizer_effects(effects)
                .run(rng))
        } else {
            // TODO: Slash proposer?
            // Drop dependent vertices.
            let _senders = self.synchronizer.on_consensus_value_invalid(value);
            // TODO: Disconnect from senders.
            Ok(vec![])
        }
    }

    /// Turns this instance into a passive observer, that does not create any new vertices.
    fn deactivate_validator(&mut self) {
        self.highway.deactivate_validator()
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

    fn sign<R: Rng + CryptoRng + ?Sized>(&self, hash: &Digest, rng: &mut R) -> Signature {
        asymmetric_key::sign(hash, self.secret_key.as_ref(), &self.public_key, rng)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct HighwayContext;

impl Context for HighwayContext {
    type ConsensusValue = ProtoBlock;
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
