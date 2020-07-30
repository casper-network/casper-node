use super::{
    active_validator::Effect,
    evidence::Evidence,
    finality_detector::{FinalityDetector, FinalityOutcome},
    highway::{Highway, HighwayParams, PreValidatedVertex, ValidVertex, VertexError},
    validators::{ValidatorIndex, Validators},
    vertex::{Dependency, Vertex},
    Weight,
};

use crate::{
    components::consensus::{
        tests::{
            consensus_des_testing::{
                DeliverySchedule, FaultType, Message, Target, TargetedMessage, Validator,
                ValidatorId, VirtualNet,
            },
            queue::{MessageT, QueueEntry},
        },
        traits::{ConsensusValueT, Context, ValidatorSecret},
        BlockContext,
    },
    types::Timestamp,
};

use serde::{Deserialize, Serialize};
use std::iter::FromIterator;
use tracing::{error, info, trace, warn};

struct HighwayConsensus {
    highway: Highway<TestContext>,
    finality_detector: FinalityDetector<TestContext>,
}

type ConsensusValue = u32;

impl HighwayConsensus {
    fn run_finality(&mut self) -> FinalityOutcome<TestContext> {
        self.finality_detector.run(&self.highway)
    }

    pub(crate) fn highway(&self) -> &Highway<TestContext> {
        &self.highway
    }

    pub(crate) fn highway_mut(&mut self) -> &mut Highway<TestContext> {
        &mut self.highway
    }
}

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
    /// We run out of consensus values to propose before the end of a test.
    NoConsensusValues,
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
            TestRunError::NoConsensusValues => write!(f, "No consensus values to propose."),
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

