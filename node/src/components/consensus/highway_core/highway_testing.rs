use std::{
    collections::{hash_map::DefaultHasher, HashMap, VecDeque},
    fmt::{self, Debug, Display, Formatter},
    hash::Hasher,
    iter::FromIterator,
};

use hex_fmt::HexFmt;
use itertools::Itertools;
use rand::Rng;
use serde::{Deserialize, Serialize};
use tracing::{trace, warn};

use super::{
    active_validator::Effect,
    evidence::Evidence,
    finality_detector::{FinalityDetector, FttExceeded},
    highway::{
        Dependency, GetDepOutcome, Highway, Params, PreValidatedVertex, SignedWireVote,
        ValidVertex, Vertex, VertexError,
    },
    validators::Validators,
    Weight,
};
use crate::{
    components::consensus::{
        consensus_protocol::FinalizedBlock,
        tests::{
            consensus_des_testing::{
                DeliverySchedule, Fault, Message, Node, Target, TargetedMessage, ValidatorId,
                VirtualNet,
            },
            queue::QueueEntry,
        },
        traits::{Context, ValidatorSecret},
        BlockContext,
    },
    types::{CryptoRngCore, Timestamp},
};

type ConsensusValue = Vec<u32>;

const TEST_MIN_ROUND_EXP: u8 = 12;
const TEST_END_HEIGHT: u64 = 100000;
pub(crate) const TEST_BLOCK_REWARD: u64 = 1_000_000_000_000;
pub(crate) const TEST_REDUCED_BLOCK_REWARD: u64 = 200_000_000_000;

#[derive(Clone, Eq, PartialEq)]
enum HighwayMessage {
    Timer(Timestamp),
    NewVertex(Vertex<TestContext>),
    RequestBlock(BlockContext),
    WeEquivocated(Evidence<TestContext>),
}

impl Debug for HighwayMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            HighwayMessage::Timer(t) => f.debug_tuple("Timer").field(&t.millis()).finish(),
            HighwayMessage::RequestBlock(bc) => f
                .debug_struct("RequestBlock")
                .field("timestamp", &bc.timestamp().millis())
                .finish(),
            HighwayMessage::NewVertex(v) => {
                f.debug_struct("NewVertex").field("vertex", &v).finish()
            }
            HighwayMessage::WeEquivocated(ev) => f.debug_tuple("WeEquivocated").field(&ev).finish(),
        }
    }
}

impl HighwayMessage {
    fn into_targeted(self, creator: ValidatorId) -> TargetedMessage<HighwayMessage> {
        let create_msg = |hwm: HighwayMessage| Message::new(creator, hwm);

        match self {
            HighwayMessage::NewVertex(_) => {
                TargetedMessage::new(create_msg(self), Target::AllExcept(creator))
            }
            HighwayMessage::Timer(_)
            | HighwayMessage::RequestBlock(_)
            | HighwayMessage::WeEquivocated(_) => {
                TargetedMessage::new(create_msg(self), Target::SingleValidator(creator))
            }
        }
    }

    fn is_new_vertex(&self) -> bool {
        match self {
            HighwayMessage::NewVertex(_) => true,
            _ => false,
        }
    }
}

impl From<Effect<TestContext>> for HighwayMessage {
    fn from(eff: Effect<TestContext>) -> Self {
        match eff {
            // The effect is `ValidVertex` but we want to gossip it to other
            // validators so for them it's just `Vertex` that needs to be validated.
            Effect::NewVertex(ValidVertex(v)) => HighwayMessage::NewVertex(v),
            Effect::ScheduleTimer(t) => HighwayMessage::Timer(t),
            Effect::RequestNewBlock(block_context) => HighwayMessage::RequestBlock(block_context),
            Effect::WeEquivocated(evidence) => HighwayMessage::WeEquivocated(evidence),
        }
    }
}

impl PartialOrd for HighwayMessage {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(&other))
    }
}

