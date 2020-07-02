use anyhow::anyhow;
use std::cmp::Ordering;
use std::{
    collections::{BTreeMap, BinaryHeap},
    fmt::{Display, Formatter},
    hash::Hash,
    time,
};

/// Enum defining recipients of the message.
enum Target {
    SingleNode(NodeId),
    All,
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
struct Message<M: Copy + Clone> {
    sender: NodeId,
    payload: M,
}

struct TargetedMessage<M: Copy + Clone> {
    message: Message<M>,
    target: Target,
}

trait ConsensusInstance {
    type M: Clone + Copy;

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
    consensus: D,
}

impl<C, D: ConsensusInstance> Node<C, D> {
    fn new(id: NodeId, is_faulty: bool, consensus: D) -> Self {
        Node {
            id,
            is_faulty,
            finalized_values: Vec::new(),
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
}

/// An entry in the message queue of the test network.
#[derive(Debug, PartialEq, Eq)]
struct QueueEntry<M>
where
    M: PartialEq + Eq + Ord + Clone + Copy,
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
    M: PartialEq + Eq + Ord + Clone + Copy,
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
    M: PartialEq + Eq + Ord + Clone + Copy,
{
    fn cmp(&self, other: &Self) -> Ordering {
        self.delivery_time
            .cmp(&other.delivery_time)
            .then_with(|| self.recipient.cmp(&other.recipient))
            .then_with(|| self.message.payload.cmp(&other.message.payload))
    }
}

impl<M> PartialOrd for QueueEntry<M>
where
    M: PartialEq + Eq + Ord + Clone + Copy,
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Priority queue of messages scheduled for delivery to nodes.
/// Ordered by the delivery time.
struct Queue<M>(BinaryHeap<QueueEntry<M>>)
where
    M: PartialEq + Eq + Ord + Clone + Copy;

impl<M> Default for Queue<M>
where
    M: PartialEq + Eq + Ord + Clone + Copy,
{
    fn default() -> Self {
        Queue(Default::default())
    }
}

impl<M> Queue<M>
where
    M: PartialEq + Eq + Ord + Clone + Copy,
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
    M: PartialEq + Eq + Ord + Clone + Copy,
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
    M: PartialEq + Eq + Ord + Clone + Copy,
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

        for TargetedMessage { message, target } in recipient_node.consensus.handle_message(
            message.sender,
            message.payload,
            recipient_node.is_faulty(),
        ) {
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
                // Simulate droping of the message.
                // TODO: Add logging.
                DeliverySchedule::Drop => (),
                DeliverySchedule::AtInstant(dt) => self.schedule_message(dt, node_id, message),
            }
            }
        }
    }
}
