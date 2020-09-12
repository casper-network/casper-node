//! Consensus service is a component that will be communicating with the reactor.
//! It will receive events (like incoming message event or create new message event)
//! and propagate them to the underlying consensus protocol.
//! It tries to know as little as possible about the underlying consensus. The only thing
//! it assumes is the concept of era/epoch and that each era runs separate consensus instance.
//! Most importantly, it doesn't care about what messages it's forwarding.

use std::{
    collections::HashMap,
    fmt::{self, Debug, Formatter},
    rc::Rc,
};

use anyhow::Error;
use casper_types::U512;
use num_traits::AsPrimitive;
use rand::{CryptoRng, Rng};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use casper_execution_engine::shared::motes::Motes;

use crate::{
    components::{
        chainspec_loader::HighwayConfig,
        consensus::{
            consensus_protocol::{
                BlockContext, ConsensusProtocol, ConsensusProtocolResult,
                FinalizedBlock as CpFinalizedBlock,
            },
            highway_core::{highway::Params, validators::Validators},
            protocols::highway::{HighwayContext, HighwayProtocol, HighwaySecret},
            traits::NodeIdT,
            Config, ConsensusMessage, Event, ReactorEventT,
        },
    },
    crypto::{
        asymmetric_key::{self, PublicKey, SecretKey, Signature},
        hash,
    },
    effect::{EffectBuilder, EffectExt, Effects, Responder},
    types::{BlockHeader, FinalizedBlock, ProtoBlock, SystemTransaction, Timestamp},
    utils::WithDir,
};

// We use one trillion as a block reward unit because it's large enough to allow precise
// fractions, and small enough for many block rewards to fit into a u64.
const BLOCK_REWARD: u64 = 1_000_000_000_000;
/// The number of recent eras to retain. Eras older than this are dropped from memory.
// TODO: This needs to be in sync with AUCTION_DELAY/booking_duration_millis. (Already duplicated!)
const RETAIN_ERAS: u64 = 4;

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

pub(crate) struct Era<I, R: Rng + CryptoRng + ?Sized> {
    /// The consensus protocol instance.
    consensus: Box<dyn ConsensusProtocol<I, ProtoBlock, PublicKey, R>>,
    /// The height of this era's first block.
    start_height: u64,
}

pub(crate) struct EraSupervisor<I, R: Rng + CryptoRng + ?Sized> {
    /// A map of active consensus protocols.
    /// A value is a trait so that we can run different consensus protocol instances per era.
    active_eras: HashMap<EraId, Era<I, R>>,
    pub(super) secret_signing_key: Rc<SecretKey>,
    pub(super) public_signing_key: PublicKey,
    validator_stakes: Vec<(PublicKey, Motes)>,
    current_era: EraId,
    highway_config: HighwayConfig,
}

impl<I, R: Rng + CryptoRng + ?Sized> Debug for EraSupervisor<I, R> {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        let ae: Vec<_> = self.active_eras.keys().collect();
        write!(formatter, "EraSupervisor {{ active_eras: {:?}, .. }}", ae)
    }
}