impl Ord for HighwayMessage {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (HighwayMessage::Timer(t1), HighwayMessage::Timer(t2)) => t1.cmp(&t2),
            (HighwayMessage::NewVertex(v1), HighwayMessage::NewVertex(v2)) => match (v1, v2) {
                (Vertex::Vote(swv1), Vertex::Vote(swv2)) => swv1.hash().cmp(&swv2.hash()),
                (Vertex::Vote(_), _) => std::cmp::Ordering::Less,
                (
                    Vertex::Evidence(Evidence::Equivocation(ev1_a, ev1_b)),
                    Vertex::Evidence(Evidence::Equivocation(ev2_a, ev2_b)),
                ) => ev1_a
                    .hash()
                    .cmp(&ev2_a.hash())
                    .then_with(|| ev1_b.hash().cmp(&ev2_b.hash())),
                (Vertex::Evidence(_), _) => std::cmp::Ordering::Less,
            },
            (HighwayMessage::RequestBlock(bc1), HighwayMessage::RequestBlock(bc2)) => bc1.cmp(&bc2),
            (HighwayMessage::WeEquivocated(ev1), HighwayMessage::WeEquivocated(ev2)) => {
                let Evidence::Equivocation(ev1_a, ev1_b) = ev1;
                let Evidence::Equivocation(ev2_a, ev2_b) = ev2;
                ev1_a
                    .hash()
                    .cmp(&ev2_a.hash())
                    .then_with(|| ev1_b.hash().cmp(&ev2_b.hash()))
            }
            (HighwayMessage::Timer(_), _) => std::cmp::Ordering::Less,
            (HighwayMessage::NewVertex(_), _) => std::cmp::Ordering::Less,
            (HighwayMessage::RequestBlock(_), _) => std::cmp::Ordering::Less,
            (HighwayMessage::WeEquivocated(_), _) => std::cmp::Ordering::Greater,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum TestRunError {
    /// VirtualNet was missing a validator when it was expected to exist.
    MissingValidator(ValidatorId),
    /// Sender sent a vertex for which it didn't have all dependencies.
    SenderMissingDependency(ValidatorId, Dependency<TestContext>),
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
            TestRunError::SenderMissingDependency(validator_id, dependency) => write!(
                f,
                "{:?} was missing a dependency {:?} of a vertex it created.",
                validator_id, dependency
            ),
            TestRunError::MissingValidator(id) => {
                write!(f, "Virtual net is missing validator {:?}.", id)
            }
        }
    }
}

enum Distribution {
    Uniform,
    // TODO: Poisson(f64), https://casperlabs.atlassian.net/browse/HWY-116
}

impl Distribution {
    /// Returns vector of `count` elements of random values between `lower` and `uppwer`.
    fn gen_range_vec(
        &self,
        rng: &mut dyn CryptoRngCore,
        lower: u64,
        upper: u64,
        count: u8,
    ) -> Vec<u64> {
        match self {
            Distribution::Uniform => (0..count).map(|_| rng.gen_range(lower, upper)).collect(),
        }
    }
}

trait DeliveryStrategy {
    fn gen_delay(
        &mut self,
        rng: &mut dyn CryptoRngCore,
        message: &HighwayMessage,
        distributon: &Distribution,
        base_delivery_timestamp: Timestamp,
    ) -> DeliverySchedule;
}

struct HighwayValidator {
    highway: Highway<TestContext>,
    finality_detector: FinalityDetector<TestContext>,
    fault: Option<Fault>,
}

impl HighwayValidator {
    fn new(
        highway: Highway<TestContext>,
        finality_detector: FinalityDetector<TestContext>,
        fault: Option<Fault>,
    ) -> Self {
        HighwayValidator {
            highway,
            finality_detector,
            fault,
        }
    }

    fn highway_mut(&mut self) -> &mut Highway<TestContext> {
        &mut self.highway
    }

    fn highway(&self) -> &Highway<TestContext> {
        &self.highway
    }

    fn run_finality(
        &mut self,
    ) -> Result<Vec<FinalizedBlock<ConsensusValue, ValidatorId>>, FttExceeded> {
        Ok(self.finality_detector.run(&self.highway)?.collect())
    }

