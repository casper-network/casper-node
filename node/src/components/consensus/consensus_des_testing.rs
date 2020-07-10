use anyhow::anyhow;
use rand::Rng;
use std::cmp::Ordering;
use std::{
    collections::{BTreeMap, BinaryHeap, VecDeque},
    fmt::{Debug, Display, Formatter},
    hash::Hash,
    time,
};

/// Enum defining recipients of the message.
pub(crate) enum Target {
    SingleValidator(ValidatorId),
    All,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) struct Message<M: Clone + Debug> {
    pub(crate) sender: ValidatorId,
    payload: M,
}

impl<M: Clone + Debug> Message<M> {
    pub(crate) fn new(sender: ValidatorId, payload: M) -> Self {
        Message { sender, payload }
    }

    pub(crate) fn payload(&self) -> &M {
        &self.payload
    }
}

pub(crate) struct TargetedMessage<M: Clone + Debug> {
    pub(crate) message: Message<M>,
    pub(crate) target: Target,
}

impl<M: Clone + Debug> TargetedMessage<M> {
    pub(crate) fn new(message: Message<M>, target: Target) -> Self {
        TargetedMessage { message, target }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
pub(crate) struct ValidatorId(pub(crate) u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
pub(crate) struct Instant(pub(crate) u64);

/// A validator in the test network.
pub(crate) struct Validator<C, M, D>
where
    M: Clone + Debug,
{
    pub(crate) id: ValidatorId,
    /// Whether a validator should produce equivocations.
    pub(crate) is_faulty: bool,
    /// Consensus values to be proposed.
    consensus_values: VecDeque<C>,
    /// Vector of consensus values finalized by the validator.
    finalized_values: Vec<C>,
    /// Number of finalized values.
    finalized_count: usize,
    /// Messages received by the validator.
    messages_received: Vec<Message<M>>,
    /// Messages produced by the validator.
    messages_produced: Vec<M>,
    /// An instance of consensus protocol.
    pub(crate) consensus: D,
}

impl<C, M, D> Validator<C, M, D>
where
    M: Clone + Debug,
{
    pub(crate) fn new(
        id: ValidatorId,
        is_faulty: bool,
        consensus_values: VecDeque<C>,
        consensus: D,
    ) -> Self {
        Validator {
            id,
            is_faulty,
            consensus_values,
            finalized_values: Vec::new(),
            finalized_count: 0,
            messages_received: Vec::new(),
            messages_produced: Vec::new(),
            consensus,
        }
    }

    pub(crate) fn is_faulty(&self) -> bool {
        self.is_faulty
    }

    pub(crate) fn validator_id(&self) -> ValidatorId {
        self.id
    }

    /// Adds vector of finalized consensus values to validator's finalized set.
    pub(crate) fn push_finalized(&mut self, finalized_values: Vec<C>) {
        self.finalized_count += finalized_values.len();
        self.finalized_values.extend(finalized_values);
    }

    /// Adds messages to validator's collection of received messages.
    pub(crate) fn push_messages_received(&mut self, messages: Vec<Message<M>>) {
        self.messages_received.extend(messages);
    }

    /// Adds messages to validator's collection of produced messages.
    pub(crate) fn push_messages_produced(&mut self, messages: Vec<M>) {
        self.messages_produced.extend(messages);
    }

    /// Iterator over consensus values finalized by the validator.
    pub(crate) fn finalized_values(&self) -> impl Iterator<Item = &C> {
        self.finalized_values.iter()
    }

    pub(crate) fn messages_received(&self) -> impl Iterator<Item = &Message<M>> {
        self.messages_received.iter()
    }

    pub(crate) fn messages_produced(&self) -> impl Iterator<Item = &M> {
        self.messages_produced.iter()
    }

    pub(crate) fn finalized_count(&self) -> usize {
        self.finalized_count
    }

    pub(crate) fn next_consensus_value(&mut self) -> Option<C> {
        self.consensus_values.pop_front()
    }
}

pub(crate) trait MessageT: PartialEq + Eq + Ord + Clone + Debug {}
impl<T> MessageT for T where T: PartialEq + Eq + Ord + Clone + Debug {}

/// An entry in the message queue of the test network.
#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) struct QueueEntry<M>
where
    M: MessageT,
{
    /// Scheduled delivery time of the message.
    /// When a message has dependencies that recipient validator is missing,
    /// those will be added to it in a loop (simulating synchronization)
    /// and not influence the delivery time.
    pub(crate) delivery_time: Instant,
    /// Recipient of the message.
    pub(crate) recipient: ValidatorId,
    /// The message.
    pub(crate) message: Message<M>,
}

impl<M> QueueEntry<M>
where
    M: MessageT,
{
    pub(crate) fn new(delivery_time: Instant, recipient: ValidatorId, message: Message<M>) -> Self {
        QueueEntry {
            delivery_time,
            recipient,
            message,
        }
    }
}

impl<M> Ord for QueueEntry<M>
where
    M: MessageT,
{
    fn cmp(&self, other: &Self) -> Ordering {
        self.delivery_time
            .cmp(&other.delivery_time)
            .reverse()
            .then_with(|| self.recipient.cmp(&other.recipient))
            .then_with(|| self.message.payload.cmp(&other.message.payload))
    }
}

impl<M> PartialOrd for QueueEntry<M>
where
    M: MessageT,
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod queue_entry_tests {
    use super::{Instant, Message, QueueEntry, ValidatorId};
    use std::cmp::Ordering;

    #[test]
    fn delivery_time_ord() {
        let sender = ValidatorId(2);
        let recipient1 = ValidatorId(1);
        let recipient2 = ValidatorId(3);
        let message = Message::new(sender, 1u8);
        let m1 = QueueEntry::new(Instant(1), recipient1, message.clone());
        let m2 = QueueEntry::new(Instant(2), recipient1, message.clone());
        assert_eq!(m1.cmp(&m2), Ordering::Greater);
        let m3 = QueueEntry::new(Instant(1), recipient2, message.clone());
        assert_eq!(m1.cmp(&m3), Ordering::Less);
    }
}

/// Priority queue of messages scheduled for delivery to validators.
/// Ordered by the delivery time.
pub(crate) struct Queue<M>(BinaryHeap<QueueEntry<M>>)
where
    M: MessageT;

impl<M> Default for Queue<M>
where
    M: MessageT,
{
    fn default() -> Self {
        Queue(Default::default())
    }
}

impl<M> Queue<M>
where
    M: MessageT,
{
    /// Gets next message.
    /// Returns `None` if there aren't any.
    pub(crate) fn pop(&mut self) -> Option<QueueEntry<M>> {
        self.0.pop()
    }

    /// Pushes new message to the queue.
    pub(crate) fn push(&mut self, item: QueueEntry<M>) {
        self.0.push(item)
    }
}

#[cfg(test)]
mod queue_tests {
    use super::{Instant, Message, Queue, QueueEntry, ValidatorId};

    #[test]
    fn pop_earliest_delivery() {
        let mut queue: Queue<u8> = Queue::default();
        let recipient_a = ValidatorId(1);
        let recipient_b = ValidatorId(3);
        let sender = ValidatorId(2);
        let message_a = Message::new(sender, 1u8);
        let message_b = Message::new(sender, 2u8);

        let first = QueueEntry::new(Instant(1), recipient_a, message_b);
        let second = QueueEntry::new(Instant(1), recipient_a, message_a.clone());
        let third = QueueEntry::new(Instant(3), recipient_b, message_a);

        queue.push(first.clone());
        queue.push(third.clone());
        queue.push(second.clone());

        assert_eq!(queue.pop(), Some(first));
        assert_eq!(queue.pop(), Some(second));
        assert_eq!(queue.pop(), Some(third));
    }
}

/// A trait defining strategy for randomly changing value of `i`.
///
/// Can be used to simulate network delays, message drops, invalid signatures,
/// panoramas etc.
pub(crate) trait Strategy<Item> {
    fn map<R: Rng>(&self, rng: &mut R, i: Item) -> Item {
        i
    }
}

pub(crate) enum DeliverySchedule {
    AtInstant(Instant),
    Drop,
}

impl DeliverySchedule {
    fn at(instant: Instant) -> DeliverySchedule {
        DeliverySchedule::AtInstant(instant)
    }

    fn drop(_instant: Instant) -> DeliverySchedule {
        DeliverySchedule::Drop
    }
}

impl From<Instant> for DeliverySchedule {
    fn from(instant: Instant) -> Self {
        DeliverySchedule::at(instant)
    }
}

pub(crate) struct VirtualNet<C, D, M, DS>
where
    M: MessageT,
    DS: Strategy<DeliverySchedule>,
{
    /// Maps validator IDs to actual validator instances.
    validators_map: BTreeMap<ValidatorId, Validator<C, M, D>>,
    /// A collection of all network messages queued up for delivery.
    msg_queue: Queue<M>,
    /// A strategy to pseudo randomly change the message delivery times.
    delivery_time_strategy: DS,
}

impl<C, D, M, DS> VirtualNet<C, D, M, DS>
where
    M: MessageT,
    DS: Strategy<DeliverySchedule>,
{
    pub(crate) fn new<I: IntoIterator<Item = Validator<C, M, D>>>(
        validators: I,
        delivery_time_strategy: DS,
    ) -> Self {
        let validators_map = validators
            .into_iter()
            .map(|validator| (validator.id, validator))
            .collect();

        VirtualNet {
            validators_map,
            msg_queue: Queue::default(),
            delivery_time_strategy,
        }
    }

    /// Dispatches messages to their recipients.
    pub(crate) fn dispatch_messages<R: Rng>(
        &mut self,
        rand: &mut R,
        delivery_time: Instant,
        messages: Vec<TargetedMessage<M>>,
    ) {
        for TargetedMessage { message, target } in messages {
            let recipients = match target {
                Target::All => self.validators_ids().cloned().collect(),
                Target::SingleValidator(recipient_id) => vec![recipient_id],
            };
            self.send_messages(rand, recipients, message, delivery_time)
        }
    }

    /// Pop a message from the queue.
    /// It's a message with the earliest delivery time.
    pub(crate) fn pop_message(&mut self) -> Option<QueueEntry<M>> {
        self.msg_queue.pop()
    }

    pub(crate) fn get_validator(&self, validator: ValidatorId) -> Option<&Validator<C, M, D>> {
        self.validators_map.get(&validator)
    }

    pub(crate) fn validators_ids(&self) -> impl Iterator<Item = &ValidatorId> {
        self.validators_map.keys().into_iter()
    }

    pub(crate) fn get_validator_mut(
        &mut self,
        validator_id: &ValidatorId,
    ) -> Option<&mut Validator<C, M, D>> {
        self.validators_map.get_mut(validator_id)
    }

    pub(crate) fn validator(&self, validator_id: &ValidatorId) -> Option<&Validator<C, M, D>> {
        self.validators_map.get(validator_id)
    }

    pub(crate) fn validators(&self) -> impl Iterator<Item = &Validator<C, M, D>> {
        self.validators_map.values()
    }

    // Utility function for dispatching message to multiple recipients.
    fn send_messages<R: Rng, I: IntoIterator<Item = ValidatorId>>(
        &mut self,
        rand: &mut R,
        recipients: I,
        message: Message<M>,
        base_delivery_time: Instant,
    ) {
        for validator_id in recipients {
            let tampered_delivery_time = self
                .delivery_time_strategy
                .map(rand, base_delivery_time.into());
            match tampered_delivery_time {
                // Simulates dropping of the message.
                // TODO: Add logging.
                DeliverySchedule::Drop => (),
                DeliverySchedule::AtInstant(dt) => {
                    self.schedule_message(dt, validator_id, message.clone())
                }
            }
        }
    }

    /// Schedules a message `message` to be delivered at `delivery_time` to `recipient` validator.
    fn schedule_message(
        &mut self,
        delivery_time: Instant,
        recipient: ValidatorId,
        message: Message<M>,
    ) {
        let qe = QueueEntry::new(delivery_time, recipient, message);
        self.msg_queue.push(qe);
    }
}

mod virtual_net_tests {

    use super::{
        DeliverySchedule, Instant, Message, Strategy, Target, TargetedMessage, Validator,
        ValidatorId, VirtualNet,
    };
    use rand_core::SeedableRng;
    use rand_xorshift::XorShiftRng;
    use std::collections::{HashSet, VecDeque};

    struct NoOpDelay;

    impl Strategy<DeliverySchedule> for NoOpDelay {
        fn map<R: rand::Rng>(&self, _rng: &mut R, i: DeliverySchedule) -> DeliverySchedule {
            i
        }
    }

    type M = u64;
    type C = u64;

    struct NoOpConsensus;

    #[test]
    fn messages_are_enqueued_in_order() {
        let validator_id = ValidatorId(1u64);
        let single_validator: Validator<C, u64, NoOpConsensus> =
            Validator::new(validator_id, false, VecDeque::new(), NoOpConsensus);
        let mut virtual_net = VirtualNet::new(vec![single_validator], NoOpDelay);

        let messages_num = 10;
        // We want to enqueue messages from the latest delivery time to the earliest.
        let messages: Vec<(Instant, Message<u64>)> = (0..messages_num)
            .map(|i| (Instant(messages_num - i), Message::new(validator_id, i)))
            .collect();

        messages.clone().into_iter().for_each(|(instant, message)| {
            virtual_net.schedule_message(instant, validator_id, message)
        });

        let queued_messages =
            std::iter::successors(virtual_net.pop_message(), |_| virtual_net.pop_message())
                .map(|qe| qe.message);

        // Since we enqueued in the order from the latest delivery time,
        // we expect that the actual delivery will be a reverse.
        let expected_order = messages.into_iter().map(|(_, msg)| msg).rev();

        assert!(
            queued_messages.eq(expected_order),
            "Messages were not delivered in the expected order."
        );
    }

    #[test]
    fn messages_are_dispatched() {
        let validator_id = ValidatorId(1u64);
        let first_validator: Validator<C, M, NoOpConsensus> =
            Validator::new(validator_id, false, VecDeque::new(), NoOpConsensus);
        let second_validator: Validator<C, M, NoOpConsensus> =
            Validator::new(ValidatorId(2u64), false, VecDeque::new(), NoOpConsensus);

        let mut virtual_net = VirtualNet::new(vec![first_validator, second_validator], NoOpDelay);
        let mut rand = XorShiftRng::from_seed(rand::random());

        let message = Message::new(validator_id, 1u64);
        let targeted_message = TargetedMessage::new(message.clone(), Target::All);

        virtual_net.dispatch_messages(&mut rand, Instant(2), vec![targeted_message]);

        let queued_msgs =
            std::iter::successors(virtual_net.pop_message(), |_| virtual_net.pop_message())
                .map(|qe| (qe.recipient, qe.message))
                .collect::<Vec<_>>();

        assert_eq!(
            queued_msgs,
            vec![(ValidatorId(2), message.clone()), (ValidatorId(1), message)],
            "A broadcast message should be delivered to every node."
        );
    }
}