impl<I, R> EraSupervisor<I, R>
where
    I: NodeIdT,
    R: Rng + CryptoRng + ?Sized,
{
    /// Creates a new `EraSupervisor`, starting in era 0.
    pub(crate) fn new<REv: ReactorEventT<I>>(
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

        let results = era_supervisor.new_era(
            EraId(0),
            timestamp,
            validator_stakes,
            highway_config.genesis_era_start_timestamp,
            0,
        );
        let effects = era_supervisor
            .handling_wrapper(effect_builder, rng)
            .handle_consensus_results(EraId(0), results);

        Ok((era_supervisor, effects))
    }

    /// Returns a temporary container with this `EraSupervisor`, `EffectBuilder` and random number
    /// generator, for handling events.
    pub(super) fn handling_wrapper<'a, REv: ReactorEventT<I>>(
        &'a mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &'a mut R,
    ) -> EraSupervisorHandlingWrapper<'a, I, REv, R> {
        EraSupervisorHandlingWrapper {
            era_supervisor: self,
            effect_builder,
            rng,
        }
    }

    /// Starts a new era; panics if it already exists.
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
        self.current_era = era_id;

        let sum_stakes: Motes = validator_stakes.iter().map(|(_, stake)| *stake).sum();
        assert!(
            !sum_stakes.value().is_zero(),
            "cannot start era with total weight 0"
        );
        // For Highway, we need u64 weights. Scale down by  sum / u64::MAX,  rounded up.
        // If we round up the divisor, the resulting sum is guaranteed to be  <= u64::MAX.
        let scaling_factor = (sum_stakes.value() + U512::from(u64::MAX) - 1) / U512::from(u64::MAX);
        let scale_stake = |(key, stake): (PublicKey, Motes)| {
            (key, AsPrimitive::<u64>::as_(stake.value() / scaling_factor))
        };
        let validators: Validators<PublicKey> =
            validator_stakes.into_iter().map(scale_stake).collect();

        let instance_id = hash::hash(format!("Highway era {}", era_id.0));
        let ftt = validators.total_weight()
            * u64::from(self.highway_config.finality_threshold_percent)
            / 100;
        // The number of rounds after which a block reward is paid out.
        // TODO: Make this configurable?
        let reward_delay = 8;
        // TODO: The initial round length should be the observed median of the switch block.
        let params = Params::new(
            0, // TODO: get a proper seed.
            BLOCK_REWARD,
            BLOCK_REWARD / 5, // TODO: Make reduced block reward configurable?
            reward_delay,
            self.highway_config.minimum_round_exponent,
            self.highway_config.minimum_era_height,
            start_time + self.highway_config.era_duration,
        );

        // Activate the era if it is still ongoing based on its minimum duration, and if we are one
        // of the validators.
        let our_id = self.public_signing_key;
        let min_end_time = start_time
            + self
                .highway_config
                .era_duration
                .max(params.min_round_len() * params.end_height());
        let should_activate =
            min_end_time >= timestamp && validators.iter().any(|v| *v.id() == our_id);

        let mut highway =
            HighwayProtocol::<I, HighwayContext>::new(instance_id, validators, params, ftt);

        let results = if should_activate {
            let secret = HighwaySecret::new(Rc::clone(&self.secret_signing_key), our_id);
            highway.activate_validator(our_id, secret, timestamp)
        } else {
            Vec::new()
        };

        let era = Era {
            consensus: Box::new(highway),
            start_height,
        };
        let _ = self.active_eras.insert(era_id, era);

        // Remove the era that has become obsolete now.
        if era_id.0 > RETAIN_ERAS {
            self.active_eras.remove(&EraId(era_id.0 - RETAIN_ERAS - 1));
        }

        results
    }

    /// Returns the current era.
    fn current_era_mut(&mut self) -> &mut Era<I, R> {
        self.active_eras
            .get_mut(&self.current_era)
            .expect("current era does not exist")
    }

    /// Inspect the active eras.
    #[cfg(test)]
    pub(crate) fn active_eras(&self) -> &HashMap<EraId, Era<I, R>> {
        &self.active_eras
    }
}

/// A mutable `EraSupervisor` reference, together with an `EffectBuilder`.
///
/// This is a short-lived convenience type to avoid passing the effect builder through lots of
/// message calls, and making every method individually generic in `REv`. It is only instantiated
/// for the duration of handling a single event.
pub(super) struct EraSupervisorHandlingWrapper<'a, I, REv: 'static, R: Rng + CryptoRng + ?Sized> {
    pub(super) era_supervisor: &'a mut EraSupervisor<I, R>,
    pub(super) effect_builder: EffectBuilder<REv>,
    pub(super) rng: &'a mut R,
}