    fn post_hook(
        &mut self,
        rng: &mut dyn CryptoRngCore,
        msg: HighwayMessage,
    ) -> Vec<HighwayMessage> {
        match self.fault.as_ref() {
            None => {
                // Honest validator.
                match &msg {
                    HighwayMessage::NewVertex(_)
                    | HighwayMessage::Timer(_)
                    | HighwayMessage::RequestBlock(_) => vec![msg],
                    HighwayMessage::WeEquivocated(ev) => {
                        panic!("validator equivocated unexpectedly: {:?}", ev);
                    }
                }
            }
            Some(Fault::Mute) => {
                // For mute validators we add it to the state but not gossip.
                match msg {
                    HighwayMessage::NewVertex(_) => {
                        warn!("Validator is mute – won't gossip vertices in response");
                        vec![]
                    }
                    HighwayMessage::Timer(_) | HighwayMessage::RequestBlock(_) => vec![msg],
                    HighwayMessage::WeEquivocated(ev) => {
                        panic!("validator equivocated unexpectedly: {:?}", ev);
                    }
                }
            }
            Some(Fault::Equivocate) => {
                match msg {
                    HighwayMessage::NewVertex(Vertex::Vote(ref swvote)) => {
                        // Create an equivocating message, with a different timestamp.
                        // TODO: Don't send both messages to every peer. Add different strategies.
                        let mut wvote = swvote.wire_vote.clone();
                        wvote.timestamp += 1.into();
                        let secret = TestSecret(wvote.creator.0.into());
                        let swvote2 = SignedWireVote::new(wvote, &secret, rng);
                        vec![msg, HighwayMessage::NewVertex(Vertex::Vote(swvote2))]
                    }
                    HighwayMessage::NewVertex(_)
                    | HighwayMessage::RequestBlock(_)
                    | HighwayMessage::WeEquivocated(_)
                    | HighwayMessage::Timer(_) => vec![msg],
                }
            }
        }
    }
}

type HighwayNode = Node<ConsensusValue, HighwayMessage, HighwayValidator>;

type HighwayNet = VirtualNet<ConsensusValue, HighwayMessage, HighwayValidator>;

struct HighwayTestHarness<DS>
where
    DS: DeliveryStrategy,
{
    virtual_net: HighwayNet,
    /// Consensus values to be proposed.
    /// Order of values in the vector defines the order in which they will be proposed.
    consensus_values: VecDeque<ConsensusValue>,
    /// A strategy to pseudo randomly change the message delivery times.
    delivery_time_strategy: DS,
    /// Distribution of delivery times.
    delivery_time_distribution: Distribution,
}

type TestResult<T> = Result<T, TestRunError>;

// Outer `Err` (from `TestResult`) represents an unexpected error in test framework, global error.
// Inner `Result` is a local result, its error is also local.
type TestRunResult<T> = TestResult<Result<T, (Vertex<TestContext>, VertexError)>>;

