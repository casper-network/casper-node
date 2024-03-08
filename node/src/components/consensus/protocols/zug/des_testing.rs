#![allow(clippy::arithmetic_side_effects)] // In tests, overflows panic anyway.

use std::{
    collections::{hash_map::DefaultHasher, HashMap, VecDeque},
    fmt::{self, Debug, Display, Formatter},
    hash::{Hash, Hasher},
};

use datasize::DataSize;
use hex_fmt::HexFmt;
use itertools::Itertools;
use rand::{prelude::IteratorRandom, Rng};
use serde::{Deserialize, Serialize};
use tracing::{trace, warn};

use casper_types::{TimeDiff, Timestamp};

use super::{
    config::Config,
    message::{Content, Message as ZugProtocolMessage, SignedMessage},
    Params, Zug,
};
use crate::{
    components::consensus::{
        consensus_protocol::{
            ConsensusProtocol, FinalizedBlock, ProposedBlock, ProtocolOutcome, ProtocolOutcomes,
        },
        tests::{
            consensus_des_testing::{
                DeliverySchedule, Fault as DesFault, Message, Node, Target, TargetedMessage,
                ValidatorId, VirtualNet,
            },
            queue::QueueEntry,
        },
        traits::{ConsensusValueT, Context, ValidatorSecret},
        utils::{Validators, Weight},
        ActionId, BlockContext, SerializedMessage, TimerId,
    },
    types::NodeId,
    NodeRng,
};

#[derive(Eq, PartialEq, Clone, Debug, Hash, Serialize, Deserialize, DataSize, Default)]
pub(crate) struct ConsensusValue(Vec<u8>);

impl ConsensusValueT for ConsensusValue {
    fn needs_validation(&self) -> bool {
        !self.0.is_empty()
    }
}

impl Display for ConsensusValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:10}", HexFmt(&self.0))
    }
}

const TEST_MIN_ROUND_LEN: TimeDiff = TimeDiff::from_millis(1 << 12);
const TEST_END_HEIGHT: u64 = 100000;
pub(crate) const TEST_INSTANCE_ID: u64 = 42;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
enum ZugMessage {
    GossipMessage(SerializedMessage),
    TargetedMessage(SerializedMessage, NodeId),
    MessageToRandomPeer(SerializedMessage),
    RequestToRandomPeer(SerializedMessage),
    Timer(Timestamp, TimerId),
    QueueAction(ActionId),
    RequestNewBlock(BlockContext<TestContext>),
    FinalizedBlock(FinalizedBlock<TestContext>),
    ValidateConsensusValue(NodeId, ProposedBlock<TestContext>),
    NewEvidence(ValidatorId),
    SendEvidence(NodeId, ValidatorId),
    WeAreFaulty,
    DoppelgangerDetected,
    FttExceeded,
    Disconnect(NodeId),
    HandledProposedBlock(ProposedBlock<TestContext>),
}

impl ZugMessage {
    fn is_signed_gossip_message(&self) -> bool {
        if let ZugMessage::GossipMessage(raw) = self {
            let deserialized: super::Message<TestContext> =
                raw.deserialize_incoming().expect("message not valid");
            matches!(deserialized, ZugProtocolMessage::Signed(_))
        } else {
            false
        }
    }

    fn is_proposal(&self) -> bool {
        if let ZugMessage::GossipMessage(raw) = self {
            let deserialized: super::Message<TestContext> =
                raw.deserialize_incoming().expect("message not valid");
            matches!(deserialized, ZugProtocolMessage::Proposal { .. })
        } else {
            false
        }
    }
}

impl PartialOrd for ZugMessage {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ZugMessage {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let mut hasher0 = DefaultHasher::new();
        let mut hasher1 = DefaultHasher::new();
        self.hash(&mut hasher0);
        other.hash(&mut hasher1);
        hasher0.finish().cmp(&hasher1.finish())
    }
}

impl From<ProtocolOutcome<TestContext>> for ZugMessage {
    fn from(outcome: ProtocolOutcome<TestContext>) -> ZugMessage {
        match outcome {
            ProtocolOutcome::CreatedGossipMessage(msg) => ZugMessage::GossipMessage(msg),
            ProtocolOutcome::CreatedTargetedMessage(msg, target) => {
                ZugMessage::TargetedMessage(msg, target)
            }
            ProtocolOutcome::CreatedMessageToRandomPeer(msg) => {
                ZugMessage::MessageToRandomPeer(msg)
            }
            ProtocolOutcome::CreatedRequestToRandomPeer(request) => {
                ZugMessage::RequestToRandomPeer(request)
            }
            ProtocolOutcome::ScheduleTimer(timestamp, timer_id) => {
                ZugMessage::Timer(timestamp, timer_id)
            }
            ProtocolOutcome::QueueAction(action_id) => ZugMessage::QueueAction(action_id),
            ProtocolOutcome::CreateNewBlock(block_ctx, _expiry) => {
                ZugMessage::RequestNewBlock(block_ctx)
            }
            ProtocolOutcome::FinalizedBlock(finalized_block) => {
                ZugMessage::FinalizedBlock(finalized_block)
            }
            ProtocolOutcome::ValidateConsensusValue {
                sender,
                proposed_block,
            } => ZugMessage::ValidateConsensusValue(sender, proposed_block),
            ProtocolOutcome::NewEvidence(vid) => ZugMessage::NewEvidence(vid),
            ProtocolOutcome::SendEvidence(target, vid) => ZugMessage::SendEvidence(target, vid),
            ProtocolOutcome::WeAreFaulty => ZugMessage::WeAreFaulty,
            ProtocolOutcome::DoppelgangerDetected => ZugMessage::DoppelgangerDetected,
            ProtocolOutcome::FttExceeded => ZugMessage::FttExceeded,
            ProtocolOutcome::Disconnect(sender) => ZugMessage::Disconnect(sender),
            ProtocolOutcome::HandledProposedBlock(proposed_block) => {
                ZugMessage::HandledProposedBlock(proposed_block)
            }
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum TestRunError {
    /// VirtualNet was missing a validator when it was expected to exist.
    MissingValidator(ValidatorId),
    /// No more messages in the message queue.
    NoMessages,
}

impl Display for TestRunError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TestRunError::NoMessages => write!(
                f,
                "Test finished prematurely due to lack of messages in the queue"
            ),
            TestRunError::MissingValidator(id) => {
                write!(f, "Virtual net is missing validator {:?}.", id)
            }
        }
    }
}

