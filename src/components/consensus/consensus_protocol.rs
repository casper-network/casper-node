// TODO: Remove when all code is used
#![allow(dead_code)]
use std::{fmt::Debug, hash::Hash, time::Instant};

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use crate::components::consensus::highway_core::active_validator::ActiveValidator;
use crate::components::consensus::traits::Context;

mod protocol_state;
mod synchronizer;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TimerId(pub(crate) u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct NodeId(u64);

pub(crate) trait ConsensusValue:
    Hash + PartialEq + Eq + Serialize + DeserializeOwned
{
}
impl<T> ConsensusValue for T where T: Hash + PartialEq + Eq + Serialize + DeserializeOwned {}

#[derive(Debug)]
pub(crate) enum ConsensusProtocolResult<C: ConsensusValue> {
    CreatedNewMessage(Vec<u8>),
    InvalidIncomingMessage(Vec<u8>, anyhow::Error),
    ScheduleTimer(Instant, TimerId),
    CreateNewBlock,
    FinalizedBlock(C),
}

/// An API for a single instance of the consensus.
pub(crate) trait ConsensusProtocol<C: ConsensusValue> {
    /// Handle an incoming message (like NewVote, RequestDependency).
    fn handle_message(
        &mut self,
        msg: Vec<u8>,
    ) -> Result<Vec<ConsensusProtocolResult<C>>, anyhow::Error>;

    /// Triggers consensus' timer.
    fn handle_timer(
        &mut self,
        timer_id: TimerId,
    ) -> Result<Vec<ConsensusProtocolResult<C>>, anyhow::Error>;
}

struct ConsensusInstance<C: Context> {
    active_validator: Option<ActiveValidator<C>>
}

#[cfg(test)]
mod example {
    use serde::{Deserialize, Serialize};

    use super::{
        protocol_state::{ProtocolState, Vertex},
        synchronizer::DagSynchronizerState,
        ConsensusProtocol, ConsensusProtocolResult, TimerId,
    };

    #[derive(Debug, Hash, PartialEq, Eq, Clone)]
    struct VIdU64(u64);

    #[derive(Debug, Hash, PartialEq, Eq, Clone, Serialize, Deserialize)]
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

    #[derive(Debug, Hash, PartialEq, Eq, Clone, Serialize, Deserialize)]
    struct DeployHash(u64);

    #[derive(Debug)]
    struct Error;

    impl<P: ProtocolState<VIdU64, DummyVertex>> ConsensusProtocol<DeployHash>
        for DagSynchronizerState<VIdU64, DummyVertex, DeployHash, P>
    {
        fn handle_message(
            &mut self,
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