impl<DS> HighwayTestHarness<DS>
where
    DS: DeliveryStrategy,
{
    /// Advance the test by one message.
    ///
    /// Pops one message from the message queue (if there are any)
    /// and pass it to the recipient validator for execution.
    /// Messages returned from the execution are scheduled for later delivery.
    pub(crate) fn crank(&mut self, rng: &mut dyn CryptoRngCore) -> TestResult<()> {
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
            .filter_map(|hwm| {
                let delivery = self.delivery_time_strategy.gen_delay(
                    rng,
                    &hwm,
                    &self.delivery_time_distribution,
                    delivery_time,
                );
                match delivery {
                    DeliverySchedule::Drop => {
                        trace!("{:?} message is dropped.", hwm);
                        None
                    }
                    DeliverySchedule::AtInstant(timestamp) => {
                        trace!("{:?} scheduled for {:?}", hwm, timestamp);
                        let targeted = hwm.into_targeted(recipient);
                        Some((targeted, timestamp))
                    }
                }
            })
            .collect();

        self.virtual_net.dispatch_messages(targeted_messages);
        Ok(())
    }

    fn next_consensus_value(&mut self) -> ConsensusValue {
        self.consensus_values.pop_front().unwrap_or_default()
    }

    /// Helper for getting validator from the underlying virtual net.
    fn node_mut(&mut self, validator_id: &ValidatorId) -> TestResult<&mut HighwayNode> {
        self.virtual_net
            .node_mut(&validator_id)
            .ok_or_else(|| TestRunError::MissingValidator(*validator_id))
    }

    fn call_validator<F>(
        &mut self,
        rng: &mut dyn CryptoRngCore,
        validator_id: &ValidatorId,
        f: F,
    ) -> TestResult<Vec<HighwayMessage>>
    where
        F: FnOnce(&mut HighwayValidator, &mut dyn CryptoRngCore) -> Vec<Effect<TestContext>>,
    {
        let validator_node = self.node_mut(validator_id)?;
        let res = f(validator_node.validator_mut(), rng);
        let messages = res
            .into_iter()
            .flat_map(|eff| {
                validator_node
                    .validator_mut()
                    .post_hook(rng, HighwayMessage::from(eff))
            })
            .collect();
        Ok(messages)
    }

    /// Processes a message sent to `validator_id`.
    /// Returns a vector of messages produced by the `validator` in reaction to processing a
    /// message.
    fn process_message(
        &mut self,
        rng: &mut dyn CryptoRngCore,
        validator_id: ValidatorId,
        message: Message<HighwayMessage>,
        delivery_time: Timestamp,
    ) -> TestResult<Vec<HighwayMessage>> {
        self.node_mut(&validator_id)?
            .push_messages_received(vec![message.clone()]);

        let messages = {
            let sender_id = message.sender;

            let hwm = message.payload().clone();

            match hwm {
                HighwayMessage::Timer(timestamp) => {
                    self.call_validator(rng, &validator_id, |consensus, rng| {
                        consensus.highway_mut().handle_timer(timestamp, rng)
                    })?
                }
                HighwayMessage::NewVertex(v) => {
                    match self.add_vertex(rng, validator_id, sender_id, v.clone(), delivery_time)? {
                        Ok(msgs) => {
                            trace!("{:?} successfuly added to the state.", v);
                            msgs
                        }
                        Err((v, error)) => {
                            // TODO: this seems to get output from passing tests
                            warn!(
                                "{:?} sent an invalid vertex {:?} to {:?} \
                                that resulted in {:?} error",
                                sender_id, v, validator_id, error
                            );
                            vec![]
                        }
                    }
                }
                HighwayMessage::RequestBlock(block_context) => {
                    let consensus_value = self.next_consensus_value();

                    self.call_validator(rng, &validator_id, |consensus, rng| {
                        consensus
                            .highway_mut()
                            .propose(consensus_value, block_context, rng)
                    })?
                }
                HighwayMessage::WeEquivocated(_evidence) => vec![],
            }
        };

        let recipient = self.node_mut(&validator_id)?;
        recipient.push_messages_produced(messages.clone());

        self.run_finality_detector(&validator_id)?;

        Ok(messages)
    }

    /// Runs finality detector.
    fn run_finality_detector(&mut self, validator_id: &ValidatorId) -> TestResult<()> {
        let recipient = self.node_mut(validator_id)?;

        let finalized_values = recipient
            .validator_mut()
            .run_finality()
            // TODO: https://casperlabs.atlassian.net/browse/HWY-119
            .expect("FTT exceeded but not handled");
        for FinalizedBlock {
            value,
            timestamp: _,
            height,
            rewards,
            proposer: _,
        } in finalized_values
        {
            trace!(
                "{}consensus value finalized: {:?}, height: {:?}",
                if rewards.is_some() { "last " } else { "" },
                value,
                height
            );
            if let Some(r) = rewards {
                warn!(?r, "rewards are not verified yet");
            }
            recipient.push_finalized(value);
        }

        Ok(())
    }

    // Adds vertex to the `recipient` validator state.
    // Synchronizes its state if necessary.
    // From the POV of the test system, synchronization is immediate.
    fn add_vertex(
        &mut self,
        rng: &mut dyn CryptoRngCore,
        recipient: ValidatorId,
        sender: ValidatorId,
        vertex: Vertex<TestContext>,
        delivery_time: Timestamp,
    ) -> TestRunResult<Vec<HighwayMessage>> {
        // 1. pre_validate_vertex
        // 2. missing_dependency
        // 3. validate_vertex
        // 4. add_valid_vertex

        let sync_result = {
            let validator = self.node_mut(&recipient)?;

            match validator
                .validator_mut()
                .highway_mut()
                .pre_validate_vertex(vertex)
            {
                Err((v, error)) => Ok(Err((v, error))),
                Ok(pvv) => self.synchronize_validator(rng, recipient, sender, pvv, delivery_time),
            }
        }?;

        match sync_result {
            Err(vertex_error) => Ok(Err(vertex_error)),
            Ok((prevalidated_vertex, mut sync_effects)) => {
                let add_vertex_effects: Vec<HighwayMessage> = {
                    match self
                        .node_mut(&recipient)?
                        .validator_mut()
                        .highway_mut()
                        .validate_vertex(prevalidated_vertex)
                    {
                        Err((pvv, error)) => return Ok(Err((pvv.into_vertex(), error))),
                        Ok(valid_vertex) => self.call_validator(rng, &recipient, |v, rng| {
                            v.highway_mut()
                                .add_valid_vertex(valid_vertex, rng, delivery_time)
                        })?,
                    }
                };

                sync_effects.extend(add_vertex_effects);

                Ok(Ok(sync_effects))
            }
        }
    }

    /// Synchronizes all missing dependencies of `pvv` that `recipient` is missing.
    /// If an error occurs during synchronization of one of `pvv`'s dependencies
    /// it's returned and the original vertex mustn't be added to the state.
    fn synchronize_validator(
        &mut self,
        rng: &mut dyn CryptoRngCore,
        recipient: ValidatorId,
        sender: ValidatorId,
        pvv: PreValidatedVertex<TestContext>,
        delivery_time: Timestamp,
    ) -> TestRunResult<(PreValidatedVertex<TestContext>, Vec<HighwayMessage>)> {
        // There may be more than one dependency missing and we want to sync all of them.
        loop {
            let validator = self
                .virtual_net
                .validator(&recipient)
                .ok_or_else(|| TestRunError::MissingValidator(recipient))?
                .validator();

            let mut messages = vec![];

            match validator.highway().missing_dependency(&pvv) {
                None => return Ok(Ok((pvv, messages))),
                Some(d) => {
                    match self.synchronize_dependency(rng, d, recipient, sender, delivery_time)? {
                        Ok(sync_messages) => {
                            // `hwm` represent messages produced while synchronizing `d`.
                            messages.extend(sync_messages)
                        }
                        Err(vertex_error) => {
                            // An error occurred when trying to synchronize a missing dependency.
                            // We must stop the synchronization process and return it to the caller.
                            return Ok(Err(vertex_error));
                        }
                    }
                }
            }
        }
    }

    // Synchronizes `validator` in case of missing dependencies.
    //
    // If validator has missing dependencies then we have to add them first.
    // We don't want to test synchronization, and the Highway theory assumes
    // that when votes are added then all their dependencies are satisfied.
    #[allow(clippy::type_complexity)]
    fn synchronize_dependency(
        &mut self,
        rng: &mut dyn CryptoRngCore,
        missing_dependency: Dependency<TestContext>,
        recipient: ValidatorId,
        sender: ValidatorId,
        delivery_time: Timestamp,
    ) -> TestRunResult<Vec<HighwayMessage>> {
        match self
            .node_mut(&sender)?
            .validator_mut()
            .highway()
            .get_dependency(&missing_dependency)
        {
            GetDepOutcome::Vertex(vv) => {
                self.add_vertex(rng, recipient, sender, vv.0, delivery_time)
            }
            GetDepOutcome::Evidence(_) | GetDepOutcome::None => Err(
                TestRunError::SenderMissingDependency(sender, missing_dependency),
            ),
        }
    }

    /// Returns a `MutableHandle` on the `HighwayTestHarness` object
    /// that allows for manipulating internal state of the test state.
    fn mutable_handle(&mut self) -> MutableHandle<DS> {
        MutableHandle(self)
    }
}

