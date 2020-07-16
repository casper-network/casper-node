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
use maplit::hashmap;
use num_traits::{AsPrimitive, Zero};
use serde::{Deserialize, Serialize};
use tracing::error;
use types::U512;

use crate::{
    components::consensus::{
        consensus_protocol::{ConsensusProtocol, ConsensusProtocolResult},
        highway_core::highway::HighwayParams,
        protocols::highway::{HighwayContext, HighwayProtocol, HighwaySecret},
        traits::NodeIdT,
        ConsensusMessage, Event, ReactorEventT,
    },
    crypto::{
        asymmetric_key::{generate_ed25519_keypair, PublicKey},
        hash::hash,
    },
    effect::{EffectBuilder, EffectExt, Effects},
    types::{Motes, ProtoBlock, Timestamp},
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

pub(crate) struct EraSupervisor<I> {
    // A map of active consensus protocols.
    // A value is a trait so that we can run different consensus protocol instances per era.
    active_eras: HashMap<EraId, Box<dyn ConsensusProtocol<I, ProtoBlock>>>,
    era_config: EraConfig,
}

impl<I> Debug for EraSupervisor<I> {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "EraSupervisor {{ era_config: {:?}, .. }}",
            self.era_config
        )
    }
}

impl<I> EraSupervisor<I>
where
    I: NodeIdT,
{
    pub(crate) fn new<REv: ReactorEventT<I>>(
        timestamp: Timestamp,
        effect_builder: EffectBuilder<REv>,
        validators: Vec<(PublicKey, Motes)>,
    ) -> (Self, Effects<Event<I>>) {
        let sum_stakes: Motes = validators
            .iter()
            .map(|(_, stake)| *stake)
            .fold(Motes::zero(), |a, b| a + b);
        let weights = if sum_stakes.value() > U512::from(u64::MAX) {
            todo!(
                "Stakes exceeding the u64 range have to be mapped to weights somehow (see HWY-121)"
            )
        } else {
            validators
                .into_iter()
                .map(|(key, stake)| (key, AsPrimitive::<u64>::as_(stake.value())))
                .collect()
        };
        // TODO: take our actual key from _somewhere_
        let (secret_key, public_key) = generate_ed25519_keypair();
        let params = HighwayParams {
            instance_id: hash("test era 0"),
            validators: weights,
        };
        let (highway, effects) = HighwayProtocol::<I, HighwayContext>::new(
            params,
            0, // TODO: get a proper seed ?
            public_key,
            HighwaySecret::new(secret_key, public_key),
            12, // 4.1 seconds; TODO: get a proper round exp
            timestamp,
        );
        let initial_era: Box<dyn ConsensusProtocol<I, ProtoBlock>> = Box::new(highway);
        let active_eras = hashmap! { EraId(0) => initial_era };
        let era_supervisor = Self {
            active_eras,
            era_config: Default::default(),
        };
        let effects = effects
            .into_iter()
            .flat_map(|result| Self::handle_consensus_result(EraId(0), effect_builder, result))
            .collect();
        (era_supervisor, effects)
    }

    fn handle_consensus_result<REv: ReactorEventT<I>>(
        era_id: EraId,
        effect_builder: EffectBuilder<REv>,
        consensus_result: ConsensusProtocolResult<I, ProtoBlock>,
    ) -> Effects<Event<I>> {
        match consensus_result {
            ConsensusProtocolResult::InvalidIncomingMessage(msg, sender, error) => {
                // TODO: we will probably want to disconnect from the sender here
                // TODO: Print a more readable representation of the message.
                error!(
                    ?msg,
                    ?sender,
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
            ConsensusProtocolResult::ScheduleTimer(timestamp) => {
                let timediff = timestamp.saturating_sub(Timestamp::now());
                effect_builder
                    .set_timeout(timediff.into())
                    .event(move |_| Event::Timer { era_id, timestamp })
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
                .validate_proto_block(sender.clone(), proto_block)
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
    ) -> Effects<Event<I>>
    where
        REv: ReactorEventT<I>,
        F: FnOnce(
            &mut dyn ConsensusProtocol<I, ProtoBlock>,
        ) -> Result<Vec<ConsensusProtocolResult<I, ProtoBlock>>, Error>,
    {
        match self.active_eras.get_mut(&era_id) {
            None => todo!("Handle missing eras."),
            Some(consensus) => match f(&mut **consensus) {
                Ok(results) => results
                    .into_iter()
                    .flat_map(|result| {
                        Self::handle_consensus_result(era_id, effect_builder, result)
                    })
                    .collect(),
                Err(error) => {
                    error!(%error, ?era_id, "got error from era id {:?}: {:?}", era_id, error);
                    Effects::new()
                }
            },
        }
    }
}
