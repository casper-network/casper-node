use std::{
    collections::BTreeMap,
    fmt::{Debug, Display, Formatter},
    hash::Hash,
};

use datasize::DataSize;

use casper_types::Timestamp;

use super::queue::{MessageT, Queue, QueueEntry};

/// Enum defining recipients of the message.
#[derive(Debug)]
pub(crate) enum Target {
    SingleValidator(ValidatorId),
    AllExcept(ValidatorId),
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) struct Message<M: Clone + Debug> {
    pub(crate) sender: ValidatorId,
    pub(crate) payload: M,
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

impl<M: Debug + Clone> Debug for TargetedMessage<M> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TargetedMessage")
            .field("from", &self.message.sender)
            .field("to", &self.target)
            .field("payload", &self.message.payload)
            .finish()
    }
}

impl<M: Clone + Debug> TargetedMessage<M> {
    pub(crate) fn new(message: Message<M>, target: Target) -> Self {
        TargetedMessage { message, target }
    }
}

#[derive(Debug, Clone, DataSize, Copy, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub(crate) struct ValidatorId(pub(crate) u64);

impl Display for ValidatorId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Fault {
    /// The validator does not send any messages within the interval between the timestamps.
    TemporarilyMute { from: Timestamp, till: Timestamp },
    /// The validator does not send any messages ever.
    PermanentlyMute,
    /// The validator is actively malicious.
    Equivocate,
}

/// A validator in the test network.
#[derive(Debug)]
pub(crate) struct Node<C, M, V>
where
    M: Clone + Debug,
{
    pub(crate) id: ValidatorId,
    /// Vector of consensus values finalized by the validator.
    finalized_values: Vec<C>,
    /// Messages received by the validator.
    messages_received: Vec<Message<M>>,
    /// Messages produced by the validator.
    messages_produced: Vec<M>,
    validator: V,
}

