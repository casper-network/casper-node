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

use anyhow::Error;
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::{
    components::{
        consensus::{
            consensus_protocol::{ConsensusProtocol, ConsensusProtocolResult},
            ConsensusMessage, Event,
        },
        small_network::NodeId,
    },
    effect::{requests::NetworkRequest, Effect, EffectBuilder, EffectExt, Multiple},
    types::ProtoBlock,
};

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct EraId(pub(crate) u64);

impl EraId {
    fn message(self, payload: Vec<u8>) -> ConsensusMessage {
        ConsensusMessage {
            era_id: self,
            payload,
        }
    }
}

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

pub(crate) struct EraSupervisor {
    // A map of active consensus protocols.
    // A value is a trait so that we can run different consensus protocol instances per era.
    active_eras: HashMap<EraId, Box<dyn ConsensusProtocol<ProtoBlock>>>,
    era_config: EraConfig,
}

impl Debug for EraSupervisor {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "EraSupervisor {{ era_config: {:?}, .. }}",
            self.era_config
        )
    }
}

impl EraSupervisor {
    pub(crate) fn new() -> Self {
        Self {
            active_eras: HashMap::new(),
            era_config: Default::default(),
        }
    }

    fn handle_consensus_result<REv>(
        &self,
        era_id: EraId,
        effect_builder: EffectBuilder<REv>,
        consensus_result: ConsensusProtocolResult<ProtoBlock>,
    ) -> Multiple<Effect<Event>>
    where
        REv: From<Event> + Send + From<NetworkRequest<NodeId, ConsensusMessage>>,
    {
        match consensus_result {
            ConsensusProtocolResult::InvalidIncomingMessage(msg, error) => {
                // TODO: we will probably want to disconnect from the sender here
                // TODO: Print a more readable representation of the message.
                error!(
                    ?msg,
                    ?error,
                    "invalid incoming message to consensus instance"
                );
                Default::default()
            }
            ConsensusProtocolResult::CreatedGossipMessage(out_msg) => {
                // TODO: we'll want to gossip instead of broadcast here
                effect_builder
                    .broadcast_message(era_id.message(out_msg))
                    .ignore()
            }
            ConsensusProtocolResult::CreatedTargetedMessage(out_msg, to) => effect_builder
                .send_message(to, era_id.message(out_msg))
                .ignore(),
            ConsensusProtocolResult::ScheduleTimer(_timestamp) => {
                // TODO: we need to get the current system time here somehow, in order to schedule
                // a timer for the correct moment - and we don't want to use std::Instant
                unimplemented!()
            }
            ConsensusProtocolResult::CreateNewBlock(block_context) => effect_builder
                .request_proto_block(block_context)
                .event(move |(proto_block, block_context)| Event::NewProtoBlock {
                    era_id,
                    proto_block,
                    block_context,
                }),
            ConsensusProtocolResult::FinalizedBlock(block) => effect_builder
                .execute_block(block)
                .event(move |executed_block| Event::ExecutedBlock {
                    era_id,
                    executed_block,
                }),
            ConsensusProtocolResult::ValidateConsensusValue(sender, proto_block) => effect_builder
                .validate_proto_block(sender, proto_block)
                .event(move |(is_valid, proto_block)| {
                    if is_valid {
                        Event::AcceptProtoBlock {
                            era_id,
                            proto_block,
                        }
                    } else {
                        Event::InvalidProtoBlock {
                            era_id,
                            sender,
                            proto_block,
                        }
                    }
                }),
        }
    }

    pub(crate) fn delegate_to_era<F, REv>(
        &mut self,
        era_id: EraId,
        effect_builder: EffectBuilder<REv>,
        f: F,
    ) -> Multiple<Effect<Event>>
    where
        REv: From<Event> + Send + From<NetworkRequest<NodeId, ConsensusMessage>>,
        F: FnOnce(
            &mut dyn ConsensusProtocol<ProtoBlock>,
        ) -> Result<Vec<ConsensusProtocolResult<ProtoBlock>>, Error>,
    {
        match self.active_eras.get_mut(&era_id) {
            None => todo!("Handle missing eras."),
            Some(consensus) => match f(&mut **consensus) {
                Ok(results) => results
                    .into_iter()
                    .flat_map(|result| self.handle_consensus_result(era_id, effect_builder, result))
                    .collect(),
                Err(error) => {
                    error!(%error, ?era_id, "got error from era id {:?}: {:?}", era_id, error);
                    Default::default()
                }
            },
        }
    }
}
