use super::{
    active_validator::Effect,
    block::Block,
    evidence::Evidence,
    finality_detector::{FinalityDetector, FinalityResult},
    highway::{Highway, VertexError},
    state::State,
    validators::ValidatorIndex,
    vertex::{Dependency, Vertex},
    vote::Vote,
};
use crate::components::consensus::{
    consensus_des_testing::{
        DeliverySchedule, Instant, Message, MessageT, Queue, QueueEntry, Strategy, Target,
        TargetedMessage, Validator, ValidatorId, VirtualNet,
    },
    traits::Context,
    BlockContext,
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
            Effect::ScheduleTimer(t) => Timer(Instant(t)),
            Effect::RequestNewBlock(block_context) => RequestBlock(block_context),
        }
    }
}

use rand::Rng;
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    fmt::{Display, Formatter},
};

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
    SenderMissingDependency(ValidatorId, Dependency<C>),
    NoMessages,
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
        }
    }
}

pub(crate) struct HighwayTestHarness<M, C, D, DS, R>
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
    // TODO: Move from constructor to method argument.
    rand: R,
}

impl<Ctx, DS, R> HighwayTestHarness<HighwayMessage<Ctx>, Ctx, HighwayConsensus<Ctx>, DS, R>
where
    Ctx: Context,
    DS: Strategy<DeliverySchedule>,
    R: Rng,
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
        rand: R,
    ) -> Self {
        HighwayTestHarness {
            virtual_net,
            start_time,
            consensus_values,
            rand,
        }
    }

    /// Advance the test by one message.
    ///
    /// Pops one message from the message queue (if there are any)
    /// and pass it to the recipient validator for execution.
    /// Messages returned from the execution are scheduled for later delivery.
    pub(crate) fn crank(&mut self) -> Result<CrankOk, TestRunError<Ctx>> {
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
            .dispatch_messages(&mut self.rand, delivery_time, targeted_messages);

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
        let recipient = self
            .virtual_net
            .get_validator_mut(&validator_id)
            .ok_or_else(|| TestRunError::MissingValidator(validator_id))?;
        recipient.push_messages_received(vec![message.clone()]);

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
                    .handle_timer(instant.0)
                    .into_iter()
                    .map(HighwayMessage::from)
                    .collect(),
                NewVertex(v) => self.add_vertex(recipient_id, sender_id, v)?,
                RequestBlock(block_context) => {
                    let consensus_value = recipient.next_consensus_value().unwrap();
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
    ) -> Result<Vec<HighwayMessage<Ctx>>, TestRunError<Ctx>> {
        // 1. pre_validate_vertex
        // 2. missing_dependency
        // 3. validate_vertex
        // 4. add_valid_vertex

        let (prevalidated_vertex, missing) = {
            let validator = self
                .virtual_net
                .get_validator_mut(&recipient)
                .ok_or_else(|| TestRunError::MissingValidator(recipient))?;
            let prevalidated_vertex = validator
                .consensus
                .highway
                .pre_validate_vertex(vertex)
                .unwrap(); // TODO: Do not unwrap

            let missing_dependency = validator
                .consensus
                .highway
                .missing_dependency(&prevalidated_vertex);

            (prevalidated_vertex, missing_dependency)
        };

        let mut sync_effects = self.synchronize_validator(missing, recipient, sender)?;

        let add_vertex_effects: Vec<HighwayMessage<Ctx>> = {
            let validator = self
                .virtual_net
                .get_validator_mut(&recipient)
                .ok_or_else(|| TestRunError::MissingValidator(recipient))?;

            let valid_vertex = validator
                .consensus
                .highway
                .validate_vertex(prevalidated_vertex)
                .unwrap(); // TODO: Do not unwrap

            validator
                .consensus
                .highway
                .add_valid_vertex(valid_vertex)
                .into_iter()
                .map(HighwayMessage::from)
                .collect()
        };

        sync_effects.extend(add_vertex_effects);

        Ok(sync_effects)
    }

    // Synchronizes `validator` in case of missing dependencies.
    //
    // If validator has missing dependencies then we have to add them first.
    // We don't want to test synchronization, and the Highway theory assumes
    // that when votes are added then all their dependencies are satisfied.
    fn synchronize_validator(
        &mut self,
        missing_dependency: Option<Dependency<Ctx>>,
        recipient: ValidatorId,
        sender: ValidatorId,
    ) -> Result<Vec<HighwayMessage<Ctx>>, TestRunError<Ctx>> {
        if let Some(dependency) = missing_dependency {
            let senders_state = self
                .virtual_net
                .get_validator_mut(&sender)
                .unwrap()
                .consensus
                .highway
                .state();

            let vertex = match dependency {
                Dependency::Vote(ref hash) => {
                    let swvote = senders_state
                        .wire_vote(&hash)
                        .ok_or_else(|| TestRunError::SenderMissingDependency(sender, dependency))?;
                    Vertex::Vote(swvote)
                }
                Dependency::Evidence(ref idx) => {
                    let evidence = senders_state
                        .opt_evidence(*idx)
                        .cloned()
                        .ok_or_else(|| TestRunError::SenderMissingDependency(sender, dependency))?;

                    Vertex::Evidence(evidence.clone())
                }
            };

            self.add_vertex(recipient, sender, vertex)
        } else {
            Ok(vec![])
        }
    }
}

mod test_harness {
    use super::{
        CrankOk, DeliverySchedule, HighwayMessage, HighwayTestHarness, Instant, Message, Strategy,
        Target, TargetedMessage, TestRunError, Validator, ValidatorId,
    };
    use crate::components::consensus::{
        consensus_des_testing::VirtualNet, highway_core::state::tests::TestContext,
    };
    use rand_core::SeedableRng;
    use rand_xorshift::XorShiftRng;
    use std::collections::VecDeque;

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
