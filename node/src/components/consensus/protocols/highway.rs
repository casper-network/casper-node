use std::fmt::Debug;

use anyhow::Error;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::components::consensus::{
    consensus_protocol::{
        synchronizer::{DagSynchronizerState, SynchronizerEffect},
        AddVertexOk, BlockContext, ConsensusProtocol, ConsensusProtocolResult, ProtocolState,
        Timestamp, VertexTrait,
    },
    highway_core::{
        active_validator::Effect as AvEffect,
        finality_detector::FinalityDetector,
        highway::{Highway, PreValidatedVertex},
        vertex::{Dependency, Vertex},
    },
    traits::{Context, NodeIdT},
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
    type Vertex = PreValidatedVertex<C>;

    fn add_vertex(&mut self, pvv: Self::Vertex) -> Result<AddVertexOk<Dependency<C>>, Self::Error> {
        let vid = pvv.vertex().id();
        if let Some(dep) = self.missing_dependency(&pvv) {
            return Ok(AddVertexOk::MissingDependency(dep));
        }
        let vv = match self.validate_vertex(pvv) {
            Ok(vv) => vv,
            Err((pvv, err)) => return Err(format!("invalid vertex: {:?}, {:?}", pvv, err)),
        };
        if !self.add_valid_vertex(vv).is_empty() {
            return Err("add_vertex returned non-empty vec of effects. \
                        This mustn't happen. You forgot to update the code!"
                .to_string());
        }
        Ok(AddVertexOk::Success(vid))
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

#[derive(Serialize, Deserialize)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
enum HighwayMessage<C: Context> {
    NewVertex(Vertex<C>),
    RequestDependency(Dependency<C>),
}

struct SynchronizerQueue<I, C: Context> {
    vertex_queue: Vec<(I, PreValidatedVertex<C>)>,
    synchronizer_effects_queue: Vec<SynchronizerEffect<I, PreValidatedVertex<C>>>,
    results: Vec<ConsensusProtocolResult<I, C::ConsensusValue>>,
}

impl<I, C: Context> SynchronizerQueue<I, C>
where
    I: NodeIdT,
{
    fn new() -> Self {
        Self {
            vertex_queue: vec![],
            synchronizer_effects_queue: vec![],
            results: vec![],
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

    fn is_finished(&self) -> bool {
        self.vertex_queue.is_empty() && self.synchronizer_effects_queue.is_empty()
    }

    fn into_results(self) -> Vec<ConsensusProtocolResult<I, C::ConsensusValue>> {
        self.results
    }

    fn process_vertex(
        &mut self,
        sender: I,
        vertex: PreValidatedVertex<C>,
        synchronizer: &mut DagSynchronizerState<I, Highway<C>>,
        highway: &mut Highway<C>,
    ) {
        match synchronizer.add_vertex(sender, vertex, highway) {
            Ok(effects) => self.synchronizer_effects_queue.extend(effects),
            Err(err) => todo!("error: {:?}", err),
        }
    }

    fn process_synchronizer_effect(
        &mut self,
        effect: SynchronizerEffect<I, PreValidatedVertex<C>>,
    ) {
        match effect {
            SynchronizerEffect::RequestVertex(sender, missing_vid) => {
                let msg = HighwayMessage::RequestDependency(missing_vid);
                let serialized_msg = match serde_json::to_vec_pretty(&msg) {
                    Ok(msg) => msg,
                    Err(err) => todo!("error: {:?}", err),
                };
                self.results
                    .push(ConsensusProtocolResult::CreatedTargetedMessage(
                        serialized_msg,
                        sender,
                    ));
            }
            SynchronizerEffect::Success(vertex) => {
                // TODO: Add new vertex to state. (Indirectly, via synchronizer?)
                let msg = HighwayMessage::NewVertex(vertex.into());
                // TODO: Don't `unwrap`.
                let serialized_msg = serde_json::to_vec_pretty(&msg).unwrap();
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
            SynchronizerEffect::InvalidVertex(v, sender, err) => {
                warn!("Invalid vertex from {:?}: {:?}, {:?}", v, sender, err);
            }
        }
    }

    fn process_item(
        &mut self,
        synchronizer: &mut DagSynchronizerState<I, Highway<C>>,
        highway: &mut Highway<C>,
    ) {
        if let Some((sender, vertex)) = self.vertex_queue.pop() {
            self.process_vertex(sender, vertex, synchronizer, highway);
        } else if let Some(effect) = self.synchronizer_effects_queue.pop() {
            self.process_synchronizer_effect(effect);
        }
    }
}

impl<I, C: Context> ConsensusProtocol<I, C::ConsensusValue> for HighwayProtocol<I, C>
where
    I: NodeIdT,
{
    fn handle_message(
        &mut self,
        sender: I,
        msg: Vec<u8>,
    ) -> Result<Vec<ConsensusProtocolResult<I, C::ConsensusValue>>, Error> {
        let highway_message: HighwayMessage<C> = serde_json::from_slice(msg.as_slice()).unwrap();
        Ok(match highway_message {
            HighwayMessage::NewVertex(v) => {
                let pvv = match self.highway.pre_validate_vertex(v) {
                    Ok(pvv) => pvv,
                    Err((_vertex, err)) => {
                        return Ok(vec![ConsensusProtocolResult::InvalidIncomingMessage(
                            msg,
                            sender,
                            err.into(),
                        )]);
                    }
                };
                let mut queue = SynchronizerQueue::new().with_vertices(vec![(sender, pvv)]);
                // TODO: Is there a danger that this takes too much time, and starves other
                // components and events? Consider replacing the loop with a "callback" effect:
                // Instead of handling `HighwayMessage::NewVertex(v)` directly, return a
                // `EnqueueVertex(v)` that causes the reactor to call us with an
                // `Event::NewVertex(v)`, and call `add_vertex` when handling that event. For each
                // returned vertex that needs to be requeued, also return an `EnqueueVertex`
                // effect.
                while !queue.is_finished() {
                    queue.process_item(&mut self.synchronizer, &mut self.highway);
                }
                queue.into_results()
            }
            HighwayMessage::RequestDependency(dep) => {
                if let Some(vv) = self.highway.get_dependency(&dep) {
                    let msg = HighwayMessage::NewVertex(vv.into());
                    let serialized_msg = serde_json::to_vec_pretty(&msg).unwrap();
                    // TODO: Should this be done via a gossip service?
                    vec![ConsensusProtocolResult::CreatedTargetedMessage(
                        serialized_msg,
                        sender,
                    )]
                } else {
                    info!(?dep, "Requested dependency doesn't exist.");
                    vec![]
                }
            }
        })
    }

    fn handle_timer(
        &mut self,
        timestamp: Timestamp,
    ) -> Result<Vec<ConsensusProtocolResult<I, <C as Context>::ConsensusValue>>, Error> {
        Ok(self
            .highway
            .handle_timer(timestamp.0)
            .into_iter()
            .map(|effect| match effect {
                AvEffect::NewVertex(v) => {
                    // TODO: Add new vertex to state. (Indirectly, via synchronizer?)
                    let msg = HighwayMessage::NewVertex(v);
                    // TODO: Don't `unwrap`.
                    // TODO: Replace serde with generic serializer
                    let serialized_msg = serde_json::to_vec_pretty(&msg).unwrap();
                    ConsensusProtocolResult::CreatedGossipMessage(serialized_msg)
                }
                AvEffect::ScheduleTimer(instant_u64) => {
                    ConsensusProtocolResult::ScheduleTimer(Timestamp(instant_u64))
                }
                AvEffect::RequestNewBlock(ctx) => ConsensusProtocolResult::CreateNewBlock(ctx),
            })
            .collect())
    }

    fn propose(
        &self,
        value: C::ConsensusValue,
        block_context: BlockContext,
    ) -> Result<Vec<ConsensusProtocolResult<I, <C as Context>::ConsensusValue>>, Error> {
        // TODO: Deduplicate
        Ok(self
            .highway
            .propose(value, block_context)
            .into_iter()
            .map(|effect| match effect {
                AvEffect::NewVertex(v) => {
                    // TODO: Add new vertex to state? (Indirectly, via synchronizer?)
                    let msg = HighwayMessage::NewVertex(v);
                    //TODO: Don't unwrap
                    // Replace serde with generic serializer.
                    let serialized_msg = serde_json::to_vec_pretty(&msg).unwrap();
                    ConsensusProtocolResult::CreatedGossipMessage(serialized_msg)
                }
                AvEffect::ScheduleTimer(instant_u64) => {
                    ConsensusProtocolResult::ScheduleTimer(Timestamp(instant_u64))
                }
                AvEffect::RequestNewBlock(block_context) => {
                    ConsensusProtocolResult::CreateNewBlock(block_context)
                }
            })
            .collect())
    }

    fn resolve_validity(
        &mut self,
        value: &C::ConsensusValue,
        valid: bool,
    ) -> Result<Vec<ConsensusProtocolResult<I, C::ConsensusValue>>, Error> {
        if valid {
            let effects = self.synchronizer.on_consensus_value_synced(value);
            let mut queue = SynchronizerQueue::new().with_synchronizer_effects(effects);
            while !queue.is_finished() {
                queue.process_item(&mut self.synchronizer, &mut self.highway);
            }
            Ok(queue.into_results())
        } else {
            todo!()
        }
    }
}
