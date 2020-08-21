//! Consensus service is a component that will be communicating with the reactor.
//! It will receive events (like incoming message event or create new message event)
//! and propagate them to the underlying consensus protocol.
//! It tries to know as little as possible about the underlying consensus. The only thing
//! it assumes is the concept of era/epoch and that each era runs separate consensus instance.
//! Most importantly, it doesn't care about what messages it's forwarding.

use std::{
    collections::{BTreeMap, HashMap},
    fmt::{self, Debug, Formatter},
    rc::Rc,
};

use anyhow::Error;
use casperlabs_types::U512;
use num_traits::AsPrimitive;
use rand::Rng;
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::{
    components::{
        chainspec_loader::HighwayConfig,
        consensus::{
            consensus_protocol::{BlockContext, ConsensusProtocol, ConsensusProtocolResult},
            highway_core::validators::Validators,
            protocols::highway::{HighwayContext, HighwayProtocol, HighwaySecret},
            traits::NodeIdT,
            Config, ConsensusMessage, Event, ReactorEventT,
        },
    },
    crypto::asymmetric_key,
    crypto::{
        asymmetric_key::{PublicKey, SecretKey},
        hash,
    },
    effect::{EffectBuilder, EffectExt, Effects},
    types::{Block, FinalizedBlock, Motes, ProtoBlock, SystemTransaction, Timestamp},
    utils::WithDir,
};

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EraId(pub(crate) u64);

impl EraId {
    fn message(self, payload: Vec<u8>) -> ConsensusMessage {
        ConsensusMessage {
            era_id: self,
            payload,
        }
    }

    fn successor(self) -> EraId {
        EraId(self.0 + 1)
    }
}

pub(crate) struct Era<I> {
    /// The consensus protocol instance.
    consensus: Box<dyn ConsensusProtocol<I, ProtoBlock, PublicKey>>,
    /// The timestamp of the last block of the previous era.
    start_time: Timestamp,
    /// The height of this era's first block.
    start_height: u64,
}

pub(crate) struct EraSupervisor<I> {
    /// A map of active consensus protocols.
    /// A value is a trait so that we can run different consensus protocol instances per era.
    active_eras: HashMap<EraId, Era<I>>,
    pub(super) secret_signing_key: Rc<SecretKey>,
    pub(super) public_signing_key: PublicKey,
    validator_stakes: Vec<(PublicKey, Motes)>,
    current_era: EraId,
    highway_config: HighwayConfig,
}

impl<I> Debug for EraSupervisor<I> {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        let ae: Vec<_> = self.active_eras.keys().collect();
        write!(formatter, "EraSupervisor {{ active_eras: {:?}, .. }}", ae)
    }
}