enum Distribution {
    Uniform,
}

impl Distribution {
    /// Returns vector of `count` elements of random values between `lower` and `upper`.
    fn gen_range_vec(&self, rng: &mut NodeRng, lower: u64, upper: u64, count: u8) -> Vec<u64> {
        match self {
            Distribution::Uniform => (0..count).map(|_| rng.gen_range(lower..upper)).collect(),
        }
    }
}

trait DeliveryStrategy {
    fn gen_delay(
        &mut self,
        rng: &mut NodeRng,
        message: &ZugMessage,
        distribution: &Distribution,
        base_delivery_timestamp: Timestamp,
    ) -> DeliverySchedule;
}

struct ZugValidator {
    zug: Zug<TestContext>,
    fault: Option<DesFault>,
}

impl ZugValidator {
    fn new(zug: Zug<TestContext>, fault: Option<DesFault>) -> Self {
        ZugValidator { zug, fault }
    }

    fn zug_mut(&mut self) -> &mut Zug<TestContext> {
        &mut self.zug
    }

    fn zug(&self) -> &Zug<TestContext> {
        &self.zug
    }

    fn post_hook(&mut self, delivery_time: Timestamp, msg: ZugMessage) -> Vec<ZugMessage> {
        match self.fault.as_ref() {
            Some(DesFault::TemporarilyMute { from, till })
                if *from <= delivery_time && delivery_time <= *till =>
            {
                // For mute validators we drop the generated messages to be sent, if the delivery
                // time is in the interval in which they are muted.
                match msg {
                    ZugMessage::GossipMessage(_)
                    | ZugMessage::TargetedMessage(_, _)
                    | ZugMessage::MessageToRandomPeer(_)
                    | ZugMessage::RequestToRandomPeer(_)
                    | ZugMessage::SendEvidence(_, _) => {
                        warn!("Validator is mute – won't send messages in response");
                        vec![]
                    }
                    ZugMessage::Timer(_, _)
                    | ZugMessage::QueueAction(_)
                    | ZugMessage::RequestNewBlock(_)
                    | ZugMessage::FinalizedBlock(_)
                    | ZugMessage::ValidateConsensusValue(_, _)
                    | ZugMessage::NewEvidence(_)
                    | ZugMessage::Disconnect(_)
                    | ZugMessage::HandledProposedBlock(_) => vec![msg],
                    ZugMessage::WeAreFaulty => {
                        panic!("validator equivocated unexpectedly");
                    }
                    ZugMessage::DoppelgangerDetected => {
                        panic!("unexpected doppelganger detected");
                    }
                    ZugMessage::FttExceeded => {
                        panic!("unexpected FTT exceeded");
                    }
                }
            }
            Some(DesFault::PermanentlyMute) => {
                // For permanently mute validators we drop the generated messages to be sent
                match msg {
                    ZugMessage::GossipMessage(_)
                    | ZugMessage::TargetedMessage(_, _)
                    | ZugMessage::MessageToRandomPeer(_)
                    | ZugMessage::RequestToRandomPeer(_)
                    | ZugMessage::SendEvidence(_, _) => {
                        warn!("Validator is mute – won't send messages in response");
                        vec![]
                    }
                    ZugMessage::Timer(_, _)
                    | ZugMessage::QueueAction(_)
                    | ZugMessage::RequestNewBlock(_)
                    | ZugMessage::FinalizedBlock(_)
                    | ZugMessage::ValidateConsensusValue(_, _)
                    | ZugMessage::NewEvidence(_)
                    | ZugMessage::Disconnect(_)
                    | ZugMessage::HandledProposedBlock(_) => vec![msg],
                    ZugMessage::WeAreFaulty => {
                        panic!("validator equivocated unexpectedly");
                    }
                    ZugMessage::DoppelgangerDetected => {
                        panic!("unexpected doppelganger detected");
                    }
                    ZugMessage::FttExceeded => {
                        panic!("unexpected FTT exceeded");
                    }
                }
            }
            None | Some(DesFault::TemporarilyMute { .. }) => {
                // Honest validator.
                match &msg {
                    ZugMessage::WeAreFaulty => {
                        panic!("validator equivocated unexpectedly");
                    }
                    ZugMessage::DoppelgangerDetected => {
                        panic!("unexpected doppelganger detected");
                    }
                    ZugMessage::FttExceeded => {
                        panic!("unexpected FTT exceeded");
                    }
                    _ => vec![msg],
                }
            }
            Some(DesFault::Equivocate) => match msg {
                ZugMessage::GossipMessage(ref serialized_msg) => {
                    match serialized_msg.deserialize_incoming::<ZugProtocolMessage<TestContext>>() {
                        Ok(ZugProtocolMessage::Signed(
                            signed_msg @ SignedMessage { content, .. },
                        )) => match content {
                            Content::Echo(hash) => {
                                let conflicting_message = SignedMessage::sign_new(
                                    signed_msg.round_id,
                                    signed_msg.instance_id,
                                    Content::<TestContext>::Echo(HashWrapper(
                                        hash.0.wrapping_add(1),
                                    )),
                                    signed_msg.validator_idx,
                                    &TestSecret(signed_msg.validator_idx.0.into()),
                                );
                                vec![
                                    ZugMessage::GossipMessage(SerializedMessage::from_message(
                                        &ZugProtocolMessage::Signed(conflicting_message),
                                    )),
                                    msg,
                                ]
                            }
                            Content::Vote(vote) => {
                                let conflicting_message = SignedMessage::sign_new(
                                    signed_msg.round_id,
                                    signed_msg.instance_id,
                                    Content::<TestContext>::Vote(!vote),
                                    signed_msg.validator_idx,
                                    &TestSecret(signed_msg.validator_idx.0.into()),
                                );
                                vec![
                                    ZugMessage::GossipMessage(SerializedMessage::from_message(
                                        &ZugProtocolMessage::Signed(conflicting_message),
                                    )),
                                    msg,
                                ]
                            }
                        },
                        _ => vec![msg],
                    }
                }
                _ => vec![msg],
            },
        }
    }
}

