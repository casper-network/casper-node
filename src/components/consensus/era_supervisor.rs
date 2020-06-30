//! Consensus service is a component that will be communicating with the reactor.
//! It will receive events (like incoming message event or create new message event)
//! and propagate them to the underlying consensus protocol.
//! It tries to know as little as possible about the underlying consensus. The only thing
//! it assumes is the concept of era/epoch and that each era runs separate consensus instance.
//! Most importantly, it doesn't care about what messages it's forwarding.

use std::{
    collections::HashMap,
    fmt::{self, Debug, Formatter},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tracing::error;

use crate::{
    components::{
        consensus::{
            consensus_protocol::{ConsensusProtocol, ConsensusProtocolResult, ConsensusValue},
            ConsensusMessage, Event,
        },
        small_network::NodeId,
    },
    effect::{requests::NetworkRequest, Effect, EffectBuilder, Multiple},
};

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EraId(pub(crate) u64);

#[derive(Clone, Debug)]
struct EraConfig {
    era_length: Duration,
    //TODO: Are these necessary for every consensus protocol?
    booking_duration: Duration,
    entropy_duration: Duration,
}

impl Default for EraConfig {
    fn default() -> Self {
        // TODO: no idea what the default values should be and if implementing defaults makes
        // sense, this is just for the time being
        Self {
            era_length: Duration::from_secs(86_400),       // one day
            booking_duration: Duration::from_secs(43_200), // half a day
            entropy_duration: Duration::from_secs(3_600),
        }
    }
}

#[derive(Clone, Debug)]
struct EraInstance<Id> {
    era_id: Id,
    era_start: u64,
    era_end: u64,
}

pub(crate) struct EraSupervisor<C: ConsensusValue> {
    // A map of active consensus protocols.
    // A value is a trait so that we can run different consensus protocol instances per era.
    active_eras: HashMap<EraId, Box<dyn ConsensusProtocol<C>>>,
    era_config: EraConfig,
}

impl<C: ConsensusValue> Debug for EraSupervisor<C> {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "EraSupervisor {{ era_config: {:?}, .. }}",
            self.era_config
        )
    }
}

impl<C: ConsensusValue> EraSupervisor<C> {
    pub(crate) fn new() -> Self {
        Self {
            active_eras: HashMap::new(),
            era_config: Default::default(),
        }
    }

    pub(crate) fn handle_message<REv>(
        &mut self,
        _effect_builder: EffectBuilder<REv>,
        sender: NodeId,
        era_id: EraId,
        payload: Vec<u8>,
    ) -> Multiple<Effect<Event>>
    where
        REv: From<Event> + Send + From<NetworkRequest<NodeId, ConsensusMessage>>,
    {
        match self.active_eras.get_mut(&era_id) {
            None => todo!("Handle missing eras."),
            Some(consensus) => match consensus.handle_message(sender, payload) {
                Ok(results) => results
                    .into_iter()
                    .map(|result| match result {
                        ConsensusProtocolResult::InvalidIncomingMessage(_msg, _error) => {
                            unimplemented!()
                        }
                        ConsensusProtocolResult::CreatedGossipMessage(_out_msg) => {
                            todo!("Create an effect to broadcast new msg")
                        }
                        ConsensusProtocolResult::CreatedTargetedMessage(_out_msg, _to) => {
                            todo!("Create an effect to send new msg")
                        }
                        ConsensusProtocolResult::ScheduleTimer(_delay, _timer_id) => {
                            unimplemented!()
                        }
                        ConsensusProtocolResult::CreateNewBlock(_instant) => unimplemented!(),
                        ConsensusProtocolResult::FinalizedBlock(_block) => unimplemented!(),
                        ConsensusProtocolResult::RequestConsensusValues(_sender, _values) => {
                            unimplemented!()
                        }
                    })
                    .collect(),
                Err(error) => {
                    error!(%error, ?era_id, "got error from era id {:?}: {:?}", era_id, error);
                    Default::default()
                }
            },
        }
    }
}