impl<I> EraSupervisor<I>
where
    I: NodeIdT,
{
    pub(crate) fn new<REv: ReactorEventT<I>, R: Rng + ?Sized>(
        timestamp: Timestamp,
        config: WithDir<Config>,
        effect_builder: EffectBuilder<REv>,
        validator_stakes: Vec<(PublicKey, Motes)>,
        highway_config: &HighwayConfig,
        rng: &mut R,
    ) -> Result<(Self, Effects<Event<I>>), Error> {
        let (root, config) = config.into_parts();
        let secret_signing_key = Rc::new(config.secret_key_path.load(root)?);
        let public_signing_key = PublicKey::from(secret_signing_key.as_ref());

        let mut era_supervisor = Self {
            active_eras: Default::default(),
            secret_signing_key,
            public_signing_key,
            current_era: EraId(0),
            validator_stakes: validator_stakes.clone(),
            highway_config: *highway_config,
        };

        let effects = era_supervisor.handling(effect_builder, rng).new_era(
            EraId(0),
            timestamp,
            validator_stakes,
            highway_config.genesis_era_start_timestamp,
            0,
        );

        Ok((era_supervisor, effects))
    }

    pub(super) fn handling<'a, REv: ReactorEventT<I>, R: Rng + ?Sized>(
        &'a mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &'a mut R,
    ) -> HandlingEraSupervisor<'a, I, REv, R> {
        HandlingEraSupervisor {
            era_supervisor: self,
            effect_builder,
            rng,
        }
    }

    fn new_era(
        &mut self,
        era_id: EraId,
        timestamp: Timestamp,
        validator_stakes: Vec<(PublicKey, Motes)>,
        start_time: Timestamp,
        start_height: u64,
    ) -> Vec<ConsensusProtocolResult<I, ProtoBlock, PublicKey>> {
        if self.active_eras.contains_key(&era_id) {
            panic!("{:?} already exists", era_id);
        }
        let sum_stakes: Motes = validator_stakes.iter().map(|(_, stake)| *stake).sum();
        let validators: Validators<PublicKey> = if sum_stakes.value() > U512::from(u64::MAX) {
            validator_stakes
                .into_iter()
                .map(|(key, stake)| {
                    let weight = stake.value() / (sum_stakes.value() / (u64::MAX / 2));
                    (key, AsPrimitive::<u64>::as_(weight))
                })
                .collect()
        } else {
            validator_stakes
                .into_iter()
                .map(|(key, stake)| (key, AsPrimitive::<u64>::as_(stake.value())))
                .collect()
        };

        let instance_id = hash::hash(format!("Highway era {}", era_id.0));
        let secret =
            HighwaySecret::new(Rc::clone(&self.secret_signing_key), self.public_signing_key);
        let ftt = validators.total_weight()
            * u64::from(self.highway_config.finality_threshold_percent)
            / 100;
        let (highway, results) = HighwayProtocol::<I, HighwayContext>::new(
            instance_id,
            validators,
            0, // TODO: get a proper seed ?
            self.public_signing_key,
            secret,
            self.highway_config.minimum_round_exponent,
            ftt,
            timestamp,
            self.highway_config.minimum_era_height,
            start_time + self.highway_config.era_duration,
        );

        let era = Era {
            consensus: Box::new(highway),
            start_time,
            start_height,
        };
        let _ = self.active_eras.insert(era_id, era);

        results
    }

    fn handle_finalized_block(
        &mut self,
        era_id: EraId,
        proto_block: ProtoBlock,
        new_equivocators: Vec<PublicKey>,
        rewards: BTreeMap<PublicKey, u64>,
        timestamp: Timestamp,
        relative_height: u64,
    ) -> (
        Vec<ConsensusProtocolResult<I, ProtoBlock, PublicKey>>,
        FinalizedBlock,
    ) {
        assert_eq!(
            era_id, self.current_era,
            "finalized block in unexpected era"
        );
        let mut system_transactions: Vec<_> = new_equivocators
            .into_iter()
            .map(SystemTransaction::Slash)
            .collect();
        if !rewards.is_empty() {
            system_transactions.push(SystemTransaction::Rewards(rewards));
        };
        let start_height = self.active_eras[&era_id].start_height;
        let start_time = self.active_eras[&era_id].start_time;
        let switch_block = relative_height + 1 >= self.highway_config.minimum_era_height
            && timestamp >= start_time + self.highway_config.era_duration;
        // Request execution of the finalized block.
        let fb = FinalizedBlock::new(
            proto_block,
            timestamp,
            system_transactions,
            switch_block,
            era_id,
            start_height + relative_height,
        );
        let results = if fb.switch_block() {
            self.current_era_mut().consensus.deactivate_validator();
            // TODO: Learn the new weights from contract (validator rotation).
            let validator_stakes = self.validator_stakes.clone();
            self.current_era = fb.era_id().successor();
            self.new_era(
                fb.era_id().successor(),
                fb.timestamp(),
                validator_stakes,
                fb.timestamp(),
                fb.height() + 1,
            )
        } else {
            vec![]
        };
        (results, fb)
    }

    /// Returns the current era.
    fn current_era_mut(&mut self) -> &mut Era<I> {
        self.active_eras
            .get_mut(&self.current_era)
            .expect("current era does not exist")
    }

    /// Inspect the active eras.
    #[cfg(test)]
    pub(crate) fn active_eras(&self) -> &HashMap<EraId, Era<I>> {
        &self.active_eras
    }
}

/// A mutable `EraSupervisor` reference, together with an `EffectBuilder`.
///
/// This is a short-lived convenience type to avoid passing the effect builder through lots of
/// message calls, and making every method individually generic in `REv`. It is only instantiated
/// for the duration of handling a single event.
pub(super) struct HandlingEraSupervisor<'a, I, REv: 'static, R: ?Sized> {
    pub(super) era_supervisor: &'a mut EraSupervisor<I>,
    pub(super) effect_builder: EffectBuilder<REv>,
    pub(super) rng: &'a mut R,
}