impl<C, M, V> Node<C, M, V>
where
    M: Clone + Debug,
{
    pub(crate) fn new(id: ValidatorId, validator: V) -> Self {
        Node {
            id,
            finalized_values: Vec::new(),
            messages_received: Vec::new(),
            messages_produced: Vec::new(),
            validator,
        }
    }

    /// Adds vector of finalized consensus values to validator's finalized set.
    pub(crate) fn push_finalized(&mut self, finalized_value: C) {
        self.finalized_values.push(finalized_value);
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

    pub(crate) fn messages_produced(&self) -> impl Iterator<Item = &M> {
        self.messages_produced.iter()
    }

    pub(crate) fn finalized_count(&self) -> usize {
        self.finalized_values.len()
    }

    pub(crate) fn validator(&self) -> &V {
        &self.validator
    }

    pub(crate) fn validator_mut(&mut self) -> &mut V {
        &mut self.validator
    }
}

pub(crate) enum DeliverySchedule {
    AtInstant(Timestamp),
    #[allow(dead_code)] // Drop variant used in tests.
    Drop,
}

impl DeliverySchedule {
    fn at(instant: Timestamp) -> DeliverySchedule {
        DeliverySchedule::AtInstant(instant)
    }
}

impl From<u64> for DeliverySchedule {
    fn from(instant: u64) -> Self {
        DeliverySchedule::at(instant.into())
    }
}

impl From<Timestamp> for DeliverySchedule {
    fn from(timestamp: Timestamp) -> Self {
        DeliverySchedule::at(timestamp)
    }
}

pub(crate) struct VirtualNet<C, M, V>
where
    M: MessageT,
{
    /// Maps validator IDs to actual validator instances.
    validators_map: BTreeMap<ValidatorId, Node<C, M, V>>,
    /// A collection of all network messages queued up for delivery.
    msg_queue: Queue<M>,
}

impl<C, M, V> VirtualNet<C, M, V>
where
    M: MessageT,
{
    pub(crate) fn new<I: IntoIterator<Item = Node<C, M, V>>>(
        validators: I,
        init_messages: Vec<QueueEntry<M>>,
    ) -> Self {
        let validators_map = validators
            .into_iter()
            .map(|validator| (validator.id, validator))
            .collect();

        let mut q = Queue::default();
        for m in init_messages.into_iter() {
            q.push(m);
        }

        VirtualNet {
            validators_map,
            msg_queue: q,
        }
    }

    /// Dispatches messages to their recipients.
    pub(crate) fn dispatch_messages(&mut self, messages: Vec<(TargetedMessage<M>, Timestamp)>) {
        for (TargetedMessage { message, target }, delivery_time) in messages {
            let recipients = match target {
                Target::AllExcept(creator) => self
                    .validators_ids()
                    .filter(|id| **id != creator)
                    .cloned()
                    .collect(),
                Target::SingleValidator(recipient_id) => vec![recipient_id],
            };
            self.send_messages(recipients, message, delivery_time)
        }
    }

    /// Pop a message from the queue.
    /// It's a message with the earliest delivery time.
    pub(crate) fn pop_message(&mut self) -> Option<QueueEntry<M>> {
        self.msg_queue.pop()
    }

    /// Returns a reference to the next message from the queue without removing it.
    /// It's a message with the earliest delivery time.
    pub(crate) fn peek_message(&self) -> Option<&QueueEntry<M>> {
        self.msg_queue.peek()
    }

    pub(crate) fn validators_ids(&self) -> impl Iterator<Item = &ValidatorId> {
        self.validators_map.keys()
    }

    pub(crate) fn node_mut(&mut self, validator_id: &ValidatorId) -> Option<&mut Node<C, M, V>> {
        self.validators_map.get_mut(validator_id)
    }

    pub(crate) fn validator(&self, validator_id: &ValidatorId) -> Option<&Node<C, M, V>> {
        self.validators_map.get(validator_id)
    }

    pub(crate) fn validators(&self) -> impl Iterator<Item = &Node<C, M, V>> {
        self.validators_map.values()
    }

    // Utility function for dispatching message to multiple recipients.
    fn send_messages<I: IntoIterator<Item = ValidatorId>>(
        &mut self,
        recipients: I,
        message: Message<M>,
        delivery_time: Timestamp,
    ) {
        for validator_id in recipients {
            self.schedule_message(delivery_time, validator_id, message.clone())
        }
    }

    /// Schedules a message `message` to be delivered at `delivery_time` to `recipient` validator.
    fn schedule_message(
        &mut self,
        delivery_time: Timestamp,
        recipient: ValidatorId,
        message: Message<M>,
    ) {
        let qe = QueueEntry::new(delivery_time, recipient, message);
        self.msg_queue.push(qe);
    }

    /// Drops all messages from the queue.
    /// Should never be called during normal operation of the test.
    pub(crate) fn empty_queue(&mut self) {
        self.msg_queue.clear();
    }
}

mod virtual_net_tests {
    use super::{Message, Node, Target, TargetedMessage, Timestamp, ValidatorId, VirtualNet};

    type M = u64;
    type C = u64;

    struct NoOpValidator;

    #[test]
    fn messages_are_enqueued_in_order() {
        let validator_id = ValidatorId(1u64);
        let single_validator: Node<C, u64, NoOpValidator> = Node::new(validator_id, NoOpValidator);
        let mut virtual_net = VirtualNet::new(vec![single_validator], vec![]);

        let messages_num = 10;
        // We want to enqueue messages from the latest delivery time to the earliest.
        let messages: Vec<(Timestamp, Message<u64>)> = (0..messages_num)
            .map(|i| ((messages_num - i).into(), Message::new(validator_id, i)))
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
        let a: Node<C, M, NoOpValidator> = Node::new(validator_id, NoOpValidator);
        let b = Node::new(ValidatorId(2u64), NoOpValidator);
        let c = Node::new(ValidatorId(3u64), NoOpValidator);

        let mut virtual_net = VirtualNet::new(vec![a, b, c], vec![]);

        let message = Message::new(validator_id, 1u64);
        let targeted_message =
            TargetedMessage::new(message.clone(), Target::AllExcept(validator_id));

        virtual_net.dispatch_messages(vec![(targeted_message, 2.into())]);

        let queued_msgs =
            std::iter::successors(virtual_net.pop_message(), |_| virtual_net.pop_message())
                .map(|qe| (qe.recipient, qe.message))
                .collect::<Vec<_>>();

        assert_eq!(
            queued_msgs,
            vec![(ValidatorId(3), message.clone()), (ValidatorId(2), message)],
            "A broadcast message should be delivered to every node but the creator."
        );
    }
}
