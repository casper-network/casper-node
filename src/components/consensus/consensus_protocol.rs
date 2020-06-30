// TODO: Remove when all code is used
#![allow(dead_code)]
use std::{fmt::Debug, hash::Hash};

use anyhow::Error;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tracing::warn;

use crate::components::{
    consensus::{
        consensus_protocol::synchronizer::{DagSynchronizerState, SynchronizerEffect},
        highway_core::{
            active_validator::{ActiveValidator, Effect as AvEffect},
            finality_detector::FinalityDetector,
            highway::Highway,
            vertex::{Dependency, Vertex},
        },
        traits::Context,
    },
    small_network::NodeId,
};

mod protocol_state;
mod synchronizer;

pub(crate) use protocol_state::{AddVertexOk, ProtocolState, VertexTrait};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TimerId(pub(crate) u64);

pub(crate) trait ConsensusValue:
    Hash + PartialEq + Eq + Serialize + DeserializeOwned
{
}
impl<T> ConsensusValue for T where T: Hash + PartialEq + Eq + Serialize + DeserializeOwned {}

#[derive(Debug)]
pub(crate) enum ConsensusProtocolResult<C: ConsensusValue> {
    CreatedGossipMessage(Vec<u8>),
    CreatedTargetedMessage(Vec<u8>, NodeId),
    InvalidIncomingMessage(Vec<u8>, Error),
    ScheduleTimer(u64, TimerId),
    /// Request deploys for a new block, whose timestamp will be the given `u64`.
    CreateNewBlock(u64),
    FinalizedBlock(C),
    RequestConsensusValues(NodeId, Vec<C>),
}

/// An API for a single instance of the consensus.
pub(crate) trait ConsensusProtocol<C: ConsensusValue> {
    /// Handle an incoming message (like NewVote, RequestDependency).
    fn handle_message(
        &mut self,
        sender: NodeId,
        msg: Vec<u8>,
    ) -> Result<Vec<ConsensusProtocolResult<C>>, Error>;

    /// Triggers consensus' timer.
    fn handle_timer(&mut self, timer_id: TimerId)
        -> Result<Vec<ConsensusProtocolResult<C>>, Error>;
}

struct HighwayProtocol<C: Context> {
    active_validator: Option<ActiveValidator<C>>,
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
                            let msg = HighwayMessage::NewVertex(v);
                            // TODO: Add new vertex to state. (Indirectly, via synchronizer?)
                            // TODO: Don't `unwrap`.
                            let serialized_msg = serde_json::to_vec_pretty(&msg).unwrap();
                            effects.push(ConsensusProtocolResult::CreatedGossipMessage(
                                serialized_msg,
                            ))
                        }
                        Ok(SynchronizerEffect::RequestConsensusValues(sender, values)) => {
                            effects.push(ConsensusProtocolResult::RequestConsensusValues(
                                sender, values,
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
        _timer_id: TimerId,
    ) -> Result<Vec<ConsensusProtocolResult<<C as Context>::ConsensusValue>>, Error> {
        // TODO: Instant!
        let av = match self.active_validator.as_mut() {
            Some(av) => av,
            None => return Ok(vec![]),
        };
        let state = self.highway.state();
        Ok(av
            .handle_timer(0, state)
            .into_iter()
            .map(|effect| match effect {
                AvEffect::NewVertex(v) => {
                    let msg = HighwayMessage::NewVertex(v);
                    // TODO: Add new vertex to state. (Indirectly, via synchronizer?)
                    // TODO: Don't `unwrap`.
                    let serialized_msg = serde_json::to_vec_pretty(&msg).unwrap();
                    ConsensusProtocolResult::CreatedGossipMessage(serialized_msg)
                }
                AvEffect::ScheduleTimer(instant) => {
                    ConsensusProtocolResult::ScheduleTimer(instant, TimerId(0))
                }
                AvEffect::RequestNewBlock(ctx) => {
                    ConsensusProtocolResult::CreateNewBlock(ctx.instant)
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod example {
    use serde::{Deserialize, Serialize};

    use super::{
        protocol_state::{ProtocolState, VertexTrait},
        synchronizer::DagSynchronizerState,
        ConsensusProtocol, ConsensusProtocolResult, NodeId, TimerId,
    };

    #[derive(Debug, Hash, PartialEq, Eq, Clone, PartialOrd, Ord)]
    struct VIdU64(u64);

    #[derive(Debug, Hash, PartialEq, Eq, Clone, Serialize, Deserialize)]
    struct DummyVertex {
        id: u64,
        deploy_hash: DeployHash,
    }

    impl VertexTrait for DummyVertex {
        type Id = VIdU64;
        type Value = DeployHash;

        fn id(&self) -> VIdU64 {
            VIdU64(self.id)
        }

        fn value(&self) -> Option<&DeployHash> {
            Some(&self.deploy_hash)
        }
    }

    #[derive(Debug, Hash, PartialEq, Eq, Clone, Serialize, Deserialize)]
    struct DeployHash(u64);

    #[derive(Debug)]
    struct Error;

    impl<P: ProtocolState> ConsensusProtocol<DeployHash> for DagSynchronizerState<P> {
        fn handle_message(
            &mut self,
            _sender: NodeId,
            _msg: Vec<u8>,
        ) -> Result<Vec<ConsensusProtocolResult<DeployHash>>, anyhow::Error> {
            unimplemented!()
        }

        fn handle_timer(
            &mut self,
            _timer_id: TimerId,
        ) -> Result<Vec<ConsensusProtocolResult<DeployHash>>, anyhow::Error> {
            unimplemented!()
        }
    }
}