type ZugNode = Node<ConsensusValue, ZugMessage, ZugValidator>;

type ZugNet = VirtualNet<ConsensusValue, ZugMessage, ZugValidator>;

struct ZugTestHarness<DS>
where
    DS: DeliveryStrategy,
{
    virtual_net: ZugNet,
    /// Consensus values to be proposed.
    /// Order of values in the vector defines the order in which they will be proposed.
    consensus_values: VecDeque<ConsensusValue>,
    /// A strategy to pseudo randomly change the message delivery times.
    delivery_time_strategy: DS,
    /// Distribution of delivery times.
    delivery_time_distribution: Distribution,
    /// Mapping of validator IDs to node IDs
    vid_to_node_id: HashMap<ValidatorId, NodeId>,
    /// Mapping of node IDs to validator IDs
    node_id_to_vid: HashMap<NodeId, ValidatorId>,
}

type TestResult<T> = Result<T, TestRunError>;

impl<DS> ZugTestHarness<DS>
where
    DS: DeliveryStrategy,
{
    /// Advance the test by one message.
    ///
    /// Pops one message from the message queue (if there are any)
    /// and pass it to the recipient validator for execution.
    /// Messages returned from the execution are scheduled for later delivery.
    pub(crate) fn crank(&mut self, rng: &mut NodeRng) -> TestResult<()> {
        let QueueEntry {
            delivery_time,
            recipient,
            message,
        } = self
            .virtual_net
            .pop_message()
            .ok_or(TestRunError::NoMessages)?;

        let span = tracing::trace_span!("crank", validator = %recipient);
        let _enter = span.enter();
        trace!(
            "Processing: tick {}, sender validator={}, payload {:?}",
            delivery_time,
            message.sender,
            message.payload(),
        );

        let messages = self.process_message(rng, recipient, message, delivery_time)?;

        let targeted_messages = messages
            .into_iter()
            .filter_map(|zm| {
                let delivery = self.delivery_time_strategy.gen_delay(
                    rng,
                    &zm,
                    &self.delivery_time_distribution,
                    delivery_time,
                );
                match delivery {
                    DeliverySchedule::Drop => {
                        trace!("{:?} message is dropped.", zm);
                        None
                    }
                    DeliverySchedule::AtInstant(timestamp) => {
                        trace!("{:?} scheduled for {:?}", zm, timestamp);
                        self.convert_into_targeted(zm, recipient, rng)
                            .map(|targeted| (targeted, timestamp))
                    }
                }
            })
            .collect();

        self.virtual_net.dispatch_messages(targeted_messages);
        Ok(())
    }

    fn convert_into_targeted(
        &self,
        zm: ZugMessage,
        creator: ValidatorId,
        rng: &mut NodeRng,
    ) -> Option<TargetedMessage<ZugMessage>> {
        let create_msg = |zm: ZugMessage| Message::new(creator, zm);

        match zm {
            ZugMessage::GossipMessage(_) => Some(TargetedMessage::new(
                create_msg(zm),
                Target::AllExcept(creator),
            )),
            ZugMessage::TargetedMessage(_, target) => self
                .node_id_to_vid
                .get(&target)
                .map(|vid| TargetedMessage::new(create_msg(zm), Target::SingleValidator(*vid))),
            ZugMessage::MessageToRandomPeer(_) | ZugMessage::RequestToRandomPeer(_) => self
                .virtual_net
                .validators_ids()
                .choose(rng)
                .map(|random_vid| {
                    TargetedMessage::new(create_msg(zm), Target::SingleValidator(*random_vid))
                }),
            ZugMessage::Timer(_, _)
            | ZugMessage::QueueAction(_)
            | ZugMessage::RequestNewBlock(_)
            | ZugMessage::FinalizedBlock(_)
            | ZugMessage::ValidateConsensusValue(_, _)
            | ZugMessage::NewEvidence(_)
            | ZugMessage::Disconnect(_)
            | ZugMessage::HandledProposedBlock(_)
            | ZugMessage::SendEvidence(_, _)
            | ZugMessage::WeAreFaulty
            | ZugMessage::DoppelgangerDetected
            | ZugMessage::FttExceeded => Some(TargetedMessage::new(
                create_msg(zm),
                Target::SingleValidator(creator),
            )),
        }
    }

    fn next_consensus_value(&mut self, height: u64) -> ConsensusValue {
        self.consensus_values
            .get(height as usize)
            .cloned()
            .unwrap_or_default()
    }

    /// Helper for getting validator from the underlying virtual net.
    fn node_mut(&mut self, validator_id: &ValidatorId) -> TestResult<&mut ZugNode> {
        self.virtual_net
            .node_mut(validator_id)
            .ok_or(TestRunError::MissingValidator(*validator_id))
    }

    fn call_validator<F>(
        &mut self,
        delivery_time: Timestamp,
        validator_id: &ValidatorId,
        f: F,
    ) -> TestResult<Vec<ZugMessage>>
    where
        F: FnOnce(&mut ZugValidator) -> ProtocolOutcomes<TestContext>,
    {
        let validator_node = self.node_mut(validator_id)?;
        let res = f(validator_node.validator_mut());
        let messages = res
            .into_iter()
            .flat_map(|outcome| {
                validator_node
                    .validator_mut()
                    .post_hook(delivery_time, ZugMessage::from(outcome))
            })
            .collect();
        Ok(messages)
    }

    /// Processes a message sent to `validator_id`.
    /// Returns a vector of messages produced by the `validator` in reaction to processing a
    /// message.
    fn process_message(
        &mut self,
        rng: &mut NodeRng,
        validator_id: ValidatorId,
        message: Message<ZugMessage>,
        delivery_time: Timestamp,
    ) -> TestResult<Vec<ZugMessage>> {
        self.node_mut(&validator_id)?
            .push_messages_received(vec![message.clone()]);

        let messages = {
            let sender_id = message.sender;

            let zm = message.payload().clone();

            match zm {
                ZugMessage::GossipMessage(msg)
                | ZugMessage::TargetedMessage(msg, _)
                | ZugMessage::MessageToRandomPeer(msg) => {
                    let sender = *self
                        .vid_to_node_id
                        .get(&sender_id)
                        .ok_or(TestRunError::MissingValidator(sender_id))?;
                    self.call_validator(delivery_time, &validator_id, |consensus| {
                        consensus
                            .zug_mut()
                            .handle_message(rng, sender, msg, delivery_time)
                    })?
                }
                ZugMessage::RequestToRandomPeer(req) => {
                    let sender = *self
                        .vid_to_node_id
                        .get(&sender_id)
                        .ok_or(TestRunError::MissingValidator(sender_id))?;
                    self.call_validator(delivery_time, &validator_id, |consensus| {
                        let (mut outcomes, maybe_msg) = consensus.zug_mut().handle_request_message(
                            rng,
                            sender,
                            req,
                            delivery_time,
                        );
                        outcomes.extend(
                            maybe_msg
                                .into_iter()
                                .map(|msg| ProtocolOutcome::CreatedTargetedMessage(msg, sender)),
                        );
                        outcomes
                    })?
                }
                ZugMessage::Timer(timestamp, timer_id) => {
                    self.call_validator(delivery_time, &validator_id, |consensus| {
                        consensus
                            .zug_mut()
                            .handle_timer(timestamp, delivery_time, timer_id, rng)
                    })?
                }
                ZugMessage::QueueAction(_) => vec![], // not used in Zug
                ZugMessage::RequestNewBlock(block_context) => {
                    let consensus_value = self.next_consensus_value(block_context.height());
                    let proposed_block = ProposedBlock::new(consensus_value, block_context);

                    self.call_validator(delivery_time, &validator_id, |consensus| {
                        consensus.zug_mut().propose(proposed_block, delivery_time)
                    })?
                }
                ZugMessage::FinalizedBlock(FinalizedBlock {
                    value,
                    timestamp: _,
                    relative_height,
                    terminal_block_data,
                    equivocators: _,
                    proposer: _,
                }) => {
                    trace!(
                        "{}consensus value finalized: {:?}, height: {:?}",
                        if terminal_block_data.is_some() {
                            "last "
                        } else {
                            ""
                        },
                        value,
                        relative_height,
                    );
                    if let Some(t) = terminal_block_data {
                        warn!(?t.rewards, "rewards and inactive validators are not verified yet");
                    }
                    self.node_mut(&validator_id)?.push_finalized(value);
                    vec![]
                }
                ZugMessage::ValidateConsensusValue(_, proposed_block) => {
                    self.call_validator(delivery_time, &validator_id, |consensus| {
                        consensus
                            .zug_mut()
                            .resolve_validity(proposed_block, true, delivery_time)
                    })?
                }
                ZugMessage::NewEvidence(_) => vec![], // irrelevant to consensus
                ZugMessage::Disconnect(target) => {
                    if let Some(vid) = self.node_id_to_vid.get(&target) {
                        warn!("{} wants to disconnect from {}", validator_id, vid);
                    }
                    vec![] // TODO: register the disconnect attempt somehow?
                }
                ZugMessage::HandledProposedBlock(_) => vec![], // irrelevant to consensus
                ZugMessage::WeAreFaulty => {
                    warn!("{} detected that it is faulty", validator_id);
                    vec![] // TODO: stop the node or something?
                }
                ZugMessage::DoppelgangerDetected => {
                    warn!("{} detected a doppelganger", validator_id);
                    vec![] // TODO: stop the node or something?
                }
                ZugMessage::FttExceeded => {
                    warn!("{} detected FTT exceeded", validator_id);
                    vec![] // TODO: stop the node or something?
                }
                ZugMessage::SendEvidence(node_id, vid) => {
                    self.call_validator(delivery_time, &validator_id, |consensus| {
                        consensus.zug_mut().send_evidence(node_id, &vid)
                    })?
                }
            }
        };

        let recipient = self.node_mut(&validator_id)?;
        recipient.push_messages_produced(messages.clone());

        Ok(messages)
    }

    /// Returns a `MutableHandle` on the `ZugTestHarness` object
    /// that allows for manipulating internal state of the test state.
    fn mutable_handle(&mut self) -> MutableHandle<DS> {
        MutableHandle(self)
    }
}

