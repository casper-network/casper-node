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
        tests::consensus_des_testing::{
            DeliverySchedule, Message, Target, TargetedMessage, Validator, ValidatorId, VirtualNet,
        },
        tests::queue::{MessageT, QueueEntry},
        traits::{ConsensusValueT, Context},
        BlockContext,
    },
    types::Timestamp,
};

use std::iter::FromIterator;

struct HighwayConsensus {
    highway: Highway<TestContext>,
    finality_detector: FinalityDetector<TestContext>,
}

impl HighwayConsensus {
    fn run_finality(
        &mut self,
    ) -> FinalityOutcome<<TestContext as Context>::ConsensusValue, ValidatorIndex> {
        self.finality_detector.run(self.highway.state())
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
            Timer(_) => TargetedMessage::new(create_msg(self), Target::SingleValidator(creator)),
            NewVertex(_) => TargetedMessage::new(create_msg(self), Target::AllExcept(creator)),
            RequestBlock(_) => {
                TargetedMessage::new(create_msg(self), Target::SingleValidator(creator))
            }
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
    collections::{HashMap, VecDeque},
    fmt::{Debug, Display, Formatter},
    marker::PhantomData,
};
use test_harness::TestContext;

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
        &self,
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
    virtual_net:
        VirtualNet<<TestContext as Context>::ConsensusValue, HighwayConsensus, HighwayMessage>,
    /// The instant the network was created.
    start_time: Timestamp,
    /// Consensus values to be proposed.
    /// Order of values in the vector defines the order in which they will be proposed.
    consensus_values: VecDeque<<TestContext as Context>::ConsensusValue>,
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
        virtual_net: VirtualNet<
            <TestContext as Context>::ConsensusValue,
            HighwayConsensus,
            HighwayMessage,
        >,
        start_time: T,
        consensus_values: VecDeque<<TestContext as Context>::ConsensusValue>,
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
        // Stop the test when each node finalized all consensus values.
        // Note that we're not testing the order of finalization here.
        // TODO: Consider moving out all the assertions to client side.
        if self
            .virtual_net
            .validators()
            .all(|v| v.finalized_count() == self.consensus_values_num)
        {
            return Ok(CrankOk::Done);
        }

        let QueueEntry {
            delivery_time,
            recipient,
            message,
        } = self
            .virtual_net
            .pop_message()
            .ok_or(TestRunError::NoMessages)?;

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
                DeliverySchedule::Drop => None,
                DeliverySchedule::AtInstant(timestamp) => {
                    Some((hwm.into_targeted(recipient), timestamp))
                }
            })
            .collect();

        self.virtual_net.dispatch_messages(targeted_messages);

        Ok(CrankOk::Continue)
    }

    fn next_consensus_value(&mut self) -> Option<<TestContext as Context>::ConsensusValue> {
        self.consensus_values.pop_front()
    }

    /// Helper for getting validator from the underlying virtual net.
    #[allow(clippy::type_complexity)]
    fn validator_mut(
        &mut self,
        validator_id: &ValidatorId,
    ) -> Result<
        &mut Validator<<TestContext as Context>::ConsensusValue, HighwayMessage, HighwayConsensus>,
        TestRunError,
    > {
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
        let res = f(self.validator_mut(validator_id)?.consensus.highway_mut());
        let mut additional_effects = vec![];
        for e in res.iter() {
            if let Effect::NewVertex(vv) = e {
                additional_effects.extend(
                    self.validator_mut(validator_id)?
                        .consensus
                        .highway_mut()
                        .add_valid_vertex(vv.clone()),
                );
            }
        }
        additional_effects.extend(res);
        Ok(additional_effects
            .into_iter()
            .map(HighwayMessage::from)
            .collect())
    }

    /// Processes a message sent to `validator_id`.
    /// Returns a vector of messages produced by the `validator` in reaction to processing a message.
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
                    match self.add_vertex(validator_id, sender_id, v)? {
                        Ok(msgs) => msgs,
                        Err((v, error)) => {
                            // TODO: Replace with tracing library and maybe add to sender state?
                            println!("{:?} sent an invalid vertex {:?} to {:?} that resulted in {:?} error", sender_id, v, validator_id, error);
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
                timestamp,
            } => {
                if !new_equivocators.is_empty() {
                    // https://casperlabs.atlassian.net/browse/HWY-120
                    unimplemented!("Equivocations detected but not handled.")
                }
                vec![value]
            }
            FinalityOutcome::None => vec![],
            // https://casperlabs.atlassian.net/browse/HWY-119
            FinalityOutcome::FttExceeded => unimplemented!("Ftt exceeded but not handled."),
        };

        recipient.push_finalized(finality_result);

        Ok(messages)
    }

    // Adds vertex to the validator's state.
    // Synchronizes its state if necessary.
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
}

