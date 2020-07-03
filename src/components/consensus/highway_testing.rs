use anyhow::anyhow;
use std::cmp::Ordering;
use std::{
    collections::{BTreeMap, BinaryHeap},
    fmt::{Debug, Display, Formatter},
    hash::Hash,
    time,
};

/// Enum defining recipients of the message.
enum Target {
    SingleNode(NodeId),
    All,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
struct Message<M: Copy + Clone + Debug> {
    sender: NodeId,
    payload: M,
}

impl<M: Copy + Clone + Debug> Message<M> {
    fn new(sender: NodeId, payload: M) -> Self {
        Message { sender, payload }
    }
}

struct TargetedMessage<M: Copy + Clone + Debug> {
    message: Message<M>,
    target: Target,
}

impl<M: Copy + Clone + Debug> TargetedMessage<M> {
    fn new(message: Message<M>, target: Target) -> Self {
        TargetedMessage { message, target }
    }
}

trait ConsensusInstance {
    type M: Clone + Copy + Debug;

    fn handle_message(
        &mut self,
        sender: NodeId,
        m: Self::M,
        is_faulty: bool,
    ) -> Vec<TargetedMessage<Self::M>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
struct NodeId(u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
struct Instant(u64);

/// A node in the test network.
struct Node<C, D: ConsensusInstance> {
    id: NodeId,
    /// Whether a node should produce equivocations.
    is_faulty: bool,
    /// Vector of consensus values finalized by the node.
    finalized_values: Vec<C>,
    /// Messages received by the node.
    messages_received: Vec<Message<D::M>>,
    /// Messages produced by the node.
    messages_produced: Vec<Message<D::M>>,
    /// An instance of consensus protocol.
    consensus: D,
}

impl<C, D: ConsensusInstance> Node<C, D> {
    fn new(id: NodeId, is_faulty: bool, consensus: D) -> Self {
        Node {
            id,
            is_faulty,
            finalized_values: Vec::new(),
            messages_received: Vec::new(),
            messages_produced: Vec::new(),
            consensus,
        }
    }

    fn is_faulty(&self) -> bool {
        self.is_faulty
    }

    fn node_id(&self) -> NodeId {
        self.id
    }

    /// Iterator over consensus values finalized by the node.
    fn finalized_values(&self) -> impl Iterator<Item = &C> {
        self.finalized_values.iter()
    }

    fn messages_received(&self) -> impl Iterator<Item = &Message<D::M>> {
        self.messages_received.iter()
    }

    fn messages_produced(&self) -> impl Iterator<Item = &Message<D::M>> {
        self.messages_produced.iter()
    }

    fn handle_message(&mut self, sender: NodeId, m: D::M) -> Vec<TargetedMessage<D::M>> {
        self.messages_received.push(Message::new(sender, m));
        let outband_msgs = self.consensus.handle_message(sender, m, self.is_faulty);
        outband_msgs
            .iter()
            .map(|tm| tm.message)
            .for_each(|message| self.messages_produced.push(message));
        outband_msgs
    }
}

trait MessageT: PartialEq + Eq + Ord + Clone + Copy + Debug {}
impl<T> MessageT for T where T: PartialEq + Eq + Ord + Clone + Copy + Debug {}

/// An entry in the message queue of the test network.
#[derive(Debug, PartialEq, Eq, Clone)]
struct QueueEntry<M>
where
    M: MessageT,
{
    /// Scheduled delivery time of the message.
    /// When a message has dependencies that recipient node is missing,
    /// those will be added to it in a loop (simulating synchronization)
    /// and not influence the delivery time.
    delivery_time: Instant,
    /// Recipient of the message.
    recipient: NodeId,
    /// The message.
    message: Message<M>,
}

impl<M> QueueEntry<M>
where
    M: MessageT,
{
    pub(crate) fn new(delivery_time: Instant, recipient: NodeId, message: Message<M>) -> Self {
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
    use super::{Instant, Message, NodeId, QueueEntry};
    use std::cmp::Ordering;

    #[test]
    fn delivery_time_ord() {
        let sender = NodeId(2);
        let recipient1 = NodeId(1);
        let recipient2 = NodeId(3);
        let message = Message::new(sender, 1u8);
        let m1 = QueueEntry::new(Instant(1), recipient1, message);
        let m2 = QueueEntry::new(Instant(2), recipient1, message);
        assert_eq!(m1.cmp(&m2), Ordering::Less);
        let m3 = QueueEntry::new(Instant(1), recipient2, message);
        assert_eq!(m1.cmp(&m3), Ordering::Less);
    }
}

/// Priority queue of messages scheduled for delivery to nodes.
/// Ordered by the delivery time.
struct Queue<M>(BinaryHeap<QueueEntry<M>>)
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
    fn pop(&mut self) -> Option<QueueEntry<M>> {
        self.0.pop()
    }

    /// Pushes new message to the queue.
    fn push(&mut self, item: QueueEntry<M>) {
        self.0.push(item)
    }
}

#[cfg(test)]
mod queue_tests {
    use super::{Instant, Message, NodeId, Queue, QueueEntry};

    #[test]
    fn pop_earliest_delivery() {
        let mut queue: Queue<u8> = Queue::default();
        let recipient_a = NodeId(1);
        let recipient_b = NodeId(3);
        let sender = NodeId(2);
        let message_a = Message::new(sender, 1u8);
        let message_b = Message::new(sender, 2u8);

        let first = QueueEntry::new(Instant(1), recipient_a, message_a);
        let second = QueueEntry::new(Instant(1), recipient_a, message_b);
        let third = QueueEntry::new(Instant(3), recipient_b, message_a);

        queue.push(first.clone());
        queue.push(third.clone());
        queue.push(second.clone());

        assert_eq!(queue.pop(), Some(first));
        assert_eq!(queue.pop(), Some(second));
        assert_eq!(queue.pop(), Some(third));
    }
}

trait Strategy<Item> {
    fn map<R: rand::Rng>(&self, rng: &mut R, i: Item) -> Item {
        i
    }
}

enum DeliverySchedule {
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

#[derive(Debug, Eq, PartialEq)]
enum TestRunError {
    MissingRecipient(NodeId),
    NoMessages,
}

impl Display for TestRunError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TestRunError::MissingRecipient(node_id) => {
                write!(f, "Recipient node {:?} was not found in the map.", node_id)
            }
            TestRunError::NoMessages => write!(
                f,
                "Test finished prematurely due to lack of messages in the queue"
            ),
        }
    }
}

struct TestHarness<M, C, D, DS, R>
where
    M: MessageT,
    D: ConsensusInstance,
    DS: Strategy<DeliverySchedule>,
{
    /// Maps node IDs to actual node instances.
    nodes_map: BTreeMap<NodeId, Node<C, D>>,
    /// A collection of all network messages queued up for delivery.
    msg_queue: Queue<M>,
    /// The instant the network was created.
    start_time: u64,
    /// Consensus values to be proposed.
    /// Order of values in the vector defines the order in which they will be proposed.
    consensus_values: Vec<C>,
    delivery_time_strategy: DS,
    rand: R,
}

impl<M, C, D, DS, R> TestHarness<M, C, D, DS, R>
where
    M: MessageT,
    D: ConsensusInstance<M = M>,
    DS: Strategy<DeliverySchedule>,
    R: rand::Rng,
{
    fn new<I: IntoIterator<Item = Node<C, D>>>(
        nodes: I,
        start_time: u64,
        consensus_values: Vec<C>,
        delivery_time_strategy: DS,
        rand: R,
    ) -> Self {
        let nodes_map = nodes.into_iter().map(|node| (node.id, node)).collect();
        TestHarness {
            nodes_map,
            msg_queue: Default::default(),
            start_time,
            consensus_values,
            delivery_time_strategy,
            rand,
        }
    }

    /// Schedules a message `message` to be delivered at `delivery_time` to `recipient` node.
    fn schedule_message(&mut self, delivery_time: Instant, recipient: NodeId, message: Message<M>) {
        let qe = QueueEntry::new(delivery_time, recipient, message);
        self.msg_queue.push(qe);
    }

    /// Advance the test by one message.
    ///
    /// Pops one message from the message queue (if there are any)
    /// and pass it to the recipient node for execution.
    /// Messages returned from the execution are scheduled for later delivery.
    fn crank(&mut self) -> Result<(), TestRunError> {
        let QueueEntry {
            delivery_time,
            recipient,
            message,
        } = self.msg_queue.pop().ok_or(TestRunError::NoMessages)?;
        // TODO: Check if we should stop the test.
        // Verify whether all nodes have finalized all consensus values.
        let mut recipient_node = self
            .nodes_map
            .get_mut(&recipient)
            .ok_or(TestRunError::MissingRecipient(recipient))?;

        for TargetedMessage { message, target } in
            recipient_node.handle_message(message.sender, message.payload)
        {
            let recipient_nodes = match target {
                Target::All => self.nodes_map.keys().cloned().collect(),
                Target::SingleNode(recipient_id) => vec![recipient_id],
            };
            self.send_messages(recipient_nodes, message, delivery_time)
        }
        Ok(())
    }

    // Utility function for dispatching message to multiple recipients.
    fn send_messages<I: IntoIterator<Item = NodeId>>(
        &mut self,
        recipients: I,
        message: Message<M>,
        base_delivery_time: Instant,
    ) {
        for node_id in recipients {
            let tampered_delivery_time = self
                .delivery_time_strategy
                .map(&mut self.rand, base_delivery_time.into());
            match tampered_delivery_time {
                // Simulates droping of the message.
                // TODO: Add logging.
                DeliverySchedule::Drop => (),
                DeliverySchedule::AtInstant(dt) => self.schedule_message(dt, node_id, message),
            }
        }
    }

    fn nodes(&self) -> impl Iterator<Item = &Node<C, D>> {
        self.nodes_map.values()
    }

    fn mut_handle(&mut self) -> &mut Self {
        self
    }
}

#[cfg(test)]
mod test_harness {
    use super::{
        ConsensusInstance, DeliverySchedule, Instant, Message, Node, NodeId, Strategy, Target,
        TargetedMessage, TestHarness, TestRunError,
    };
    use rand_core::SeedableRng;
    use rand_xorshift::XorShiftRng;

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