fn crank_until<F, DS: DeliveryStrategy>(
    zth: &mut ZugTestHarness<DS>,
    rng: &mut NodeRng,
    f: F,
) -> TestResult<()>
where
    F: Fn(&ZugTestHarness<DS>) -> bool,
{
    while !f(zth) {
        zth.crank(rng)?;
    }
    Ok(())
}

struct MutableHandle<'a, DS: DeliveryStrategy>(&'a mut ZugTestHarness<DS>);

impl<'a, DS: DeliveryStrategy> MutableHandle<'a, DS> {
    /// Drops all messages from the queue.
    fn clear_message_queue(&mut self) {
        self.0.virtual_net.empty_queue();
    }

    fn validators(&self) -> impl Iterator<Item = &ZugNode> {
        self.0.virtual_net.validators()
    }
}

#[derive(Debug)]
enum BuilderError {
    WeightLimits,
}

struct ZugTestHarnessBuilder<DS: DeliveryStrategy> {
    /// Maximum number of faulty validators in the network.
    /// Defaults to 10.
    max_faulty_validators: u8,
    /// Percentage of faulty validators' (i.e. equivocators) weight.
    /// Defaults to 0 (network is perfectly secure).
    faulty_percent: u64,
    fault_type: Option<DesFault>,
    /// FTT value for the finality detector.
    /// If not given, defaults to 1/3 of total validators' weight.
    ftt: Option<u64>,
    /// Number of consensus values to be proposed by the nodes in the network.
    /// Those will be generated by the test framework.
    /// Defaults to 10.
    consensus_values_count: u8,
    /// Distribution of message delivery (delaying, dropping) delays..
    delivery_distribution: Distribution,
    delivery_strategy: DS,
    /// Upper and lower limits for validators' weights.
    weight_limits: (u64, u64),
    /// Time when the test era starts at.
    /// Defaults to 0.
    start_time: Timestamp,
    /// Era end height.
    end_height: u64,
    /// Type of discrete distribution of validators' weights.
    /// Defaults to uniform.
    weight_distribution: Distribution,
    /// Zug protocol config
    config: Config,
}

