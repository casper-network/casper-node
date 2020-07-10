use super::{
    active_validator::Effect,
    evidence::Evidence,
    finality_detector::{FinalityDetector, FinalityResult},
    highway::{Highway, VertexError},
    validators::ValidatorIndex,
    vertex::{Dependency, Vertex},
};
use crate::{
    components::consensus::{
        consensus_des_testing::{
            DeliverySchedule, Instant, Message, MessageT, QueueEntry, Strategy, Target,
            TargetedMessage, ValidatorId, VirtualNet,
        },
        traits::Context,
        BlockContext,
    },
    types::Timestamp,
};

struct HighwayConsensus<C: Context> {
    highway: Highway<C>,
    finality_detector: FinalityDetector<C>,
}

impl<C: Context> HighwayConsensus<C> {
    fn run_finality(&mut self) -> FinalityResult<C::ConsensusValue, ValidatorIndex> {
        self.finality_detector.run(self.highway.state())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum HighwayMessage<C: Context> {
    Timer(Instant),
    NewVertex(Vertex<C>),
    RequestBlock(BlockContext),
}

impl<C: Context> HighwayMessage<C> {
    fn into_targeted(self, creator: ValidatorId) -> TargetedMessage<HighwayMessage<C>> {
        let create_msg = |hwm: HighwayMessage<C>| Message::new(creator, hwm);

        match self {
            Timer(_) => TargetedMessage::new(create_msg(self), Target::SingleValidator(creator)),
            NewVertex(_) => TargetedMessage::new(create_msg(self), Target::All),
            RequestBlock(_) => {
                TargetedMessage::new(create_msg(self), Target::SingleValidator(creator))
            }
        }
    }
}

use HighwayMessage::*;

impl<C: Context> From<Effect<C>> for HighwayMessage<C> {
    fn from(eff: Effect<C>) -> Self {
        match eff {
            Effect::NewVertex(v) => NewVertex(v),
            Effect::ScheduleTimer(t) => Timer(Instant(t.millis())),
            Effect::RequestNewBlock(block_context) => RequestBlock(block_context),
        }
    }
}

use rand::Rng;
use std::fmt::{Display, Formatter};

impl<C: Context> PartialOrd for HighwayMessage<C> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(&other))
    }
}

impl<C: Context> Ord for HighwayMessage<C> {
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
enum CrankOk {
    /// Test run is not done.
    Continue,
    /// Test run is finished.
    Done,
}

#[derive(Debug, Eq, PartialEq)]
enum TestRunError<C: Context> {
    /// VirtualNet was missing a validator when it was expected to exist.
    MissingValidator(ValidatorId),
    /// Sender sent a vertex for which it didn't have all dependencies.
    SenderMissingDependency(ValidatorId, Dependency<C>),
    /// No more messages in the message queue.
    NoMessages,
    /// Validator run out of consensus values to propose before the end of a test.
    NoConsensusValues(ValidatorId),
}

impl<C: Context> Display for TestRunError<C> {
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
            TestRunError::NoConsensusValues(id) => {
                write!(f, "{:?} has no consensus values to propose.", id)
            }
        }
    }
}

pub(crate) struct HighwayTestHarness<M, C, D, DS>
where
    M: MessageT,
    DS: Strategy<DeliverySchedule>,
    C: Context,
{
    virtual_net: VirtualNet<C::ConsensusValue, D, M, DS>,
    /// The instant the network was created.
    start_time: u64,
    /// Consensus values to be proposed.
    /// Order of values in the vector defines the order in which they will be proposed.
    consensus_values: Vec<C::ConsensusValue>,
}

