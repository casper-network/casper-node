use super::{
    active_validator::Effect,
    evidence::Evidence,
    finality_detector::{FinalityDetector, FinalityOutcome},
    highway::{Highway, PreValidatedVertex, ValidVertex, VertexError},
    validators::{ValidatorIndex, Validators},
    vertex::{Dependency, Vertex},
    Weight,
};

use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::iter::{self, FromIterator};
use tracing::{error, info, trace, warn};

use crate::{
    components::consensus::{
        tests::{
            consensus_des_testing::{
                DeliverySchedule, Fault, Message, Node, Target, TargetedMessage, ValidatorId,
                VirtualNet,
            },
            queue::{MessageT, QueueEntry},
        },
        traits::{ConsensusValueT, Context, ValidatorSecret},
        BlockContext,
    },
    types::Timestamp,
};

type ConsensusValue = Vec<u32>;

const TEST_FORGIVENESS_FACTOR: (u16, u16) = (2, 5);
const TEST_MIN_ROUND_EXP: u8 = 12;

#[derive(Clone, Eq, PartialEq)]
enum HighwayMessage {
    Timer(Timestamp),
    NewVertex(Vertex<TestContext>),
    RequestBlock(BlockContext),
}

impl Debug for HighwayMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Timer(t) => f.debug_tuple("Timer").field(&t.millis()).finish(),
            RequestBlock(bc) => f
                .debug_struct("RequestBlock")
                .field("timestamp", &bc.timestamp().millis())
                .finish(),
            NewVertex(v) => f.debug_struct("NewVertex").field("vertex", &v).finish(),
        }
    }
}

impl HighwayMessage {
    fn into_targeted(self, creator: ValidatorId) -> TargetedMessage<HighwayMessage> {
        let create_msg = |hwm: HighwayMessage| Message::new(creator, hwm);

        match self {
            NewVertex(_) => TargetedMessage::new(create_msg(self), Target::AllExcept(creator)),
            Timer(_) | RequestBlock(_) => {
                TargetedMessage::new(create_msg(self), Target::SingleValidator(creator))
            }
        }
    }

    fn is_new_vertex(&self) -> bool {
        match self {
            NewVertex(_) => true,
            _ => false,
        }
    }
}

use HighwayMessage::*;

impl From<Effect<TestContext>> for HighwayMessage {
    fn from(eff: Effect<TestContext>) -> Self {
        match eff {
            // The effect is `ValidVertex` but we want to gossip it to other
            // validators so for them it's just `Vertex` that needs to be validated.
            Effect::NewVertex(ValidVertex(v)) => NewVertex(v),
            Effect::ScheduleTimer(t) => Timer(t),
            Effect::RequestNewBlock(block_context) => RequestBlock(block_context),
        }
    }
}

use rand::Rng;
use std::{
    cell::RefCell,
    collections::{hash_map::DefaultHasher, HashMap, VecDeque},
    fmt::{Debug, Display, Formatter},
    hash::Hasher,
    marker::PhantomData,
};

impl PartialOrd for HighwayMessage {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(&other))
    }
}

impl Ord for HighwayMessage {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (Timer(t1), Timer(t2)) => t1.cmp(&t2),
            (Timer(_), _) => std::cmp::Ordering::Less,
            (NewVertex(v1), NewVertex(v2)) => match (v1, v2) {
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
            (NewVertex(_), _) => std::cmp::Ordering::Less,
            (RequestBlock(bc1), RequestBlock(bc2)) => bc1.cmp(&bc2),
            (RequestBlock(_), _) => std::cmp::Ordering::Less,
        }
    }
}

/// Result of single `crank()` call.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum CrankOk {
    /// Test run is not done.
    Continue,
    /// Test run is finished.
    Done,
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
    Poisson(f64),
}