// Default strategy for message delivery.
struct InstantDeliveryNoDropping;

impl DeliveryStrategy for InstantDeliveryNoDropping {
    fn gen_delay(
        &mut self,
        _rng: &mut NodeRng,
        message: &ZugMessage,
        _distribution: &Distribution,
        base_delivery_timestamp: Timestamp,
    ) -> DeliverySchedule {
        match message {
            ZugMessage::RequestNewBlock(bc) => DeliverySchedule::AtInstant(bc.timestamp()),
            ZugMessage::Timer(t, _) => DeliverySchedule::AtInstant(*t),
            ZugMessage::GossipMessage(_)
            | ZugMessage::TargetedMessage(_, _)
            | ZugMessage::MessageToRandomPeer(_)
            | ZugMessage::RequestToRandomPeer(_)
            | ZugMessage::QueueAction(_)
            | ZugMessage::FinalizedBlock(_)
            | ZugMessage::ValidateConsensusValue(_, _)
            | ZugMessage::NewEvidence(_)
            | ZugMessage::Disconnect(_)
            | ZugMessage::HandledProposedBlock(_)
            | ZugMessage::WeAreFaulty
            | ZugMessage::DoppelgangerDetected
            | ZugMessage::FttExceeded
            | ZugMessage::SendEvidence(_, _) => {
                DeliverySchedule::AtInstant(base_delivery_timestamp + TimeDiff::from_millis(1))
            }
        }
    }
}

