use anyhow::anyhow;
use std::cmp::Ordering;
use std::{
    collections::{BTreeMap, BinaryHeap},
    hash::Hash,
    time,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
struct NodeId(u64);
/// A node in the test network.
struct Node<C> {
    id: NodeId,
    /// Whether a node should produce equivocations.
    is_faulty: bool,
    /// Vector of consensus values finalized by the node.
    finalized_values: Vec<C>,
}

impl<C> Node<C> {
    fn new(id: NodeId, is_faulty: bool) -> Self {
        Node {
            id,
            is_faulty,
            finalized_values: Vec::new(),
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
    M: PartialEq + Eq + Ord,
{
    /// Scheduled delivery time of the message.
    /// When a message has dependencies that recipient node is missing,
    /// those will be added to it in a loop (simulating synchronization)
    /// and not influence the delivery time.
    delivery_time: u64,
    /// Recipient of the message.
    recipient: NodeId,
    /// The message.
    message: M,
}

impl<M> QueueEntry<M>
where
    M: PartialEq + Eq + Ord,
{
    pub(crate) fn new(delivery_time: u64, recipient: NodeId, message: M) -> Self {
        QueueEntry {
            delivery_time,
            recipient,
            message,
        }
    }
}

impl<M> Ord for QueueEntry<M>
where
    M: PartialEq + Eq + Ord,
{
    fn cmp(&self, other: &Self) -> Ordering {
        self.delivery_time
            .cmp(&other.delivery_time)
            .then_with(|| self.recipient.cmp(&other.recipient))
            .then_with(|| self.message.cmp(&other.message))
    }
}

impl<M> PartialOrd for QueueEntry<M>
where
    M: PartialEq + Eq + Ord,
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Priority queue of messages scheduled for delivery to nodes.
/// Ordered by the delivery time.
struct Queue<M>(BinaryHeap<QueueEntry<M>>)
where
    M: PartialEq + Eq + Ord;

impl<M> Default for Queue<M>
where
    M: PartialEq + Eq + Ord,
{
    fn default() -> Self {
        Queue(Default::default())
    }
}

impl<M> Queue<M>
where
    M: PartialEq + Eq + Ord,
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

struct TestHarness<M, C>
where
    M: PartialEq + Eq + Ord,
{
    /// Maps node IDs to actual node instances.
    nodes_map: BTreeMap<NodeId, Node<C>>,
    /// A collection of all network messages queued up for delivery.
    msg_queue: Queue<M>,
    /// The instant the network was created.
    start_time: u64,
    /// Consensus values to be proposed.
    /// Order of values in the vector defines the order in which they will be proposed.
    consensus_values: Vec<C>,
}

impl<M, C> TestHarness<M, C>
where
    M: PartialEq + Eq + Ord,
{
    fn new<I: IntoIterator<Item = Node<C>>>(
        nodes: I,
        start_time: u64,
        consensus_values: Vec<C>,
    ) -> Self {
        let nodes_map = nodes.into_iter().map(|node| (node.id, node)).collect();
        TestHarness {
            nodes_map,
            msg_queue: Default::default(),
            start_time,
            consensus_values,
        }
    }

    /// Schedules a message `message` to be delivered at `delivery_time` to `recipient` node.
    fn schedule_message(
        &mut self,
        delivery_time: u64,
        recipient: NodeId,
        message: M,
    ) -> Result<(), anyhow::Error> {
        let qe = QueueEntry::new(delivery_time, recipient, message);
        self.msg_queue.push(qe);
        Ok(())
    }
}