impl Distribution {
    /// Returns vector of `count` elements of random values between `lower` and `uppwer`.
    fn gen_range_vec<R: Rng>(&self, rng: &mut R, lower: u64, upper: u64, count: u8) -> Vec<u64> {
        match self {
            Distribution::Uniform => (0..count).map(|_| rng.gen_range(lower, upper)).collect(),
            // https://casperlabs.atlassian.net/browse/HWY-116
            Distribution::Poisson(_) => unimplemented!("Poisson distribution of weights"),
        }
    }
}

trait DeliveryStrategy {
    fn gen_delay<R: Rng>(
        &mut self,
        rng: &mut R,
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

    fn run_finality(&mut self) -> FinalityOutcome<TestContext> {
        self.finality_detector.run(&self.highway)
    }

    fn post_hook<R: Rng>(&mut self, rng: &mut R, msg: HighwayMessage) -> Vec<HighwayMessage> {
        match self.fault.as_ref() {
            None => {
                // Honest validator.
                // If validator produced a `NewVertex` effect,
                // we want to add it to his state immediately and gossip all effects.
                match &msg {
                    NewVertex(vv) => self
                        .highway_mut()
                        .add_valid_vertex(ValidVertex(vv.clone()))
                        .into_iter()
                        .map(HighwayMessage::from)
                        .chain(iter::once(msg))
                        .collect(),
                    Timer(_) | RequestBlock(_) => vec![msg],
                }
            }
            Some(Fault::Mute) => {
                // For mute validators we add it to the state but not gossip.
                match msg {
                    NewVertex(vv) => {
                        warn!("Validator is mute – won't gossip vertices in response");
                        vec![]
                    }
                    Timer(_) | RequestBlock(_) => vec![msg],
                }
            }
            Some(Fault::Equivocate) => {
                // For equivocators we don't add to state and gossip.
                // This way, the next time this validator creates a message
                // it won't cite previous one.
                match msg {
                    NewVertex(_) => {
                        warn!(
                            "Validator is an equivocator – not adding {:?} to the state.",
                            msg
                        );
                        vec![msg]
                    }
                    Timer(_) | RequestBlock(_) => vec![msg],
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
    /// The instant the network was created.
    start_time: Timestamp,
    /// Consensus values to be proposed.
    /// Order of values in the vector defines the order in which they will be proposed.
    consensus_values: VecDeque<ConsensusValue>,
    /// Number of consensus values that the test is started with.
    consensus_values_num: usize,
    /// A strategy to pseudo randomly change the message delivery times.
    delivery_time_strategy: DS,
    /// Distribution of delivery times.
    delivery_time_distribution: Distribution,
}

type TestResult<T> = Result<T, TestRunError>;

// Outer `Err` (from `TestResult`) represents an unexpected error in test framework, global error.
// Inner `Result` is a local result, it's error is also local.
type TestRunResult<T> = TestResult<Result<T, (Vertex<TestContext>, VertexError)>>;

impl<DS> HighwayTestHarness<DS>
where
    DS: DeliveryStrategy,
{
    fn new<T: Into<Timestamp>>(
        virtual_net: HighwayNet,
        start_time: T,
        consensus_values: VecDeque<ConsensusValue>,
        delivery_time_distribution: Distribution,
        delivery_time_strategy: DS,
    ) -> Self {
        let cv_len = consensus_values.len();
        HighwayTestHarness {
            virtual_net,
            start_time: start_time.into(),
            consensus_values,
            consensus_values_num: cv_len,
            delivery_time_distribution,
            delivery_time_strategy,
        }
    }

    /// Advance the test by one message.
    ///
    /// Pops one message from the message queue (if there are any)
    /// and pass it to the recipient validator for execution.
    /// Messages returned from the execution are scheduled for later delivery.
    pub(crate) fn crank<R: Rng>(&mut self, rng: &mut R) -> TestResult<()> {
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

        let messages = self.process_message(rng, recipient, message)?;

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

    fn call_validator<F, R: Rng>(
        &mut self,
        rng: &mut R,
        validator_id: &ValidatorId,
        f: F,
    ) -> TestResult<Vec<HighwayMessage>>
    where
        F: FnOnce(&mut HighwayValidator) -> Vec<Effect<TestContext>>,
    {
        let validator_node = self.node_mut(validator_id)?;
        let res = f(validator_node.validator_mut());
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
    fn process_message<R: Rng>(
        &mut self,
        rng: &mut R,
        validator_id: ValidatorId,
        message: Message<HighwayMessage>,
    ) -> TestResult<Vec<HighwayMessage>> {
        self.node_mut(&validator_id)?
            .push_messages_received(vec![message.clone()]);

        let messages = {
            let sender_id = message.sender;

            let v = self.node_mut(&validator_id)?;

            let hwm = message.payload().clone();

            match hwm {
                Timer(timestamp) => self.call_validator(rng, &validator_id, |consensus| {
                    consensus.highway_mut().handle_timer(timestamp)
                })?,

                NewVertex(v) => {
                    match self.add_vertex(rng, validator_id, sender_id, v.clone())? {
                        Ok(msgs) => {
                            trace!("{:?} successfuly added to the state.", v);
                            msgs
                        }
                        Err((v, error)) => {
                            error!("{:?} sent an invalid vertex {:?} to {:?} that resulted in {:?} error", sender_id, v, validator_id, error);
                            vec![]
                        }
                    }
                }
                RequestBlock(block_context) => {
                    let consensus_value = self.next_consensus_value();

                    self.call_validator(rng, &validator_id, |consensus| {
                        consensus
                            .highway_mut()
                            .propose(consensus_value, block_context)
                    })?
                }
            }
        };

        let recipient = self.node_mut(&validator_id)?;
        recipient.push_messages_produced(messages.clone());

        self.run_finality_detector(&validator_id);

        Ok(messages)
    }

    /// Runs finality detector.
    fn run_finality_detector(&mut self, validator_id: &ValidatorId) -> TestResult<()> {
        let recipient = self.node_mut(validator_id)?;

        let finality_result = match recipient.validator_mut().run_finality() {
            FinalityOutcome::Finalized {
                value,
                new_equivocators,
                rewards,
                timestamp,
            } => {
                if !new_equivocators.is_empty() {
                    warn!("New equivocators detected: {:?}", new_equivocators);
                    recipient.new_equivocators(new_equivocators.into_iter());
                }
                if !rewards.is_empty() {
                    warn!("Rewards are not verified yet: {:?}", rewards);
                }
                trace!("Consensus value finalized: {:?}", value);
                vec![value]
            }
            FinalityOutcome::None => vec![],
            // https://casperlabs.atlassian.net/browse/HWY-119
            FinalityOutcome::FttExceeded => unimplemented!("Ftt exceeded but not handled."),
        };

        recipient.push_finalized(finality_result);

        Ok(())
    }

    // Adds vertex to the `recipient` validator state.
    // Synchronizes its state if necessary.
    // From the POV of the test system, synchronization is immediate.
    fn add_vertex<R: Rng>(
        &mut self,
        rng: &mut R,
        recipient: ValidatorId,
        sender: ValidatorId,
        vertex: Vertex<TestContext>,
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
                Ok(pvv) => self.synchronize_validator(rng, recipient, sender, pvv),
            }
        }?;

        match sync_result {
            Err(vertex_error) => Ok(Err(vertex_error)),
            Ok(prevalidated_vertex) => {
                let add_vertex_effects: Vec<HighwayMessage> = {
                    match self
                        .node_mut(&recipient)?
                        .validator_mut()
                        .highway_mut()
                        .validate_vertex(prevalidated_vertex)
                    {
                        Err((pvv, error)) => return Ok(Err((pvv.into_vertex(), error))),
                        Ok(valid_vertex) => self.call_validator(rng, &recipient, |v| {
                            v.highway_mut().add_valid_vertex(valid_vertex)
                        })?,
                    }
                };

                Ok(Ok(add_vertex_effects))
            }
        }
    }

    /// Synchronizes all missing dependencies of `pvv` that `recipient` is missing.
    /// If an error occurs during synchronization of one of `pvv`'s dependencies
    /// it's returned and the original vertex mustn't be added to the state.
    fn synchronize_validator<R: Rng>(
        &mut self,
        rng: &mut R,
        recipient: ValidatorId,
        sender: ValidatorId,
        pvv: PreValidatedVertex<TestContext>,
    ) -> TestRunResult<PreValidatedVertex<TestContext>> {
        // There may be more than one dependency missing and we want to sync all of them.
        loop {
            let validator = self
                .virtual_net
                .validator(&recipient)
                .ok_or_else(|| TestRunError::MissingValidator(recipient))?
                .validator();

            match validator.highway().missing_dependency(&pvv) {
                None => return Ok(Ok(pvv)),
                Some(d) => match self.synchronize_dependency(rng, d, recipient, sender)? {
                    Ok(()) => continue,
                    Err(vertex_error) => {
                        // An error occurred when trying to synchronize a missing dependency.
                        // We must stop the synchronization process and return it to the caller.
                        return Ok(Err(vertex_error));
                    }
                },
            }
        }
    }

    // Synchronizes `validator` in case of missing dependencies.
    //
    // If validator has missing dependencies then we have to add them first.
    // We don't want to test synchronization, and the Highway theory assumes
    // that when votes are added then all their dependencies are satisfied.
    #[allow(clippy::type_complexity)]
    fn synchronize_dependency<R: Rng>(
        &mut self,
        rng: &mut R,
        missing_dependency: Dependency<TestContext>,
        recipient: ValidatorId,
        sender: ValidatorId,
    ) -> TestRunResult<()> {
        let vertex = self
            .node_mut(&sender)?
            .validator_mut()
            .highway()
            .get_dependency(&missing_dependency)
            .map(|vv| vv.0)
            .ok_or_else(|| TestRunError::SenderMissingDependency(sender, missing_dependency))?;

        match self.add_vertex(rng, recipient, sender, vertex)? {
            Ok(messages) => {
                if !messages.is_empty() {
                    error!("Syncing produced effects. There should be no effects produced while syncing.");
                    panic!("Syncing produced effects.")
                } else {
                    Ok(Ok(()))
                }
            }
            Err(err) => Ok(Err(err)),
        }
    }

    /// Returns a `MutableHandle` on the `HighwayTestHarness` object
    /// that allows for manipulating internal state of the test state.
    fn mutable_handle(&mut self) -> MutableHandle<DS> {
        MutableHandle(self)
    }
}

fn crank_until<F, R: Rng, DS: DeliveryStrategy>(
    htt: &mut HighwayTestHarness<DS>,
    rng: &mut R,
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
    EmptyConsensusValues,
    WeightLimits,
    TooManyFaultyNodes(String),
    EmptyFtt,
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
    /// Round exponent.
    /// Defaults to 12.
    round_exp: u8,
}

// Default strategy for message delivery.
struct InstantDeliveryNoDropping;

impl DeliveryStrategy for InstantDeliveryNoDropping {
    fn gen_delay<R: Rng>(
        &mut self,
        _rng: &mut R,
        message: &HighwayMessage,
        _distributon: &Distribution,
        base_delivery_timestamp: Timestamp,
    ) -> DeliverySchedule {
        match message {
            RequestBlock(bc) => DeliverySchedule::AtInstant(bc.timestamp()),
            Timer(t) => DeliverySchedule::AtInstant(*t),
            NewVertex(_) => DeliverySchedule::AtInstant(base_delivery_timestamp + 1.into()),
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
            round_exp: 12,
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

    pub(crate) fn delivery_strategy<DS2: DeliveryStrategy>(
        self,
        ds: DS2,
    ) -> HighwayTestHarnessBuilder<DS2> {
        HighwayTestHarnessBuilder {
            max_faulty_validators: self.max_faulty_validators,
            faulty_percent: self.faulty_percent,
            fault_type: self.fault_type,
            ftt: self.ftt,
            consensus_values_count: self.consensus_values_count,
            delivery_distribution: self.delivery_distribution,
            delivery_strategy: ds,
            weight_limits: self.weight_limits,
            start_time: self.start_time,
            weight_distribution: self.weight_distribution,
            seed: self.seed,
            round_exp: self.round_exp,
        }
    }

    pub(crate) fn weight_limits(mut self, lower: u64, upper: u64) -> Self {
        self.weight_limits = (lower, upper);
        self
    }

    pub(crate) fn weight_distribution(mut self, wd: Distribution) -> Self {
        self.weight_distribution = wd;
        self
    }

    pub(crate) fn start_time<T: Into<Timestamp>>(mut self, start_time: T) -> Self {
        self.start_time = start_time.into();
        self
    }

    pub(crate) fn seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    pub(crate) fn round_exp(mut self, round_exp: u8) -> Self {
        self.round_exp = round_exp;
        self
    }

    pub(crate) fn ftt(mut self, ftt: u64) -> Self {
        assert!(ftt < 100 && ftt > 0);
        self.ftt = Some(ftt);
        self
    }

    fn max_faulty_validators(mut self, max_faulty_count: u8) -> Self {
        self.max_faulty_validators = max_faulty_count;
        self
    }

    fn build<R: Rng>(mut self, rng: &mut R) -> Result<HighwayTestHarness<DS>, BuilderError> {
        let consensus_values = (0..self.consensus_values_count as u32)
            .map(|el| vec![el])
            .collect::<VecDeque<Vec<u32>>>();

        let instance_id = 0;
        let seed = self.seed;
        let round_exp = self.round_exp;
        let start_time = self.start_time;

        let (lower, upper) = {
            let (l, u) = self.weight_limits;
            if l >= u {
                return Err(BuilderError::WeightLimits);
            }
            (l, u)
        };

        let (faulty_weights, honest_weights): (Vec<Weight>, Vec<Weight>) = {
            if (self.faulty_percent > 33) {
                return Err(BuilderError::TooManyFaultyNodes(
                    "Total weight of all malicious validators cannot be more than 33% of all network weight."
                        .to_string(),
                ));
            }

            if (self.faulty_percent == 0) {
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

                // Assign enough weights to honest nodes so that we reach expected `faulty_percantage` ratio.
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

                let mut highway = Highway::new(
                    instance_id,
                    validators.clone(),
                    seed,
                    TEST_FORGIVENESS_FACTOR,
                    TEST_MIN_ROUND_EXP,
                );
                let effects = highway.activate_validator(vid, v_sec, round_exp, start_time);

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
                let fault = if (vid.0 < faulty_num as u64) {
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
                        QueueEntry::new(self.start_time, vid, Message::new(vid, hwm))
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

        let cv_len = consensus_values.len();

        let hwth = HighwayTestHarness {
            virtual_net,
            start_time: self.start_time,
            consensus_values,
            consensus_values_num: cv_len,
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
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:10}", hex_fmt::HexFmt(self.0.to_le_bytes()))
    }
}

// Newtype wrapper for test hash.
// Added so that we can use custom Debug impl.
#[derive(Clone, Hash, Ord, PartialOrd, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct HashWrapper(u64);

impl Debug for HashWrapper {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:10}", hex_fmt::HexFmt(self.0.to_le_bytes()))
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
    type ConsensusValue = Vec<u32>;
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
    use std::{
        collections::{hash_map::DefaultHasher, HashMap, HashSet},
        hash::Hasher,
    };

    use super::{
        crank_until, ConsensusValue, CrankOk, DeliverySchedule, DeliveryStrategy, HighwayMessage,
        HighwayNode, HighwayTestHarness, HighwayTestHarnessBuilder, InstantDeliveryNoDropping,
        TestRunError,
    };
    use crate::{
        components::consensus::{
            highway_core::{validators::ValidatorIndex, vertex::Vertex},
            tests::consensus_des_testing::{Fault, ValidatorId},
            traits::{Context, ValidatorSecret},
        },
        logging,
        testing::TestRng,
        types::TimeDiff,
    };
    use logging::{LoggingConfig, LoggingFormat};
    use tracing::{span, warn, Level, Span};

    #[test]
    fn on_empty_queue_error() {
        let mut rng = TestRng::new();
        let mut highway_test_harness: HighwayTestHarness<InstantDeliveryNoDropping> =
            HighwayTestHarnessBuilder::new()
                .consensus_values_count(1)
                .weight_limits(7, 10)
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

    // Test that validators have finalized consensus values in the same order.
    fn assert_finalization_order(
        finalized_values: Vec<Vec<ConsensusValue>>,
        expected_number: usize,
    ) {
        let mut iter = finalized_values.into_iter();
        let reference_order = iter.next().unwrap();

        assert_eq!(
            reference_order.len(),
            expected_number,
            "Expected to finalize {} consensus values.",
            expected_number
        );

        iter.for_each(|v| assert_eq!(v, reference_order));
    }

    #[test]
    fn liveness_test_no_faults() {
        logging::init_with_config(&LoggingConfig::new(LoggingFormat::Text, true)).ok();

        let mut rng = TestRng::new();
        let cv_count = 10;

        let mut highway_test_harness = HighwayTestHarnessBuilder::new()
            .max_faulty_validators(3)
            .consensus_values_count(cv_count)
            .weight_limits(3, 10)
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
        let mut validators = handle.validators();

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
                // NOTE: Works only when all validators are honest and correct (no "mute" validators).
                // Validator produces two `NewVertex` type messages per round.
                // It may produce just one before lambda message is finalized.
                // Add one in case it's just one round (one consensus value) – 1 message.
                // 1/2=0 but 3/2=1 b/c of the rounding.
                let rounds_participated_in = (vertices_count as u8 + 1) / 2;

                assert_eq!(
                    rounds_participated_in, cv_count,
                    "Expected that validator={} participated in {} rounds.",
                    v_idx, cv_count
                )
            });

        assert_finalization_order(finalized_values, cv_count as usize);
    }

    #[test]
    fn liveness_test_some_mute() {
        assert!(logging::init_with_config(&LoggingConfig::new(LoggingFormat::Text, true)).is_ok());

        let mut rng = TestRng::new();
        let cv_count = 10;
        let fault_perc = 30;

        let mut highway_test_harness = HighwayTestHarnessBuilder::new()
            .max_faulty_validators(3)
            .faulty_weight_perc(fault_perc)
            .fault_type(Fault::Mute)
            .consensus_values_count(cv_count)
            .weight_limits(7, 10)
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
        let mut validators = handle.validators();

        let finalized_values: Vec<Vec<ConsensusValue>> = validators
            .map(|v| v.finalized_values().cloned().collect::<Vec<_>>())
            .collect();

        assert_finalization_order(finalized_values, cv_count as usize);
    }

    #[test]
    fn liveness_test_some_equivocate() {
        assert!(logging::init_with_config(&LoggingConfig::new(LoggingFormat::Text, true)).is_ok());

        let mut rng = TestRng::new();
        let cv_count = 10;
        let fault_perc = 10;

        let mut highway_test_harness = HighwayTestHarnessBuilder::new()
            .max_faulty_validators(3)
            .faulty_weight_perc(fault_perc)
            .fault_type(Fault::Equivocate)
            .consensus_values_count(cv_count)
            .weight_limits(7, 10)
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
        let mut validators = handle.validators();

        let finalized_values: Vec<Vec<ConsensusValue>> = validators
            .map(|v| v.finalized_values().cloned().collect::<Vec<_>>())
            .collect();

        assert_finalization_order(finalized_values, cv_count as usize);
    }
}
