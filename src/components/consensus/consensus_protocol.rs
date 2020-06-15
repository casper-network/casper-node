// TODO: Remove when all code is used
#![allow(dead_code)]
use std::{hash::Hash, time::Instant};

mod protocol_state;
mod synchronizer;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TimerId(pub(crate) u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NodeId(u64);

pub(crate) trait ConsensusContext {
    /// Consensus specific message.
    /// What gets sent over the wire is opaque to the networking layer,
    /// it is materialized to concrete type in the consensus protocol layer.
    ///
    /// Example ADT might be:
    /// ```ignore
    /// enum Message {
    ///   NewVote(…),
    ///   NewBlock(…),
    ///   RequestDependency(…),
    /// }
    /// ```
    ///
    /// Note that some consensus protocols (like HoneyBadgerBFT) don't have dependencies,
    /// so it's not possible to differentiate between new message and dependency requests
    /// in consensus-agnostic layers.
    type Message;

    type ConsensusValue: Hash + PartialEq + Eq;
}

#[derive(Debug)]
pub(crate) enum ConsensusProtocolResult<Ctx: ConsensusContext> {
    CreatedNewMessage(Ctx::Message),
    InvalidIncomingMessage(Ctx::Message, anyhow::Error),
    ScheduleTimer(Instant, TimerId),
    CreateNewBlock,
    FinalizedBlock(Ctx::ConsensusValue),
}

/// An API for a single instance of the consensus.
pub(crate) trait ConsensusProtocol<Ctx: ConsensusContext> {
    /// Handle an incoming message (like NewVote, RequestDependency).
    fn handle_message(
        &mut self,
        msg: Ctx::Message,
    ) -> Result<Vec<ConsensusProtocolResult<Ctx>>, anyhow::Error>;

    /// Triggers consensus' timer.
    fn handle_timer(
        &mut self,
        timer_id: TimerId,
    ) -> Result<Vec<ConsensusProtocolResult<Ctx>>, anyhow::Error>;
}

#[cfg(test)]
mod example {
    use super::{
        protocol_state::{ProtocolState, Vertex},
        synchronizer::DagSynchronizerState,
        ConsensusContext, ConsensusProtocol, ConsensusProtocolResult, TimerId,
    };
    use anyhow::Error;

    struct HighwayContext();

    #[derive(Debug, Hash, PartialEq, Eq, Clone)]
    struct VIdU64(u64);

    #[derive(Debug, Hash, PartialEq, Eq, Clone)]
    struct DummyVertex {
        id: u64,
        deploy_hash: DeployHash,
    }

    impl Vertex<DeployHash, VIdU64> for DummyVertex {
        fn id(&self) -> VIdU64 {
            VIdU64(self.id)
        }

        fn values(&self) -> Vec<DeployHash> {
            vec![self.deploy_hash.clone()]
        }
    }

    #[derive(Debug, Hash, PartialEq, Eq, Clone)]
    struct DeployHash(u64);

    impl ConsensusContext for HighwayContext {
        type Message = HighwayMessage;
        type ConsensusValue = DeployHash;
    }

    enum HighwayMessage {
        NewVertex(DummyVertex),
        RequestVertex(VIdU64),
    }

    impl<P: ProtocolState<VIdU64, DummyVertex>> ConsensusProtocol<HighwayContext>
        for DagSynchronizerState<VIdU64, DummyVertex, DeployHash, P>
    {
        fn handle_message(
            &mut self,
            msg: <HighwayContext as ConsensusContext>::Message,
        ) -> Result<Vec<ConsensusProtocolResult<HighwayContext>>, Error> {
            match msg {
                HighwayMessage::RequestVertex(_v_id) => unimplemented!(),
                HighwayMessage::NewVertex(_vertex) => unimplemented!(),
            }
        }

        fn handle_timer(
            &mut self,
            _timer_id: TimerId,
        ) -> Result<Vec<ConsensusProtocolResult<HighwayContext>>, Error> {
            unimplemented!()
        }
    }
}