impl<Ctx, DS> HighwayTestHarness<HighwayMessage<Ctx>, Ctx, HighwayConsensus<Ctx>, DS>
where
    Ctx: Context,
    DS: Strategy<DeliverySchedule>,
{
    fn new(
        virtual_net: VirtualNet<
            Ctx::ConsensusValue,
            HighwayConsensus<Ctx>,
            HighwayMessage<Ctx>,
            DS,
        >,
        start_time: u64,
        consensus_values: Vec<Ctx::ConsensusValue>,
    ) -> Self {
        HighwayTestHarness {
            virtual_net,
            start_time,
            consensus_values,
        }
    }

    /// Advance the test by one message.
    ///
    /// Pops one message from the message queue (if there are any)
    /// and pass it to the recipient validator for execution.
    /// Messages returned from the execution are scheduled for later delivery.
    pub(crate) fn crank<R: Rng>(&mut self, rand: &mut R) -> Result<CrankOk, TestRunError<Ctx>> {
        // Stop the test when each node finalized all consensus values.
        // Note that we're not testing the order of finalization here.
        // TODO: Consider moving out all the assertions to client side.
        if self
            .virtual_net
            .validators()
            .all(|v| v.finalized_count() == self.consensus_values.len())
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
            .map(|hwm| hwm.into_targeted(recipient))
            .collect();

        self.virtual_net
            .dispatch_messages(rand, delivery_time, targeted_messages);

        Ok(CrankOk::Continue)
    }

    fn mut_handle(&mut self) -> &mut Self {
        self
    }

    /// Processes a message sent to `validator_id`.
    /// Returns a vector of messages produced by the `validator` in reaction to processing a message.
    fn process_message(
        &mut self,
        validator_id: ValidatorId,
        message: Message<HighwayMessage<Ctx>>,
    ) -> Result<Vec<HighwayMessage<Ctx>>, TestRunError<Ctx>> {
        // Using the same `recipient` instance in the whole method body is not possible due to compiler
        // complaining about using `self` as mutable and immutable.
        {
            let recipient = self
                .virtual_net
                .get_validator_mut(&validator_id)
                .ok_or_else(|| TestRunError::MissingValidator(validator_id))?;
            recipient.push_messages_received(vec![message.clone()]);
        }

        let messages = {
            let sender_id = message.sender;
            let recipient = self
                .virtual_net
                .get_validator_mut(&validator_id)
                .ok_or_else(|| TestRunError::MissingValidator(validator_id))?;

            let recipient_id = recipient.id;
            let hwm = message.payload().clone();
            match hwm {
                Timer(instant) => recipient
                    .consensus
                    .highway
                    .handle_timer(Timestamp::zero() + instant.0.into())
                    .into_iter()
                    .map(HighwayMessage::from)
                    .collect(),
                NewVertex(v) => {
                    match self.add_vertex(recipient_id, sender_id, v)? {
                        Ok(msgs) => msgs,
                        Err((v, error)) => {
                            // TODO: Replace with tracing library and maybe add to sender state?
                            println!("{:?} sent an invalid vertex {:?} to {:?} that resulted in {:?} error", sender_id, v, recipient_id, error);
                            vec![]
                        }
                    }
                }
                RequestBlock(block_context) => {
                    let consensus_value = recipient
                        .next_consensus_value()
                        .ok_or_else(|| TestRunError::NoConsensusValues(recipient_id))?;

                    recipient
                        .consensus
                        .highway
                        .propose(consensus_value, block_context)
                        .into_iter()
                        .map(HighwayMessage::from)
                        .collect()
                }
            }
        };

        let recipient = self
            .virtual_net
            .get_validator_mut(&validator_id)
            .ok_or_else(|| TestRunError::MissingValidator(validator_id))?;
        recipient.push_messages_produced(messages.clone());

        let finality_result = match recipient.consensus.run_finality() {
            FinalityResult::Finalized(v, equivocated_ids) => {
                if !equivocated_ids.is_empty() {
                    unimplemented!("Equivocations detected but not handled.")
                }
                vec![v]
            }
            FinalityResult::None => vec![],
            FinalityResult::FttExceeded => unimplemented!("Ftt exceeded but not handled."),
        };

        recipient.push_finalized(finality_result);

        Ok(messages)
    }

    // Adds vertex to the validator's state.
    // Synchronizes its state if necessary.
    fn add_vertex(
        &mut self,
        recipient: ValidatorId,
        sender: ValidatorId,
        vertex: Vertex<Ctx>,
    ) -> Result<Result<Vec<HighwayMessage<Ctx>>, (Vertex<Ctx>, VertexError)>, TestRunError<Ctx>>
    {
        // 1. pre_validate_vertex
        // 2. missing_dependency
        // 3. validate_vertex
        // 4. add_valid_vertex

        let sync_result = {
            let validator = self
                .virtual_net
                .get_validator_mut(&recipient)
                .ok_or_else(|| TestRunError::MissingValidator(recipient))?;

            match validator.consensus.highway.pre_validate_vertex(vertex) {
                Err((v, error)) => Ok(Err((v, error))),
                Ok(pvv) => match validator.consensus.highway.missing_dependency(&pvv) {
                    None => Ok(Ok((pvv, vec![]))),
                    Some(d) => self
                        .synchronize_validator(d, recipient, sender)
                        .map(|r| r.map(|hwm| (pvv, hwm))),
                },
            }
        }?;

        match sync_result {
            Err(vertex_error) => Ok(Err(vertex_error)),
            Ok((prevalidated_vertex, mut sync_effects)) => {
                let add_vertex_effects: Vec<HighwayMessage<Ctx>> = {
                    let validator = self
                        .virtual_net
                        .get_validator_mut(&recipient)
                        .ok_or_else(|| TestRunError::MissingValidator(recipient))?;

                    match validator
                        .consensus
                        .highway
                        .validate_vertex(prevalidated_vertex)
                        .map_err(|(pvv, error)| (pvv.into_vertex(), error))
                    {
                        Err(vertex_error) => return Ok(Err(vertex_error)),
                        Ok(valid_vertex) => validator
                            .consensus
                            .highway
                            .add_valid_vertex(valid_vertex)
                            .into_iter()
                            .map(HighwayMessage::from)
                            .collect(),
                    }
                };

                sync_effects.extend(add_vertex_effects);

                Ok(Ok(sync_effects))
            }
        }
    }

    // Synchronizes `validator` in case of missing dependencies.
    //
    // If validator has missing dependencies then we have to add them first.
    // We don't want to test synchronization, and the Highway theory assumes
    // that when votes are added then all their dependencies are satisfied.
    fn synchronize_validator(
        &mut self,
        missing_dependency: Dependency<Ctx>,
        recipient: ValidatorId,
        sender: ValidatorId,
    ) -> Result<Result<Vec<HighwayMessage<Ctx>>, (Vertex<Ctx>, VertexError)>, TestRunError<Ctx>>
    {
        let senders_state = self
            .virtual_net
            .get_validator_mut(&sender)
            .ok_or_else(|| TestRunError::MissingValidator(sender))?
            .consensus
            .highway
            .state();

        let vertex = match missing_dependency {
            Dependency::Vote(ref hash) => {
                let swvote = senders_state.wire_vote(&hash).ok_or_else(|| {
                    TestRunError::SenderMissingDependency(sender, missing_dependency)
                })?;
                Vertex::Vote(swvote)
            }
            Dependency::Evidence(ref idx) => {
                let evidence = senders_state.opt_evidence(*idx).cloned().ok_or_else(|| {
                    TestRunError::SenderMissingDependency(sender, missing_dependency)
                })?;

                Vertex::Evidence(evidence.clone())
            }
        };

        self.add_vertex(recipient, sender, vertex)
    }
}

mod test_harness {
    use super::{DeliverySchedule, Instant, Strategy};

    struct SmallDelay();

    impl Strategy<DeliverySchedule> for SmallDelay {
        fn map<R: rand::Rng>(&self, _rng: &mut R, i: DeliverySchedule) -> DeliverySchedule {
            match i {
                DeliverySchedule::Drop => DeliverySchedule::Drop,
                DeliverySchedule::AtInstant(instant) => {
                    DeliverySchedule::AtInstant(Instant(instant.0 + 1))
                }
            }
        }
    }

    #[test]
    fn on_empty_queue_error() {
        unimplemented!()
    }

    #[test]
    fn done_when_all_finalized() {
        unimplemented!()
    }
}
