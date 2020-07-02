use anyhow::Error;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::components::{
    consensus::{
        consensus_protocol::{
            synchronizer::{DagSynchronizerState, SynchronizerEffect},
            AddVertexOk, BlockContext, ConsensusProtocol, ConsensusProtocolResult, ProtocolState,
            Timestamp, VertexTrait,
        },
        highway_core::{
            active_validator::Effect as AvEffect,
            finality_detector::FinalityDetector,
            highway::{AddVertexOutcome, Highway},
            vertex::{Dependency, Vertex},
        },
        traits::Context,
    },
    small_network::NodeId,
};

impl<C: Context> VertexTrait for Vertex<C> {
    type Id = Dependency<C>;
    type Value = C::ConsensusValue;

    fn id(&self) -> Dependency<C> {
        self.id()
    }

    fn value(&self) -> Option<&C::ConsensusValue> {
        self.value()
    }
}

impl<C: Context> ProtocolState for Highway<C> {
    type Error = String;
    type VId = Dependency<C>;
    type Vertex = Vertex<C>;

    fn add_vertex(&mut self, v: Vertex<C>) -> Result<AddVertexOk<Dependency<C>>, Self::Error> {
        let vid = v.id();
        match self.add_vertex(v) {
            AddVertexOutcome::Success(vec) => {
                if !vec.is_empty() {
                    Err("add_vertex returned non-empty vec of effects. \
                    This mustn't happen. You forgot to update the code!"
                        .to_string())
                } else {
                    Ok(AddVertexOk::Success(vid))
                }
            }
            AddVertexOutcome::MissingDependency(_vertex, dependency) => {
                Ok(AddVertexOk::MissingDependency(dependency))
            }
            AddVertexOutcome::Invalid(vertex) => Err(format!("invalid vertex: {:?}", vertex)),
        }
    }

    fn get_vertex(&self, v: Dependency<C>) -> Result<Option<Vertex<C>>, Self::Error> {
        Ok(self.get_dependency(v))
    }
}

#[derive(Debug)]
pub(crate) struct HighwayProtocol<C: Context> {
    synchronizer: DagSynchronizerState<Highway<C>>,
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

impl<C: Context> ConsensusProtocol<C::ConsensusValue> for HighwayProtocol<C> {
    fn handle_message(
        &mut self,
        sender: NodeId,
        msg: Vec<u8>,
    ) -> Result<Vec<ConsensusProtocolResult<<C as Context>::ConsensusValue>>, Error> {
        let highway_message: HighwayMessage<C> = serde_json::from_slice(msg.as_slice()).unwrap();
        Ok(match highway_message {
            HighwayMessage::NewVertex(v) => {
                let mut new_vertices = vec![v];
                let mut effects = vec![];
                // TODO: Is there a danger that this takes too much time, and starves other
                // components and events? Consider replacing the loop with a "callback" effect:
                // Instead of handling `HighwayMessage::NewVertex(v)` directly, return a
                // `EnqueueVertex(v)` that causes the reactor to call us with an
                // `Event::NewVertex(v)`, and call `add_vertex` when handling that event. For each
                // returned vertex that needs to be requeued, also return an `EnqueueVertex`
                // effect.
                while let Some(v) = new_vertices.pop() {
                    // TODO: This is wrong!! `add_vertex` should not be called with `sender`, but
                    // with the node that sent us `v` in the first place.
                    match self
                        .synchronizer
                        .add_vertex(sender, v.clone(), &mut self.highway)
                    {
                        Ok(SynchronizerEffect::RequestVertex(sender, missing_vid)) => {
                            let msg = HighwayMessage::RequestDependency(missing_vid);
                            let serialized_msg = serde_json::to_vec_pretty(&msg)?;
                            effects.push(ConsensusProtocolResult::CreatedTargetedMessage(
                                serialized_msg,
                                sender,
                            ));
                        }
                        Ok(SynchronizerEffect::RequeueVertex(vertices)) => {
                            new_vertices.extend(vertices);
                            // TODO: Add new vertex to state. (Indirectly, via synchronizer?)
                            let msg = HighwayMessage::NewVertex(v);
                            // TODO: Don't `unwrap`.
                            let serialized_msg = serde_json::to_vec_pretty(&msg).unwrap();
                            effects.push(ConsensusProtocolResult::CreatedGossipMessage(
                                serialized_msg,
                            ))
                        }
                        Ok(SynchronizerEffect::RequestConsensusValue(sender, value)) => {
                            effects.push(ConsensusProtocolResult::ValidateConsensusValue(
                                sender, value,
                            ));
                        }
                        Ok(SynchronizerEffect::InvalidVertex(v, sender, err)) => {
                            warn!("Invalid vertex from {:?}: {:?}, {:?}", v, sender, err);
                        }
                        Err(err) => todo!("error: {:?}", err),
                    }
                }
                effects
            }
            HighwayMessage::RequestDependency(_dep) => todo!(),
        })
    }

    fn handle_timer(
        &mut self,
        timestamp: Timestamp,
    ) -> Result<Vec<ConsensusProtocolResult<<C as Context>::ConsensusValue>>, Error> {
        Ok(self
            .highway
            .handle_timer(timestamp.0)
            .into_iter()
            .map(|effect| match effect {
                AvEffect::NewVertex(v) => {
                    let msg = HighwayMessage::NewVertex(v);
                    // TODO: Add new vertex to state. (Indirectly, via synchronizer?)
                    // TODO: Don't `unwrap`.
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
    ) -> Result<Vec<ConsensusProtocolResult<<C as Context>::ConsensusValue>>, Error> {
        // TODO: Deduplicate
        Ok(self
            .highway
            .propose(value, block_context)
            .into_iter()
            .map(|effect| match effect {
                AvEffect::NewVertex(v) => {
                    //TODO: Don't unwrap
                    // Replace serde with generic serializer.
                    let vertex_bytes = serde_json::to_vec_pretty(&v).unwrap();
                    ConsensusProtocolResult::CreatedGossipMessage(vertex_bytes)
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
        _value: &C::ConsensusValue,
        _valid: bool,
    ) -> Result<Vec<ConsensusProtocolResult<C::ConsensusValue>>, Error> {
        todo!()
    }
}