enum BuilderError {
    NoValidators,
    EmptyConsensusValues,
    WeightLimits,
    TooManyFaultyNodes(String),
    EmptyFtt,
}

struct HighwayTestHarnessBuilder<DS: DeliveryStrategy> {
    /// Validators (together with their secret keys) in the test run.
    validators_secs:
        HashMap<<TestContext as Context>::ValidatorId, <TestContext as Context>::ValidatorSecret>,
    /// Percentage of faulty validators' (i.e. equivocators) weight.
    /// Defaults to 0 (network is perfectly secure).
    faulty_weight: u64,
    /// FTT value for the finality detector.
    /// If not given, defaults to 1/3 of total validators' weight.
    ftt: Option<u64>,
    /// Consensus values to be proposed by the nodes in the network.
    consensus_values: Option<VecDeque<<TestContext as Context>::ConsensusValue>>,
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
        &self,
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
            validators_secs: HashMap::new(),
            faulty_weight: 0,
            ftt: None,
            consensus_values: None,
            delivery_distribution: Distribution::Uniform,
            delivery_strategy: InstantDeliveryNoDropping,
            weight_limits: (0, 0),
            start_time: Timestamp::zero(),
            weight_distribution: Distribution::Uniform,
            seed: 0,
            round_exp: 12,
        }
    }
}

impl<DS: DeliveryStrategy> HighwayTestHarnessBuilder<DS> {
    pub(crate) fn validators(
        mut self,
        validators_secs: HashMap<
            <TestContext as Context>::ValidatorId,
            <TestContext as Context>::ValidatorSecret,
        >,
    ) -> Self {
        self.validators_secs = validators_secs;
        self
    }

    /// Sets a percentage of weight that will be assigned to malicious nodes.
    /// `faulty_weight` must be a value between 0 (inclusive) and 100 (inclusive).
    pub(crate) fn faulty_weight(mut self, faulty_weight: u64) -> Self {
        assert!(
            faulty_weight <= 33,
            "Expected value between 0 (inclusive) and 33 (inclusive)"
        );
        self.faulty_weight = faulty_weight;
        self
    }

    pub(crate) fn consensus_values(
        mut self,
        cv: Vec<<TestContext as Context>::ConsensusValue>,
    ) -> Self {
        assert!(!cv.is_empty());
        self.consensus_values = Some(VecDeque::from(cv));
        self
    }