fn crank_until<F, DS: DeliveryStrategy>(
    htt: &mut HighwayTestHarness<DS>,
    rng: &mut dyn CryptoRngCore,
    f: F,
) -> TestResult<()>
where
    F: Fn(&HighwayTestHarness<DS>) -> bool,
{
    while !f(htt) {
        htt.crank(rng)?;
    }
    Ok(())
}

struct MutableHandle<'a, DS: DeliveryStrategy>(&'a mut HighwayTestHarness<DS>);

impl<'a, DS: DeliveryStrategy> MutableHandle<'a, DS> {
    /// Drops all messages from the queue.
    fn clear_message_queue(&mut self) {
        self.0.virtual_net.empty_queue();
    }

    fn validators(&self) -> impl Iterator<Item = &HighwayNode> {
        self.0.virtual_net.validators()
    }
}

enum BuilderError {
    WeightLimits,
    TooManyFaultyNodes(String),
}

struct HighwayTestHarnessBuilder<DS: DeliveryStrategy> {
    /// Maximum number of faulty validators in the network.
    /// Defaults to 10.
    max_faulty_validators: u8,
    /// Percentage of faulty validators' (i.e. equivocators) weight.
    /// Defaults to 0 (network is perfectly secure).
    faulty_percent: u64,
    fault_type: Option<Fault>,
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
    /// Type of discrete distribution of validators' weights.
    /// Defaults to uniform.
    weight_distribution: Distribution,
    /// Seed for `Highway`.
    /// Defaults to 0.
    seed: u64,
}