impl ZugTestHarnessBuilder<InstantDeliveryNoDropping> {
    fn new() -> Self {
        ZugTestHarnessBuilder {
            max_faulty_validators: 10,
            faulty_percent: 0,
            fault_type: None,
            ftt: None,
            consensus_values_count: 10,
            delivery_distribution: Distribution::Uniform,
            delivery_strategy: InstantDeliveryNoDropping,
            weight_limits: (1, 100),
            start_time: Timestamp::zero(),
            end_height: TEST_END_HEIGHT,
            weight_distribution: Distribution::Uniform,
            config: Default::default(),
        }
    }
}

impl<DS: DeliveryStrategy> ZugTestHarnessBuilder<DS> {
    /// Sets a percentage of weight that will be assigned to malicious nodes.
    /// `faulty_weight` must be a value between 0 (inclusive) and 33 (inclusive).
    pub(crate) fn faulty_weight_perc(mut self, faulty_weight: u64) -> Self {
        self.faulty_percent = faulty_weight;
        self
    }

    fn fault_type(mut self, fault_type: DesFault) -> Self {
        self.fault_type = Some(fault_type);
        self
    }

    pub(crate) fn consensus_values_count(mut self, count: u8) -> Self {
        assert!(count > 0);
        self.consensus_values_count = count;
        self
    }

    pub(crate) fn weight_limits(mut self, lower: u64, upper: u64) -> Self {
        assert!(
            lower >= 100,
            "Lower limit has to be higher than 100 to avoid rounding problems."
        );
        self.weight_limits = (lower, upper);
        self
    }

    fn max_faulty_validators(mut self, max_faulty_count: u8) -> Self {
        self.max_faulty_validators = max_faulty_count;
        self
    }

    fn build(self, rng: &mut NodeRng) -> Result<ZugTestHarness<DS>, BuilderError> {
        let consensus_values = (0..self.consensus_values_count)
            .map(|el| ConsensusValue(vec![el]))
            .collect::<VecDeque<ConsensusValue>>();

        let instance_id = TEST_INSTANCE_ID;
        let start_time = self.start_time;

        let (lower, upper) = {
            let (l, u) = self.weight_limits;
            if l >= u {
                return Err(BuilderError::WeightLimits);
            }
            (l, u)
        };

        let (faulty_weights, honest_weights): (Vec<Weight>, Vec<Weight>) = {
            if self.faulty_percent == 0 {
                // All validators are honest.
                let validators_num = rng.gen_range(2..self.max_faulty_validators + 1);
                let honest_validators: Vec<Weight> = self
                    .weight_distribution
                    .gen_range_vec(rng, lower, upper, validators_num)
                    .into_iter()
                    .map(Weight)
                    .collect();

                (vec![], honest_validators)
            } else {
                // At least 2 validators total and at least one faulty.
                let faulty_num = rng.gen_range(1..self.max_faulty_validators + 1);

                // Randomly (but within chosen range) assign weights to faulty nodes.
                let faulty_weights = self
                    .weight_distribution
                    .gen_range_vec(rng, lower, upper, faulty_num);

                // Assign enough weights to honest nodes so that we reach expected
                // `faulty_percentage` ratio.
                let honest_weights = {
                    let faulty_sum = faulty_weights.iter().sum::<u64>();
                    let mut weights_to_distribute: u64 =
                        (faulty_sum * 100 + self.faulty_percent - 1) / self.faulty_percent
                            - faulty_sum;
                    let mut weights = vec![];
                    while weights_to_distribute > 0 {
                        let weight = if weights_to_distribute < upper {
                            weights_to_distribute
                        } else {
                            rng.gen_range(lower..upper)
                        };
                        weights.push(weight);
                        weights_to_distribute -= weight
                    }
                    weights
                };

                (
                    faulty_weights.into_iter().map(Weight).collect(),
                    honest_weights.into_iter().map(Weight).collect(),
                )
            }
        };

        let weights_sum = faulty_weights
            .iter()
            .chain(honest_weights.iter())
            .sum::<Weight>();

        let validators: Validators<ValidatorId> = faulty_weights
            .iter()
            .chain(honest_weights.iter())
            .enumerate()
            .map(|(i, weight)| (ValidatorId(i as u64), *weight))
            .collect();

        trace!("Weights: {:?}", validators.iter().collect::<Vec<_>>());

        let mut secrets = validators
            .iter()
            .map(|validator| (*validator.id(), TestSecret(validator.id().0)))
            .collect();

        let ftt = self
            .ftt
            .map(|p| p * weights_sum.0 / 100)
            .unwrap_or_else(|| (weights_sum.0 - 1) / 3);

        let params = Params::new(
            instance_id,
            TEST_MIN_ROUND_LEN,
            start_time,
            self.end_height,
            start_time, // Length depends only on block number.
            ftt.into(),
        );

        // Local function creating an instance of `ZugConsensus` for a single validator.
        let zug_consensus =
            |(vid, secrets): (ValidatorId, &mut HashMap<ValidatorId, TestSecret>)| {
                let v_sec = secrets.remove(&vid).expect("Secret key should exist.");

                let mut zug = Zug::new_with_params(
                    validators.clone(),
                    params.clone(),
                    &self.config,
                    None,
                    0, // random seed
                );
                let tmpdir = tempfile::tempdir().expect("could not create tempdir");
                let wal_file = tmpdir.path().join("wal_file.dat");
                let effects = zug.activate_validator(vid, v_sec, start_time, Some(wal_file));

                (zug, effects.into_iter().map(ZugMessage::from).collect_vec())
            };

        let faulty_num = faulty_weights.len();

        let (validators, init_messages) = {
            let mut validators_loc = vec![];
            let mut init_messages = vec![];

            for validator in validators.iter() {
                let vid = *validator.id();
                let fault = if vid.0 < faulty_num as u64 {
                    self.fault_type
                } else {
                    None
                };
                let (zug, msgs) = zug_consensus((vid, &mut secrets));
                let zug_consensus = ZugValidator::new(zug, fault);
                let validator = Node::new(vid, zug_consensus);
                let qm: Vec<QueueEntry<ZugMessage>> = msgs
                    .into_iter()
                    .map(|zm| {
                        // These are messages crated on the start of the network.
                        // They are sent from validator to himself.
                        QueueEntry::new(start_time, vid, Message::new(vid, zm))
                    })
                    .collect();
                init_messages.extend(qm);
                validators_loc.push(validator);
            }

            (validators_loc, init_messages)
        };

        let delivery_time_strategy = self.delivery_strategy;

        let delivery_time_distribution = self.delivery_distribution;

        let vid_to_node_id: HashMap<_, _> = validators
            .iter()
            .map(|validator| (validator.id, NodeId::random(rng)))
            .collect();

        let node_id_to_vid: HashMap<_, _> = vid_to_node_id
            .iter()
            .map(|(vid, node_id)| (*node_id, *vid))
            .collect();

        let virtual_net = VirtualNet::new(validators, init_messages);

        let zth = ZugTestHarness {
            virtual_net,
            consensus_values,
            delivery_time_strategy,
            delivery_time_distribution,
            vid_to_node_id,
            node_id_to_vid,
        };

        Ok(zth)
    }
}