    type M = u64;
    type C = u64;

    struct NoOpConsensus();

    impl ConsensusInstance for NoOpConsensus {
        type M = M;
        fn handle_message(
            &mut self,
            sender: NodeId,
            m: Self::M,
            is_faulty: bool,
        ) -> Vec<TargetedMessage<Self::M>> {
            vec![]
        }
    }

    #[test]
    fn on_empty_queue_error() {
        let single_node: Node<C, NoOpConsensus> = Node::new(NodeId(1u64), false, NoOpConsensus());
        let mut rand = XorShiftRng::from_seed(rand::random());
        let mut test_harness: TestHarness<M, C, NoOpConsensus, SmallDelay, XorShiftRng> =
            TestHarness::new(vec![single_node], 0, vec![], SmallDelay(), rand);
        assert_eq!(test_harness.crank(), Err(TestRunError::NoMessages));
    }

    #[test]
    fn messages_are_delivered_in_order() {
        let node_id = NodeId(1u64);
        let single_node = Node::new(node_id, false, NoOpConsensus());
        let mut rand = XorShiftRng::from_seed(rand::random());
        let mut test_harness: TestHarness<M, C, NoOpConsensus, SmallDelay, XorShiftRng> =
            TestHarness::new(vec![single_node], 0, vec![], SmallDelay(), rand);

        let messages_num = 10;
        // We want to enqueue messages from the latest delivery time to the earliest.
        let messages: Vec<(Instant, Message<u64>)> = (0..messages_num)
            .map(|i| (Instant(messages_num - i), Message::new(node_id, i)))
            .collect();

        let last_message_payload = messages.last().cloned().unwrap().1;

        messages.into_iter().for_each(|(instant, message)| {
            test_harness.schedule_message(instant, node_id, message)
        });

        let mut crank_count = 0;
        let mut previous_payload = last_message_payload.payload;
        while test_harness.crank().is_ok() {
            let new_message = test_harness
                .mut_handle()
                .nodes()
                .next()
                .unwrap()
                .messages_received()
                .next()
                .unwrap();

            let new_payload = new_message.payload;
            assert_eq!(
                new_payload, previous_payload,
                "Messages were not delivered in the expected order."
            );
            previous_payload = new_payload;
            crank_count += 1;
        }

        assert_eq!(
            crank_count, messages_num,
            "There was more messages in the network than scheduled initially."
        )
    }