// Default strategy for message delivery.
struct InstantDeliveryNoDropping;

impl DeliveryStrategy for InstantDeliveryNoDropping {
    fn gen_delay(
        &mut self,
        _rng: &mut dyn CryptoRngCore,
        message: &HighwayMessage,
        _distributon: &Distribution,
        base_delivery_timestamp: Timestamp,
    ) -> DeliverySchedule {
        match message {
            HighwayMessage::RequestBlock(bc) => DeliverySchedule::AtInstant(bc.timestamp()),
            HighwayMessage::Timer(t) => DeliverySchedule::AtInstant(*t),
            HighwayMessage::NewVertex(_) => {
                DeliverySchedule::AtInstant(base_delivery_timestamp + 1.into())
            }
            HighwayMessage::WeEquivocated(_) => {
                DeliverySchedule::AtInstant(base_delivery_timestamp + 1.into())
            }
        }
    }
}

impl HighwayTestHarnessBuilder<InstantDeliveryNoDropping> {
    fn new() -> Self {
        HighwayTestHarnessBuilder {
            max_faulty_validators: 10,
            faulty_percent: 0,
            fault_type: None,
            ftt: None,
            consensus_values_count: 10,
            delivery_distribution: Distribution::Uniform,
            delivery_strategy: InstantDeliveryNoDropping,
            weight_limits: (1, 100),
            start_time: Timestamp::zero(),
            weight_distribution: Distribution::Uniform,
            seed: 0,
        }
    }
}

impl<DS: DeliveryStrategy> HighwayTestHarnessBuilder<DS> {
    /// Sets a percentage of weight that will be assigned to malicious nodes.
    /// `faulty_weight` must be a value between 0 (inclusive) and 33 (inclusive).
    pub(crate) fn faulty_weight_perc(mut self, faulty_weight: u64) -> Self {
        self.faulty_percent = faulty_weight;
        self
    }