#[derive(Clone, DataSize, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct TestContext;

#[derive(Clone, DataSize, Debug, Eq, PartialEq)]
pub(crate) struct TestSecret(pub(crate) u64);

// Newtype wrapper for test signature.
// Added so that we can use custom Debug impl.
#[derive(Clone, DataSize, Copy, Hash, PartialOrd, Ord, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct SignatureWrapper(u64);

impl Debug for SignatureWrapper {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:10}", HexFmt(&self.0.to_le_bytes()))
    }
}

// Newtype wrapper for test hash.
// Added so that we can use custom Debug impl.
#[derive(Clone, Copy, DataSize, Hash, Ord, PartialOrd, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct HashWrapper(u64);

impl Debug for HashWrapper {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:10}", HexFmt(&self.0.to_le_bytes()))
    }
}

impl Display for HashWrapper {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(self, f)
    }
}

impl ValidatorSecret for TestSecret {
    type Hash = HashWrapper;
    type Signature = SignatureWrapper;

    fn sign(&self, data: &Self::Hash) -> Self::Signature {
        SignatureWrapper(data.0 + self.0)
    }
}

impl Context for TestContext {
    type ConsensusValue = ConsensusValue;
    type ValidatorId = ValidatorId;
    type ValidatorSecret = TestSecret;
    type Signature = SignatureWrapper;
    type Hash = HashWrapper;
    type InstanceId = u64;

    fn hash(data: &[u8]) -> Self::Hash {
        let mut hasher = DefaultHasher::new();
        hasher.write(data);
        HashWrapper(hasher.finish())
    }

    fn verify_signature(
        hash: &Self::Hash,
        public_key: &Self::ValidatorId,
        signature: &<Self::ValidatorSecret as ValidatorSecret>::Signature,
    ) -> bool {
        let computed_signature = hash.0 + public_key.0;
        computed_signature == signature.0
    }
}

mod test_harness {
    use std::{collections::HashSet, fmt::Debug};

    use super::{
        crank_until, ConsensusValue, InstantDeliveryNoDropping, TestRunError, ZugTestHarness,
        ZugTestHarnessBuilder,
    };
    use crate::{
        components::consensus::{
            consensus_protocol::ConsensusProtocol,
            tests::consensus_des_testing::{Fault as DesFault, ValidatorId},
        },
        logging,
    };
    use logging::{LoggingConfig, LoggingFormat};

    #[test]
    fn on_empty_queue_error() {
        let mut rng = crate::new_rng();
        let mut zug_test_harness: ZugTestHarness<InstantDeliveryNoDropping> =
            ZugTestHarnessBuilder::new()
                .consensus_values_count(1)
                .weight_limits(100, 120)
                .build(&mut rng)
                .expect("Construction was successful");

        zug_test_harness.mutable_handle().clear_message_queue();

        assert_eq!(
            zug_test_harness.crank(&mut rng),
            Err(TestRunError::NoMessages),
            "Expected the test run to stop."
        );
    }