    pub(crate) fn delivery_strategy<DS2: DeliveryStrategy>(
        self,
        ds: DS2,
    ) -> HighwayTestHarnessBuilder<DS2> {
        HighwayTestHarnessBuilder {
            validators_secs: self.validators_secs,
            faulty_weight: self.faulty_weight,
            ftt: self.ftt,
            consensus_values: self.consensus_values,
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
        self.ftt = Some(ftt);
        self
    }

    fn build<R: Rng>(mut self, rng: &mut R) -> Result<HighwayTestHarness<DS>, BuilderError> {
        if self.validators_secs.is_empty() {
            return Err(BuilderError::NoValidators);
        }

        let validators_num = self.validators_secs.len() as u8;

        let consensus_values = self
            .consensus_values
            .clone()
            .ok_or_else(|| BuilderError::EmptyConsensusValues)?;

        let instance_id = 0;
        let seed = self.seed;
        let round_exp = self.round_exp;
        let start_time = self.start_time;

        let (lower, upper) = {
            let (l, u) = self.weight_limits;
            // `rng.gen_range(x, y)` panics if `x >= y`
            // and since `safe_ftt` is calculated as: `(weight_sum - 1) / 3`
            // it may panic on `x` < 7 b/c it will round down to 1.
            if (l >= u) || l < 7 {
                return Err(BuilderError::WeightLimits);
            }
            (l, u)
        };

        let (faulty_weights, honest_weights): (Vec<Weight>, Vec<Weight>) = {
            if (self.faulty_weight > 0 && validators_num == 1) {
                return Err(BuilderError::TooManyFaultyNodes(
                    "Network has only 1 validator, it cannot be malicious. \
                        Provide more validators or set `fauluty_weight` to 0."
                        .to_string(),
                ));
            }

            if (self.faulty_weight == 0) {
                // All validators are honest.
                let honest_validators: Vec<Weight> = self
                    .weight_distribution
                    .gen_range_vec(rng, lower, upper, validators_num)
                    .into_iter()
                    .map(|w| Weight(w))
                    .collect();

                (vec![], honest_validators)
            } else {
                // At least 2 validators with some level of faults.
                let honest_num = rng.gen_range(1, validators_num);
                let faulty_num = validators_num - honest_num;

                assert!(
                    faulty_num > 0,
                    "Expected that at least one validator to be malicious."
                );

                let honest_weights = self
                    .weight_distribution
                    .gen_range_vec(rng, lower, upper, honest_num);

                let faulty_weights: Vec<u64> = {
                    // Weight of all malicious validators.
                    let mut weight_limit: u64 =
                        (self.faulty_weight / 100u64) * honest_weights.iter().sum::<u64>();
                    let mut validators_left = faulty_num;
                    let mut weights: Vec<u64> = vec![];
                    // Generate weight as long as there are empty validator slots and there's a weight left.
                    while validators_left > 0 {
                        if validators_left == 1 {
                            weights.push(weight_limit);
                        } else {
                            let weight: u64 = rng.gen_range(lower, weight_limit);
                            weight_limit -= weight;
                            weights.push(weight);
                        }
                        validators_left -= 1;
                    }
                    weights
                };

                (
                    faulty_weights.into_iter().map(Weight).collect(),
                    honest_weights.into_iter().map(Weight).collect(),
                )
            }
        };

        let mut validator_ids = (0..validators_num).map(|i| ValidatorId(i as u64));

        let weights_sum = faulty_weights
            .iter()
            .chain(honest_weights.iter())
            .sum::<Weight>();

        let faulty_validators = validator_ids
            .by_ref()
            .take(faulty_weights.len())
            .zip(faulty_weights)
            .collect::<Vec<_>>();

        let honest_validators = validator_ids
            .by_ref()
            .take(honest_weights.len())
            .zip(honest_weights)
            .collect::<Vec<_>>();

        // Sanity check.
        assert_eq!(
            faulty_validators.len() + honest_validators.len(),
            validators_num as usize,
        );

        let validators: Validators<ValidatorId> = Validators::from_iter(
            faulty_validators
                .clone()
                .into_iter()
                .chain(honest_validators.clone().into_iter()),
        );

        let ftt = self.ftt.unwrap_or_else(|| (weights_sum.0 - 1) / 3);

        // Local function creating an instance of `HighwayConsensus` for a single validator.
        let highway_consensus = |(vid, secrets): (
            <TestContext as Context>::ValidatorId,
            &mut HashMap<
                <TestContext as Context>::ValidatorId,
                <TestContext as Context>::ValidatorSecret,
            >,
        )| {
            let v_sec = secrets.remove(&vid).expect("Secret key should exist.");

            let (highway, effects) = {
                let highway_params: HighwayParams<TestContext> = HighwayParams {
                    instance_id: instance_id.clone(),
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

        let (validators, init_messages) = {
            let mut validators = vec![];
            let mut init_messages = vec![];

            for (vid, is_faulty) in faulty_validators
                .iter()
                .map(|(vid, _)| (*vid, true))
                .chain(honest_validators.iter().map(|(vid, _)| (*vid, false)))
            {
                let (consensus, msgs) = highway_consensus((vid, &mut self.validators_secs));
                let validator = Validator::new(vid, is_faulty, consensus);
                let qm: Vec<QueueEntry<HighwayMessage>> = msgs
                    .into_iter()
                    .map(|hwm| {
                        // These are messages crated on the start of the network.
                        // They are sent from validator to himself.
                        QueueEntry::new(self.start_time, vid, Message::new(vid, hwm))
                    })
                    .collect();
                init_messages.extend(qm);
                validators.push(validator);
            }

            (validators, init_messages)
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

mod test_harness {
    use super::{
        CrankOk, DeliverySchedule, DeliveryStrategy, HighwayMessage, HighwayTestHarness,
        HighwayTestHarnessBuilder, InstantDeliveryNoDropping, TestRunError,
    };
    use crate::{
        components::consensus::{
            tests::consensus_des_testing::ValidatorId,
            traits::{Context, ValidatorSecret},
        },
        types::TimeDiff,
    };
    use rand_core::SeedableRng;
    use rand_xorshift::XorShiftRng;
    use std::{
        collections::{hash_map::DefaultHasher, HashMap},
        hash::Hasher,
    };

    #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub(crate) struct TestContext;

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub(crate) struct TestSecret(pub(crate) u32);

    impl ValidatorSecret for TestSecret {
        type Hash = u64;
        type Signature = u64;

        fn sign(&self, data: &Self::Hash) -> Self::Signature {
            data + u64::from(self.0)
        }
    }

    impl Context for TestContext {
        type ConsensusValue = u32;
        type ValidatorId = ValidatorId;
        type ValidatorSecret = TestSecret;
        type Signature = u64;
        type Hash = u64;
        type InstanceId = u64;

        fn hash(data: &[u8]) -> Self::Hash {
            let mut hasher = DefaultHasher::new();
            hasher.write(data);
            hasher.finish()
        }

        fn verify_signature(
            hash: &Self::Hash,
            public_key: &Self::ValidatorId,
            signature: &<Self::ValidatorSecret as ValidatorSecret>::Signature,
        ) -> bool {
            let computed_signature = hash + public_key.0;
            computed_signature == *signature
        }
    }

    #[test]
    fn on_empty_queue_error() {
        let mut rand = XorShiftRng::from_seed(rand::random());

        let mut highway_test_harness: HighwayTestHarness<InstantDeliveryNoDropping> =
            HighwayTestHarnessBuilder::new()
                .consensus_values(vec![1])
                .weight_limits(7, 10)
                .build(&mut rand)
                .ok()
                .expect("Construction was successful");

        highway_test_harness.mutable_handle().clear_message_queue();

        assert_eq!(
            highway_test_harness.crank(&mut rand),
            Err(TestRunError::NoMessages),
            "Expected the test run to stop."
        );
    }

    #[test]
    fn done_when_all_finalized() -> Result<(), TestRunError> {
        let mut rand = XorShiftRng::from_seed(rand::random());
        let mut highway_test_harness: HighwayTestHarness<InstantDeliveryNoDropping> =
            HighwayTestHarnessBuilder::new()
                .consensus_values((0..10).collect())
                .weight_limits(7, 10)
                .build(&mut rand)
                .ok()
                .expect("Construction was successful");

        loop {
            let crank_res = highway_test_harness.crank(&mut rand)?;
            match crank_res {
                CrankOk::Continue => continue,
                CrankOk::Done => break,
            }
        }
        Ok(())
    }
}
