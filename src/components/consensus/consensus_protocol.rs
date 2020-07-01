// TODO: Remove when all code is used
#![allow(dead_code)]
use std::fmt::Debug;

use anyhow::Error;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::components::{
    consensus::{
        consensus_protocol::synchronizer::{DagSynchronizerState, SynchronizerEffect},
        highway_core::{
            active_validator::{BlockContext, Effect as AvEffect},
            finality_detector::FinalityDetector,
            highway::Highway,
            vertex::{Dependency, Vertex},
        },
        traits::{ConsensusValueT, Context},
    },
    small_network::NodeId,
};

mod protocol_state;
mod synchronizer;

pub(crate) use protocol_state::{AddVertexOk, ProtocolState, VertexTrait};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TimerId(pub(crate) u64);

#[derive(Debug)]
pub(crate) enum ConsensusProtocolResult<C: ConsensusValueT> {
    CreatedGossipMessage(Vec<u8>),
    CreatedTargetedMessage(Vec<u8>, NodeId),
    InvalidIncomingMessage(Vec<u8>, Error),
    ScheduleTimer(u64, TimerId),
    /// Request deploys for a new block, whose timestamp will be the given `u64`.
    /// TODO: Add more details that are necessary for block creation.
    CreateNewBlock(u64),
    FinalizedBlock(C),
    /// Request validation of the consensus value, contained in a message received from the given
    /// node.
    ///
    /// The domain logic should verify any intrinsic validity conditions of consensus values, e.g.
    /// that it has the expected structure, or that deploys that are mentioned by hash actually
    /// exist, and then call `ConsensusProtocol::resolve_validity`.
    ValidateConsensusValue(NodeId, C),
}

/// An API for a single instance of the consensus.
pub(crate) trait ConsensusProtocol<C: ConsensusValueT> {
    /// Handles an incoming message (like NewVote, RequestDependency).
    fn handle_message(
        &mut self,
        sender: NodeId,
        msg: Vec<u8>,
    ) -> Result<Vec<ConsensusProtocolResult<C>>, Error>;

    /// Triggers consensus' timer.
    fn handle_timer(&mut self, timer_id: TimerId)
        -> Result<Vec<ConsensusProtocolResult<C>>, Error>;

    fn propose(
        &self,
        value: C,
        block_context: BlockContext,
    ) -> Result<Vec<ConsensusProtocolResult<C>>, Error>;

    /// Marks the `value` as valid or invalid, based on validation requested via
    /// `ConsensusProtocolResult::ValidateConsensusvalue`.
    fn resolve_validity(
        &mut self,
        value: &C,
        valid: bool,
    ) -> Result<Vec<ConsensusProtocolResult<C>>, Error>;
}

struct HighwayProtocol<C: Context> {
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
                            // TODO: Handle effects from `highway.on_new_vote`.
                            let _additional_effects = match &v {
                                Vertex::Vote(swv) => {
                                    self.highway.on_new_vote(&swv.hash(), swv.wire_vote.instant)
                                }
                                Vertex::Evidence(_) => {
                                    // Do nothing?
                                    vec![]
                                }
                            };
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
        timer_id: TimerId,
    ) -> Result<Vec<ConsensusProtocolResult<<C as Context>::ConsensusValue>>, Error> {
        Ok(self
            .highway
            .handle_timer(timer_id.0)
            .into_iter()
            .map(|effect| match effect {
                AvEffect::NewVertex(v) => {
                    //TODO: Don't unwrap
                    // Replace serde with generic serializer.
                    let vertex_bytes = serde_json::to_vec_pretty(&v).unwrap();
                    ConsensusProtocolResult::CreatedGossipMessage(vertex_bytes)
                }
                AvEffect::ScheduleTimer(instant_u64) => {
                    ConsensusProtocolResult::ScheduleTimer(instant_u64, TimerId(instant_u64))
                }
                AvEffect::RequestNewBlock(block_context) => {
                    ConsensusProtocolResult::CreateNewBlock(block_context.instant())
                }
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
                    ConsensusProtocolResult::ScheduleTimer(instant_u64, TimerId(instant_u64))
                }
                AvEffect::RequestNewBlock(block_context) => {
                    ConsensusProtocolResult::CreateNewBlock(block_context.instant())
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

#[cfg(test)]
mod example {
    use serde::{Deserialize, Serialize};

    use super::{
        protocol_state::{ProtocolState, VertexTrait},
        synchronizer::DagSynchronizerState,
        BlockContext, ConsensusProtocol, ConsensusProtocolResult, NodeId, TimerId,
    };

    #[derive(Debug, Hash, PartialEq, Eq, Clone, PartialOrd, Ord)]
    struct VIdU64(u64);

    #[derive(Debug, Hash, PartialEq, Eq, Clone, Serialize, Deserialize)]
    struct DummyVertex {
        id: u64,
        proto_block: ProtoBlock,
    }

    impl VertexTrait for DummyVertex {
        type Id = VIdU64;
        type Value = ProtoBlock;

        fn id(&self) -> VIdU64 {
            VIdU64(self.id)
        }

        fn value(&self) -> Option<&ProtoBlock> {
            Some(&self.proto_block)
        }
    }

    #[derive(Debug, Hash, PartialEq, Eq, Clone, Serialize, Deserialize)]
    struct ProtoBlock(u64);

    #[derive(Debug)]
    struct Error;

    impl<P: ProtocolState> ConsensusProtocol<ProtoBlock> for DagSynchronizerState<P> {
        fn handle_message(
            &mut self,
            _sender: NodeId,
            _msg: Vec<u8>,
        ) -> Result<Vec<ConsensusProtocolResult<ProtoBlock>>, anyhow::Error> {
            unimplemented!()
        }

        fn handle_timer(
            &mut self,
            _timer_id: TimerId,
        ) -> Result<Vec<ConsensusProtocolResult<ProtoBlock>>, anyhow::Error> {
            unimplemented!()
        }

        fn resolve_validity(
            &mut self,
            _value: &ProtoBlock,
            _valid: bool,
        ) -> Result<Vec<ConsensusProtocolResult<ProtoBlock>>, anyhow::Error> {
            unimplemented!()
        }

        fn propose(
            &self,
            _value: ProtoBlock,
            _block_context: BlockContext,
        ) -> Result<Vec<ConsensusProtocolResult<ProtoBlock>>, anyhow::Error> {
            unimplemented!()
        }
    }
}