impl<'a, I, REv, R> EraSupervisorHandlingWrapper<'a, I, REv, R>
where
    I: NodeIdT,
    REv: ReactorEventT<I>,
    R: Rng + CryptoRng + ?Sized,
{
    /// Applies `f` to the consensus protocol of the specified era.
    fn delegate_to_era<F>(&mut self, era_id: EraId, f: F) -> Effects<Event<I>>
    where
        F: FnOnce(
            &mut dyn ConsensusProtocol<I, ProtoBlock, PublicKey, R>,
            &mut R,
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
            Some(era) => match f(&mut *era.consensus, self.rng) {
                Ok(results) => self.handle_consensus_results(era_id, results),
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
        self.delegate_to_era(era_id, move |consensus, rng| {
            consensus.handle_timer(timestamp, rng)
        })
    }

    pub(super) fn handle_message(&mut self, sender: I, msg: ConsensusMessage) -> Effects<Event<I>> {
        let ConsensusMessage { era_id, payload } = msg;
        self.delegate_to_era(era_id, move |consensus, rng| {
            consensus.handle_message(sender, payload, rng)
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
        effects.extend(self.delegate_to_era(era_id, move |consensus, rng| {
            consensus.propose(proto_block, block_context, rng)
        }));
        effects
    }

    pub(super) fn handle_linear_chain_block(
        &mut self,
        block_header: BlockHeader,
        responder: Responder<Signature>,
    ) -> Effects<Event<I>> {
        assert_eq!(
            block_header.era_id(),
            self.era_supervisor.current_era,
            "executed block in unexpected era"
        );
        // TODO - we should only sign if we're a validator for the given era ID.
        let signature = asymmetric_key::sign(
            block_header.hash().inner(),
            &self.era_supervisor.secret_signing_key,
            &self.era_supervisor.public_signing_key,
            self.rng,
        );
        let mut effects = responder.respond(signature).ignore();
        if block_header.switch_block() {
            // TODO: Learn the new weights from contract (validator rotation).
            let validator_stakes = self.era_supervisor.validator_stakes.clone();
            self.era_supervisor
                .current_era_mut()
                .consensus
                .deactivate_validator();
            let new_era_id = block_header.era_id().successor();
            let results = self.era_supervisor.new_era(
                new_era_id,
                Timestamp::now(), // TODO: This should be passed in.
                validator_stakes,
                block_header.timestamp(),
                block_header.height() + 1,
            );
            effects.extend(self.handle_consensus_results(new_era_id, results));
        }
        effects
    }

    pub(super) fn handle_accept_proto_block(
        &mut self,
        era_id: EraId,
        proto_block: ProtoBlock,
    ) -> Effects<Event<I>> {
        let mut effects = self.delegate_to_era(era_id, |consensus, rng| {
            consensus.resolve_validity(&proto_block, true, rng)
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
        self.delegate_to_era(era_id, |consensus, rng| {
            consensus.resolve_validity(&proto_block, false, rng)
        })
    }

    fn handle_consensus_results<T>(&mut self, era_id: EraId, results: T) -> Effects<Event<I>>
    where
        T: IntoIterator<Item = ConsensusProtocolResult<I, ProtoBlock, PublicKey>>,
    {
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
                    .broadcast_message(era_id.message(out_msg).into())
                    .ignore()
            }
            ConsensusProtocolResult::CreatedTargetedMessage(out_msg, to) => self
                .effect_builder
                .send_message(to, era_id.message(out_msg).into())
                .ignore(),
            ConsensusProtocolResult::ScheduleTimer(timestamp) => {
                let timediff = timestamp.saturating_sub(Timestamp::now());
                self.effect_builder
                    .set_timeout(timediff.into())
                    .event(move |_| Event::Timer { era_id, timestamp })
            }
            ConsensusProtocolResult::CreateNewBlock { block_context } => self
                .effect_builder
                .request_proto_block(block_context, self.rng.gen())
                .event(move |(proto_block, block_context)| Event::NewProtoBlock {
                    era_id,
                    proto_block,
                    block_context,
                }),
            ConsensusProtocolResult::FinalizedBlock(CpFinalizedBlock {
                value: proto_block,
                new_equivocators,
                rewards,
                timestamp,
                height,
                terminal,
                proposer,
            }) => {
                // Announce the finalized proto block.
                let mut effects = self
                    .effect_builder
                    .announce_finalized_proto_block(proto_block.clone())
                    .ignore();
                // Create instructions for slashing equivocators.
                let mut system_transactions: Vec<_> = new_equivocators
                    .into_iter()
                    .map(SystemTransaction::Slash)
                    .collect();
                if !rewards.is_empty() {
                    system_transactions.push(SystemTransaction::Rewards(rewards));
                };
                let fb = FinalizedBlock::new(
                    proto_block,
                    timestamp,
                    system_transactions,
                    terminal,
                    era_id,
                    self.era_supervisor.active_eras[&era_id].start_height + height,
                    proposer,
                );
                // Request execution of the finalized block.
                effects.extend(self.effect_builder.execute_block(fb).ignore());
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