    struct ForwardAllConsensus;

    impl ConsensusInstance for ForwardAllConsensus {
        type M = M;
        fn handle_message(
            &mut self,
            sender: NodeId,
            m: Self::M,
            is_faulty: bool,
        ) -> Vec<TargetedMessage<Self::M>> {
            vec![TargetedMessage::new(Message::new(sender, m), Target::All)]
        }
    }

    #[test]
    fn messages_are_broadcasted() {
        let node_count = 10;
        let nodes: Vec<Node<C, ForwardAllConsensus>> = (0u64..node_count)
            .map(|id| Node::new(NodeId(id), false, ForwardAllConsensus))
            .collect();
        let mut rand = XorShiftRng::from_seed(rand::random());
        let mut test_harness: TestHarness<M, C, ForwardAllConsensus, SmallDelay, XorShiftRng> =
            TestHarness::new(nodes, 0, vec![], SmallDelay(), rand);

        let test_message = Message::new(NodeId(1), 1u64);
        test_harness.schedule_message(Instant(1), NodeId(0), test_message);
        // Fist crank to deliver the first message.
        // As a result of processing it, 1 message will be delivered to each node.
        assert!(test_harness.crank().is_ok());
        // We need to crank the network as many times as there are nodes so that everyone gets their response message.
        // That's b/c 1 crank == 1 popped from the queue and delivered to the node.
        (0..node_count).for_each(|_| assert!(test_harness.crank().is_ok()));
        assert!(test_harness
            .mut_handle()
            .nodes()
            .all(|node| node.messages_received().next() == Some(&test_message)));
    }
}