impl<'a, I, REv, R> HandlingEraSupervisor<'a, I, REv, R>
where
    I: NodeIdT,
    REv: ReactorEventT<I>,
    R: Rng + ?Sized,
{
    fn delegate_to_era<F>(&mut self, era_id: EraId, f: F) -> Effects<Event<I>>
    where
        F: FnOnce(
            &mut dyn ConsensusProtocol<I, ProtoBlock, PublicKey>,
        ) -> Result<Vec<ConsensusProtocolResult<I, ProtoBlock, PublicKey>>, Error>,
    {
        match self.era_supervisor.active_eras.get_mut(&era_id) {
            None => {
                if era_id > self.era_supervisor.current_era {
                    info!("received message for future {:?}", era_id);
                } else {
                    info!("received message for obsolete {:?}", era_id);
                }
                Effects::new()
            }
            Some(era) => match f(&mut *era.consensus) {
                Ok(results) => results
                    .into_iter()
                    .flat_map(|result| self.handle_consensus_result(era_id, result))
                    .collect(),
                Err(error) => {
                    error!(%error, ?era_id, "got error from era id {:?}: {:?}", era_id, error);
                    Effects::new()
                }
            },
        }
    }

    pub(super) fn handle_timer(
        &mut self,
        era_id: EraId,
        timestamp: Timestamp,
    ) -> Effects<Event<I>> {
        self.delegate_to_era(era_id, move |consensus| consensus.handle_timer(timestamp))
    }

    pub(super) fn handle_message(&mut self, sender: I, msg: ConsensusMessage) -> Effects<Event<I>> {
        let ConsensusMessage { era_id, payload } = msg;
        self.delegate_to_era(era_id, move |consensus| {
            consensus.handle_message(sender, payload)
        })
    }

    pub(super) fn handle_new_proto_block(
        &mut self,
        era_id: EraId,
        proto_block: ProtoBlock,
        block_context: BlockContext,
    ) -> Effects<Event<I>> {
        let mut effects = self
            .effect_builder
            .announce_proposed_proto_block(proto_block.clone())
            .ignore();
        effects.extend(self.delegate_to_era(era_id, move |consensus| {
            consensus.propose(proto_block, block_context)
        }));
        effects
    }

    pub(super) fn handle_executed_block(
        &mut self,
        _era_id: EraId,
        mut block: Block,
    ) -> Effects<Event<I>> {
        // TODO - we should only sign if we're a validator for the given era ID.
        let signature = asymmetric_key::sign(
            block.hash().inner(),
            &self.era_supervisor.secret_signing_key,
            &self.era_supervisor.public_signing_key,
        );
        block.append_proof(signature);
        self.effect_builder
            .put_block_to_storage(Box::new(block))
            .ignore()
    }

    pub(super) fn handle_accept_proto_block(
        &mut self,
        era_id: EraId,
        proto_block: ProtoBlock,
    ) -> Effects<Event<I>> {
        let mut effects = self.delegate_to_era(era_id, |consensus| {
            consensus.resolve_validity(&proto_block, true)
        });
        effects.extend(
            self.effect_builder
                .announce_proposed_proto_block(proto_block)
                .ignore(),
        );
        effects
    }

    pub(super) fn handle_invalid_proto_block(
        &mut self,
        era_id: EraId,
        _sender: I,
        proto_block: ProtoBlock,
    ) -> Effects<Event<I>> {
        self.delegate_to_era(era_id, |consensus| {
            consensus.resolve_validity(&proto_block, false)
        })
    }

    fn new_era(
        &mut self,
        era_id: EraId,
        timestamp: Timestamp,
        validator_stakes: Vec<(PublicKey, Motes)>,
        start_time: Timestamp,
        start_height: u64,
    ) -> Effects<Event<I>> {
        let results = self.era_supervisor.new_era(
            era_id,
            timestamp,
            validator_stakes,
            start_time,
            start_height,
        );

        results
            .into_iter()
            .flat_map(|result| self.handle_consensus_result(era_id, result))
            .collect()
    }

    fn handle_consensus_result(
        &mut self,
        era_id: EraId,
        consensus_result: ConsensusProtocolResult<I, ProtoBlock, PublicKey>,
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
                self.effect_builder
                    .broadcast_message(era_id.message(out_msg))
                    .ignore()
            }
            ConsensusProtocolResult::CreatedTargetedMessage(out_msg, to) => self
                .effect_builder
                .send_message(to, era_id.message(out_msg))
                .ignore(),
            ConsensusProtocolResult::ScheduleTimer(timestamp) => {
                let timediff = timestamp.saturating_sub(Timestamp::now());
                self.effect_builder
                    .set_timeout(timediff.into())
                    .event(move |_| Event::Timer { era_id, timestamp })
            }
            ConsensusProtocolResult::CreateNewBlock {
                block_context,
                opt_parent,
            } => self
                .effect_builder
                .request_proto_block(block_context, opt_parent, self.rng.gen())
                .event(move |(proto_block, block_context)| Event::NewProtoBlock {
                    era_id,
                    proto_block,
                    block_context,
                }),
            ConsensusProtocolResult::FinalizedBlock {
                value: proto_block,
                new_equivocators,
                rewards,
                timestamp,
                relative_height,
            } => {
                // Announce the finalized proto block.
                let mut effects = self
                    .effect_builder
                    .announce_finalized_proto_block(proto_block.clone())
                    .ignore();
                let (results, fb) = self.era_supervisor.handle_finalized_block(
                    era_id,
                    proto_block,
                    new_equivocators,
                    rewards,
                    timestamp,
                    relative_height,
                );
                effects.extend(
                    results
                        .into_iter()
                        .flat_map(|result| self.handle_consensus_result(era_id, result)),
                );
                effects.extend(
                    self.effect_builder
                        .execute_block(fb)
                        .event(move |block| Event::ExecutedBlock { era_id, block }),
                );
                effects
            }
            ConsensusProtocolResult::ValidateConsensusValue(sender, proto_block) => self
                .effect_builder
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
}