    fn fault_type(mut self, fault_type: Fault) -> Self {
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

    fn build(self, rng: &mut dyn CryptoRngCore) -> Result<HighwayTestHarness<DS>, BuilderError> {
        let consensus_values = (0..self.consensus_values_count as u32)
            .map(|el| vec![el])
            .collect::<VecDeque<ConsensusValue>>();

        let instance_id = 0;
        let seed = self.seed;
        let start_time = self.start_time;

        let (lower, upper) = {
            let (l, u) = self.weight_limits;
            if l >= u {
                return Err(BuilderError::WeightLimits);
            }
            (l, u)
        };

        let (faulty_weights, honest_weights): (Vec<Weight>, Vec<Weight>) = {
            if self.faulty_percent > 33 {
                return Err(BuilderError::TooManyFaultyNodes(
                    "Total weight of all malicious validators cannot be more than 33% of all network weight."
                        .to_string(),
                ));
            }

            if self.faulty_percent == 0 {
                // All validators are honest.
                let validators_num = rng.gen_range(2, self.max_faulty_validators + 1);
                let honest_validators: Vec<Weight> = self
                    .weight_distribution
                    .gen_range_vec(rng, lower, upper, validators_num)
                    .into_iter()
                    .map(Weight)
                    .collect();

                (vec![], honest_validators)
            } else {
                // At least 2 validators total and at least one faulty.
                let faulty_num = rng.gen_range(1, self.max_faulty_validators + 1);

                // Randomly (but within chosed range) assign weights to faulty nodes.
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
                            rng.gen_range(lower, upper)
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

        let validators: Validators<ValidatorId> = Validators::from_iter(
            faulty_weights
                .iter()
                .chain(honest_weights.iter())
                .enumerate()
                .map(|(i, weight)| (ValidatorId(i as u64), *weight)),
        );

        trace!(
            "Weights: {:?}",
            validators.iter().map(|v| v).collect::<Vec<_>>()
        );

        let mut secrets = validators
            .iter()
            .map(|validator| (*validator.id(), TestSecret(validator.id().0)))
            .collect();

        let ftt = self
            .ftt
            .map(|p| p * weights_sum.0 / 100)
            .unwrap_or_else(|| (weights_sum.0 - 1) / 3);

        // Local function creating an instance of `HighwayConsensus` for a single validator.
        let highway_consensus =
            |(vid, secrets): (ValidatorId, &mut HashMap<ValidatorId, TestSecret>)| {
                let v_sec = secrets.remove(&vid).expect("Secret key should exist.");

                let params = Params::new(
                    seed,
                    TEST_BLOCK_REWARD,
                    TEST_REDUCED_BLOCK_REWARD,
                    TEST_MIN_ROUND_EXP,
                    TEST_END_HEIGHT,
                    Timestamp::now(),
                    Timestamp::zero(), // Length depends only on block number.
                );
                let mut highway = Highway::new(instance_id, validators.clone(), params);
                let effects = highway.activate_validator(vid, v_sec, params, start_time);

                let finality_detector = FinalityDetector::new(Weight(ftt));

                (
                    highway,
                    finality_detector,
                    effects.into_iter().map(HighwayMessage::from).collect_vec(),
                )
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
                let (highway, finality_detector, msgs) = highway_consensus((vid, &mut secrets));
                let highway_consensus = HighwayValidator::new(highway, finality_detector, fault);
                let validator = Node::new(vid, highway_consensus);
                let qm: Vec<QueueEntry<HighwayMessage>> = msgs
                    .into_iter()
                    .map(|hwm| {
                        // These are messages crated on the start of the network.
                        // They are sent from validator to himself.
                        QueueEntry::new(start_time, vid, Message::new(vid, hwm))
                    })
                    .collect();
                init_messages.extend(qm);
                validators_loc.push(validator);
            }

            (validators_loc, init_messages)
        };

        let delivery_time_strategy = self.delivery_strategy;

        let delivery_time_distribution = self.delivery_distribution;

        let virtual_net = VirtualNet::new(validators, init_messages);

        let hwth = HighwayTestHarness {
            virtual_net,
            consensus_values,
            delivery_time_strategy,
            delivery_time_distribution,
        };

        Ok(hwth)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct TestContext;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TestSecret(pub(crate) u64);

// Newtype wrapper for test signature.
// Added so that we can use custom Debug impl.
#[derive(Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct SignatureWrapper(u64);

impl Debug for SignatureWrapper {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:10}", HexFmt(self.0.to_le_bytes()))
    }
}

// Newtype wrapper for test hash.
// Added so that we can use custom Debug impl.
#[derive(Clone, Hash, Ord, PartialOrd, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct HashWrapper(u64);

impl Debug for HashWrapper {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:10}", HexFmt(self.0.to_le_bytes()))
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

    fn sign(&self, data: &Self::Hash, _rng: &mut dyn CryptoRngCore) -> Self::Signature {
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
        crank_until, ConsensusValue, HighwayTestHarness, HighwayTestHarnessBuilder,
        InstantDeliveryNoDropping, TestRunError,
    };
    use crate::{
        components::consensus::tests::consensus_des_testing::{Fault, ValidatorId},
        logging,
        testing::TestRng,
    };
    use logging::{LoggingConfig, LoggingFormat};

    #[test]
    fn on_empty_queue_error() {
        let mut rng = TestRng::new();
        let mut highway_test_harness: HighwayTestHarness<InstantDeliveryNoDropping> =
            HighwayTestHarnessBuilder::new()
                .consensus_values_count(1)
                .weight_limits(100, 120)
                .build(&mut rng)
                .ok()
                .expect("Construction was successful");

        highway_test_harness.mutable_handle().clear_message_queue();

        assert_eq!(
            highway_test_harness.crank(&mut rng),
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

        let mut rng = TestRng::new();
        let cv_count = 10;

        let mut highway_test_harness = HighwayTestHarnessBuilder::new()
            .max_faulty_validators(3)
            .consensus_values_count(cv_count)
            .weight_limits(100, 120)
            .build(&mut rng)
            .ok()
            .expect("Construction was successful");

        crank_until(&mut highway_test_harness, &mut rng, |hth| {
            // Stop the test when each node finalized expected number of consensus values.
            // Note that we're not testing the order of finalization here.
            // It will be tested later – it's not the condition for stopping the test run.
            hth.virtual_net
                .validators()
                .all(|v| v.finalized_count() == cv_count as usize)
        })
        .unwrap();

        let handle = highway_test_harness.mutable_handle();
        let validators = handle.validators();

        let (finalized_values, vertices_produced): (Vec<Vec<ConsensusValue>>, Vec<usize>) =
            validators
                .map(|v| {
                    (
                        v.finalized_values().cloned().collect::<Vec<_>>(),
                        v.messages_produced()
                            .cloned()
                            .filter(|hwm| hwm.is_new_vertex())
                            .count(),
                    )
                })
                .unzip();

        vertices_produced
            .into_iter()
            .enumerate()
            .for_each(|(v_idx, vertices_count)| {
                // NOTE: Works only when all validators are honest and correct (no "mute"
                // validators). Validator produces two `NewVertex` type messages per round. It may
                // produce just one before lambda message is finalized. Add one in case it's just
                // one round (one consensus value) – 1 message. 1/2=0 but 3/2=1 b/c of the rounding.
                let rounds_participated_in = (vertices_count as u8 + 1) / 2;

                assert_eq!(
                    rounds_participated_in, cv_count,
                    "Expected that validator={} participated in {} rounds.",
                    v_idx, cv_count
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

        let mut rng = TestRng::new();
        let cv_count = 10;
        let fault_perc = 30;

        let mut highway_test_harness = HighwayTestHarnessBuilder::new()
            .max_faulty_validators(3)
            .faulty_weight_perc(fault_perc)
            .fault_type(Fault::Mute)
            .consensus_values_count(cv_count)
            .weight_limits(100, 120)
            .build(&mut rng)
            .ok()
            .expect("Construction was successful");

        crank_until(&mut highway_test_harness, &mut rng, |hth| {
            // Stop the test when each node finalized expected number of consensus values.
            // Note that we're not testing the order of finalization here.
            // It will be tested later – it's not the condition for stopping the test run.
            hth.virtual_net
                .validators()
                .all(|v| v.finalized_count() == cv_count as usize)
        })
        .unwrap();

        let handle = highway_test_harness.mutable_handle();
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

        let mut rng = TestRng::new();
        let cv_count = 10;
        let fault_perc = 10;

        let mut highway_test_harness = HighwayTestHarnessBuilder::new()
            .max_faulty_validators(3)
            .faulty_weight_perc(fault_perc)
            .fault_type(Fault::Equivocate)
            .consensus_values_count(cv_count)
            .weight_limits(100, 150)
            .build(&mut rng)
            .ok()
            .expect("Construction was successful");

        crank_until(&mut highway_test_harness, &mut rng, |hth| {
            // Stop the test when each node finalized expected number of consensus values.
            // Note that we're not testing the order of finalization here.
            // It will be tested later – it's not the condition for stopping the test run.
            hth.virtual_net
                .validators()
                .all(|v| v.finalized_count() == cv_count as usize)
        })
        .unwrap();

        let handle = highway_test_harness.mutable_handle();
        let validators = handle.validators();

        let (finalized_values, equivocators_seen): (
            Vec<Vec<ConsensusValue>>,
            Vec<HashSet<ValidatorId>>,
        ) = validators
            .map(|v| {
                (
                    v.finalized_values().cloned().collect::<Vec<_>>(),
                    v.validator()
                        .highway()
                        .validators_with_evidence()
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