struct HighwayTestHarness<DS>
where
    DS: DeliveryStrategy,
{
    virtual_net: VirtualNet<ConsensusValue, HighwayConsensus, HighwayMessage>,
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

impl<DS> HighwayTestHarness<DS>
where
    DS: DeliveryStrategy,
{
    fn new<T: Into<Timestamp>>(
        virtual_net: VirtualNet<ConsensusValue, HighwayConsensus, HighwayMessage>,
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
    pub(crate) fn crank<R: Rng>(&mut self, rand: &mut R) -> Result<CrankOk, TestRunError> {
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

        let messages = self.process_message(recipient, message)?;

        let targeted_messages = messages
            .into_iter()
            .map(|hwm| {
                let delivery = self.delivery_time_strategy.gen_delay(
                    rand,
                    &hwm,
                    &self.delivery_time_distribution,
                    delivery_time,
                );
                (hwm, delivery)
            })
            .filter_map(|(hwm, delivery)| match delivery {
                DeliverySchedule::Drop => {
                    trace!("{:?} message is dropped.", hwm);
                    None
                }
                DeliverySchedule::AtInstant(timestamp) => {
                    trace!("{:?} scheduled for {:?}", hwm, timestamp);
                    let targeted = hwm.into_targeted(recipient);
                    Some((targeted, timestamp))
                }
            })
            .collect();

        self.virtual_net.dispatch_messages(targeted_messages);

        // Stop the test when each node finalized all consensus values.
        // Note that we're not testing the order of finalization here.
        // TODO: Consider moving out all the assertions to client side.
        if self
            .virtual_net
            .validators()
            .all(|v| v.finalized_count() == self.consensus_values_num)
        {
            return Ok(CrankOk::Done);
        } else {
            Ok(CrankOk::Continue)
        }
    }

    fn next_consensus_value(&mut self) -> Option<ConsensusValue> {
        self.consensus_values.pop_front()
    }

    /// Helper for getting validator from the underlying virtual net.
    #[allow(clippy::type_complexity)]
    fn validator_mut(
        &mut self,
        validator_id: &ValidatorId,
    ) -> Result<&mut Validator<ConsensusValue, HighwayMessage, HighwayConsensus>, TestRunError>
    {
        self.virtual_net
            .validator_mut(&validator_id)
            .ok_or_else(|| TestRunError::MissingValidator(*validator_id))
    }

    fn call_validator<F>(
        &mut self,
        validator_id: &ValidatorId,
        f: F,
    ) -> Result<Vec<HighwayMessage>, TestRunError>
    where
        F: FnOnce(&mut Highway<TestContext>) -> Vec<Effect<TestContext>>,
    {
        let validator = self.validator_mut(validator_id)?;
        let res = f(validator.consensus.highway_mut());
        let mut effects = vec![];
        for e in res.into_iter() {
            // If validator produced a `NewVertex` effect,
            // we want to add it to his state immediately.
            if let Effect::NewVertex(vv) = &e {
                effects.extend(
                    validator
                        .consensus
                        .highway_mut()
                        .add_valid_vertex(vv.clone()),
                );
            }

            match validator.fault() {
                None => effects.push(e),
                Some(FaultType::Mute) => {
                    // If validator is `FaultType::Mute` it shouldn't send a `NewVertex` in response.
                    // Other effects – like `Timer` or `RequestBlock` should still be returned and scheduled.
                    match e {
                        Effect::NewVertex(ValidVertex(v)) => {
                            warn!("{:} is mute. {:?} won't be gossiped.", validator.id, v)
                        }
                        Effect::ScheduleTimer(_) | Effect::RequestNewBlock(_) => effects.push(e),
                    }
                }
                Some(e) => unimplemented!("{:?} is not handled.", e),
            }
        }
        Ok(effects.into_iter().map(HighwayMessage::from).collect())
    }

    /// Processes a message sent to `validator_id`.
    /// Returns a vector of messages produced by the `validator` in reaction to processing a
    /// message.
    fn process_message(
        &mut self,
        validator_id: ValidatorId,
        message: Message<HighwayMessage>,
    ) -> Result<Vec<HighwayMessage>, TestRunError> {
        self.validator_mut(&validator_id)?
            .push_messages_received(vec![message.clone()]);

        let messages = {
            let sender_id = message.sender;

            let hwm = message.payload().clone();
            match hwm {
                Timer(timestamp) => self
                    .call_validator(&validator_id, |consensus| consensus.handle_timer(timestamp))?,

                NewVertex(v) => {
                    match self.add_vertex(validator_id, sender_id, v.clone())? {
                        Ok(msgs) => {
                            trace!("{:?} successfuly added to the state.", v);
                            msgs
                        }
                        Err((v, error)) => {
                            // TODO: Replace with tracing library and maybe add to sender state?
                            error!("{:?} sent an invalid vertex {:?} to {:?} that resulted in {:?} error", sender_id, v, validator_id, error);
                            vec![]
                        }
                    }
                }
                RequestBlock(block_context) => {
                    let consensus_value = self
                        .next_consensus_value()
                        .ok_or_else(|| TestRunError::NoConsensusValues)?;

                    self.call_validator(&validator_id, |consensus| {
                        consensus.propose(consensus_value, block_context)
                    })?
                }
            }
        };

        let recipient = self.validator_mut(&validator_id)?;
        recipient.push_messages_produced(messages.clone());

        let finality_result = match recipient.consensus.run_finality() {
            FinalityOutcome::Finalized {
                value,
                new_equivocators,
                rewards,
                timestamp,
            } => {
                if !new_equivocators.is_empty() {
                    trace!("New equivocators detected: {:?}", new_equivocators);
                    // https://casperlabs.atlassian.net/browse/HWY-120
                    unimplemented!("Equivocations detected but not handled.")
                }
                if !rewards.is_empty() {
                    trace!("Rewards are not verified yet: {:?}", rewards);
                }
                trace!("Consensus value finalized: {:?}", value);
                vec![value]
            }
            FinalityOutcome::None => vec![],
            // https://casperlabs.atlassian.net/browse/HWY-119
            FinalityOutcome::FttExceeded => unimplemented!("Ftt exceeded but not handled."),
        };

        recipient.push_finalized(finality_result);

        Ok(messages)
    }

    // Adds vertex to the `recipient` validator state.
    // Synchronizes its state if necessary.
    // From the POV of the test system, synchronization is immediate.
    #[allow(clippy::type_complexity)]
    fn add_vertex(
        &mut self,
        recipient: ValidatorId,
        sender: ValidatorId,
        vertex: Vertex<TestContext>,
    ) -> Result<Result<Vec<HighwayMessage>, (Vertex<TestContext>, VertexError)>, TestRunError> {
        // 1. pre_validate_vertex
        // 2. missing_dependency
        // 3. validate_vertex
        // 4. add_valid_vertex

        let sync_result = {
            let validator = self.validator_mut(&recipient)?;

            match validator.consensus.highway.pre_validate_vertex(vertex) {
                Err((v, error)) => Ok(Err((v, error))),
                Ok(pvv) => self.synchronize_validator(recipient, sender, pvv),
            }
        }?;

        match sync_result {
            Err(vertex_error) => Ok(Err(vertex_error)),
            Ok((prevalidated_vertex, mut sync_effects)) => {
                let add_vertex_effects: Vec<HighwayMessage> = {
                    match self
                        .validator_mut(&recipient)?
                        .consensus
                        .highway
                        .validate_vertex(prevalidated_vertex)
                        .map_err(|(pvv, error)| (pvv.into_vertex(), error))
                    {
                        Err(vertex_error) => return Ok(Err(vertex_error)),
                        Ok(valid_vertex) => self.call_validator(&recipient, |highway| {
                            highway.add_valid_vertex(valid_vertex)
                        })?,
                    }
                };

                sync_effects.extend(add_vertex_effects);

                Ok(Ok(sync_effects))
            }
        }
    }

    /// Synchronizes missing dependencies of `pvv` that `recipient` is missing.
    /// If an error occurs during synchronization of one of `pvv`'s dependencies
    /// it's returned and the original vertex mustn't be added to the state.
    #[allow(clippy::type_complexity)]
    fn synchronize_validator(
        &mut self,
        recipient: ValidatorId,
        sender: ValidatorId,
        pvv: PreValidatedVertex<TestContext>,
    ) -> Result<
        Result<
            (PreValidatedVertex<TestContext>, Vec<HighwayMessage>),
            (Vertex<TestContext>, VertexError),
        >,
        TestRunError,
    > {
        let mut hwms: Vec<HighwayMessage> = vec![];

        loop {
            let validator = self
                .virtual_net
                .validator_mut(&recipient)
                .ok_or_else(|| TestRunError::MissingValidator(recipient))?;

            match validator.consensus.highway.missing_dependency(&pvv) {
                None => return Ok(Ok((pvv, hwms))),
                Some(d) => match self.synchronize_dependency(d, recipient, sender)? {
                    Ok(hwm) => {
                        // `hwm` represent messages produced while synchronizing `d`.
                        hwms.extend(hwm)
                    }
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
    fn synchronize_dependency(
        &mut self,
        missing_dependency: Dependency<TestContext>,
        recipient: ValidatorId,
        sender: ValidatorId,
    ) -> Result<Result<Vec<HighwayMessage>, (Vertex<TestContext>, VertexError)>, TestRunError> {
        let vertex = self
            .validator_mut(&sender)?
            .consensus
            .highway
            .get_dependency(&missing_dependency)
            .map(|vv| vv.0)
            .ok_or_else(|| TestRunError::SenderMissingDependency(sender, missing_dependency))?;

        self.add_vertex(recipient, sender, vertex)
    }

    /// Returns a `MutableHandle` on the `HighwayTestHarness` object
    /// that allows for manipulating internal state of the test state.
    fn mutable_handle(&mut self) -> MutableHandle<DS> {
        MutableHandle(self)
    }
}

struct MutableHandle<'a, DS: DeliveryStrategy>(&'a mut HighwayTestHarness<DS>);

impl<'a, DS: DeliveryStrategy> MutableHandle<'a, DS> {
    /// Drops all messages from the queue.
    fn clear_message_queue(&mut self) {
        self.0.virtual_net.empty_queue();
    }

    fn validators(
        &self,
    ) -> impl Iterator<Item = &Validator<ConsensusValue, HighwayMessage, HighwayConsensus>> {
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
        let consensus_values = (0..self.consensus_values_count as u32).collect::<VecDeque<u32>>();

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

                let faulty_weights = self
                    .weight_distribution
                    .gen_range_vec(rng, lower, upper, faulty_num);

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
            validators
                .enumerate()
                .map(|(_, v)| v)
                .cloned()
                .collect::<Vec<_>>()
        );

        let mut secrets = validators
            .enumerate()
            .map(|(_, vid)| (*vid.id(), TestSecret(vid.id().0)))
            .collect();

        let ftt = self
            .ftt
            .map(|p| p * weights_sum.0 / 100)
            .unwrap_or_else(|| (weights_sum.0 - 1) / 3);

        // Local function creating an instance of `HighwayConsensus` for a single validator.
        let highway_consensus =
            |(vid, secrets): (ValidatorId, &mut HashMap<ValidatorId, TestSecret>)| {
                let v_sec = secrets.remove(&vid).expect("Secret key should exist.");

                let (highway, effects) = {
                    let highway_params: HighwayParams<TestContext> = HighwayParams {
                        instance_id,
                        validators: validators.clone(),
                    };

                    Highway::new(highway_params, seed, vid, v_sec, round_exp, start_time)
                };

                let finality_detector = FinalityDetector::new(Weight(ftt));

                (
                    HighwayConsensus {
                        highway,
                        finality_detector,
                    },
                    effects
                        .into_iter()
                        .map(HighwayMessage::from)
                        .collect::<Vec<_>>(),
                )
            };

        let faulty_num = faulty_weights.len();

        let (validators, init_messages) = {
            let mut validators_loc = vec![];
            let mut init_messages = vec![];

            for (_, validator) in validators.enumerate() {
                let vid = *validator.id();
                let fault = if (vid.0 < faulty_num as u64) {
                    Some(FaultType::Mute)
                } else {
                    None
                };
                let (consensus, msgs) = highway_consensus((vid, &mut secrets));
                let validator = Validator::new(vid, fault, consensus);
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
    type ConsensusValue = u32;
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

impl Validator<ConsensusValue, HighwayMessage, HighwayConsensus> {
    fn rounds_participated_in(&self) -> u8 {
        // Validator produces two `NewVertex` type messages per round.
        // It may produce just one before lambda message is finalized.
        let vertices_count = self
            .messages_produced()
            .cloned()
            .filter(|hwm| hwm.is_new_vertex())
            .collect::<Vec<_>>()
            .len();
        // Add one in case it's just one round – 1 message.
        // 1/2=0 but 3/2=1 b/c of the rounding.
        (vertices_count as u8 + 1) / 2
    }
}

mod test_harness {
    use std::{
        collections::{hash_map::DefaultHasher, HashMap, HashSet},
        hash::Hasher,
    };

    use super::{
        CrankOk, DeliverySchedule, DeliveryStrategy, HighwayMessage, HighwayTestHarness,
        HighwayTestHarnessBuilder, InstantDeliveryNoDropping, TestRunError,
    };
    use crate::{
        components::consensus::{
            highway_core::{validators::ValidatorIndex, vertex::Vertex},
            tests::consensus_des_testing::ValidatorId,
            traits::{Context, ValidatorSecret},
        },
        logging,
        testing::TestRng,
        types::TimeDiff,
    };
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

    #[test]
    fn liveness_test_no_faults() {
        logging::init_params(true).ok();

        let mut rng = TestRng::new();
        let cv_count = 3;

        let mut highway_test_harness = HighwayTestHarnessBuilder::new()
            .max_faulty_validators(3)
            .consensus_values_count(cv_count)
            .weight_limits(3, 10)
            .build(&mut rng)
            .ok()
            .expect("Construction was successful");

        loop {
            let crank_res = highway_test_harness.crank(&mut rng).unwrap();
            match crank_res {
                CrankOk::Continue => continue,
                CrankOk::Done => break,
            }
        }

        highway_test_harness
            .mutable_handle()
            .validators()
            .filter(|v| v.fault().is_none())
            .for_each(|v| {
                assert_eq!(
                    v.rounds_participated_in(),
                    cv_count,
                    "Expected that validator={} participated in {} rounds.",
                    v.id,
                    cv_count
                )
            });
    }
}