    // Test that all elements of the vector all equal.
    fn assert_eq_vectors<I: Eq + Debug>(coll: Vec<I>, error_msg: &str) {
        let mut iter = coll.into_iter();
        let reference = iter.next().unwrap();

        iter.for_each(|v| assert_eq!(v, reference, "{}", error_msg));
    }

    #[test]
    fn liveness_test_no_faults() {
        let _ = logging::init_with_config(&LoggingConfig::new(LoggingFormat::Text, true, true));

        let mut rng = crate::new_rng();
        let cv_count = 10;

        let mut zug_test_harness = ZugTestHarnessBuilder::new()
            .max_faulty_validators(3)
            .consensus_values_count(cv_count)
            .weight_limits(100, 120)
            .build(&mut rng)
            .expect("Construction was successful");

        crank_until(&mut zug_test_harness, &mut rng, |zth| {
            // Stop the test when each node finalized expected number of consensus values.
            // Note that we're not testing the order of finalization here.
            // It will be tested later – it's not the condition for stopping the test run.
            zth.virtual_net
                .validators()
                .all(|v| v.finalized_count() == cv_count as usize)
        })
        .unwrap();

        let handle = zug_test_harness.mutable_handle();
        let validators = handle.validators();

        let (finalized_values, msgs_produced): (Vec<Vec<ConsensusValue>>, Vec<usize>) = validators
            .map(|v| {
                (
                    v.finalized_values().cloned().collect::<Vec<_>>(),
                    v.messages_produced()
                        .cloned()
                        .filter(|zm| zm.is_signed_gossip_message() || zm.is_proposal())
                        .count(),
                )
            })
            .unzip();

        msgs_produced
            .into_iter()
            .enumerate()
            .for_each(|(v_idx, units_count)| {
                // NOTE: Works only when all validators are honest and correct (no "mute"
                // validators). Validator produces two units per round. It may
                // produce just one before lambda message is finalized. Add one in case it's just
                // one round (one consensus value) – 1 message. 1/2=0 but 3/2=1 b/c of the rounding.
                let expected_msgs = cv_count as usize * 2;

                assert_eq!(
                    units_count, expected_msgs,
                    "Expected that validator={} produced {} messages.",
                    v_idx, expected_msgs
                )
            });

        assert_eq_vectors(
            finalized_values,
            "Nodes finalized different consensus values.",
        );
    }

    #[test]
    fn liveness_test_some_mute() {
        let _ = logging::init_with_config(&LoggingConfig::new(LoggingFormat::Text, true, true));

        let mut rng = crate::new_rng();
        let cv_count = 10;
        let fault_perc = 30;

        let mut zug_test_harness = ZugTestHarnessBuilder::new()
            .max_faulty_validators(3)
            .faulty_weight_perc(fault_perc)
            .fault_type(DesFault::PermanentlyMute)
            .consensus_values_count(cv_count)
            .weight_limits(100, 120)
            .build(&mut rng)
            .expect("Construction was successful");

        crank_until(&mut zug_test_harness, &mut rng, |zth| {
            // Stop the test when each node finalized expected number of consensus values.
            // Note that we're not testing the order of finalization here.
            // It will be tested later – it's not the condition for stopping the test run.
            zth.virtual_net
                .validators()
                .all(|v| v.finalized_count() == cv_count as usize)
        })
        .unwrap();

        let handle = zug_test_harness.mutable_handle();
        let validators = handle.validators();

        let finalized_values: Vec<Vec<ConsensusValue>> = validators
            .map(|v| v.finalized_values().cloned().collect::<Vec<_>>())
            .collect();

        assert_eq_vectors(
            finalized_values,
            "Nodes finalized different consensus values.",
        );
    }

    #[test]
    fn liveness_test_some_equivocate() {
        let _ = logging::init_with_config(&LoggingConfig::new(LoggingFormat::Text, true, true));

        let mut rng = crate::new_rng();
        let cv_count = 10;
        let fault_perc = 10;

        let mut zug_test_harness = ZugTestHarnessBuilder::new()
            .max_faulty_validators(3)
            .faulty_weight_perc(fault_perc)
            .fault_type(DesFault::Equivocate)
            .consensus_values_count(cv_count)
            .weight_limits(100, 150)
            .build(&mut rng)
            .expect("Construction was successful");

        crank_until(&mut zug_test_harness, &mut rng, |zth| {
            // Stop the test when each node finalized expected number of consensus values.
            // Note that we're not testing the order of finalization here.
            // It will be tested later – it's not the condition for stopping the test run.
            zth.virtual_net
                .validators()
                .all(|v| v.finalized_count() == cv_count as usize)
        })
        .unwrap();

        let handle = zug_test_harness.mutable_handle();
        let validators = handle.validators();

        let (finalized_values, equivocators_seen): (
            Vec<Vec<ConsensusValue>>,
            Vec<HashSet<ValidatorId>>,
        ) = validators
            .map(|v| {
                (
                    v.finalized_values().cloned().collect::<Vec<_>>(),
                    v.validator()
                        .zug()
                        .validators_with_evidence()
                        .into_iter()
                        .cloned()
                        .collect::<HashSet<_>>(),
                )
            })
            .unzip();

        assert_eq_vectors(
            finalized_values,
            "Nodes finalized different consensus values.",
        );
        assert_eq_vectors(
            equivocators_seen,
            "Nodes saw different set of equivocators.",
        );
    }
}
