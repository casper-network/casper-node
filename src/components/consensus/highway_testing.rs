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
struct Node {
    id: NodeId,
    // Whether a node should produce equivocations.
    is_faulty: bool,
}

impl Node {
    fn is_faulty(&self) -> bool {
        self.is_faulty
    }

    fn node_id(&self) -> NodeId {
        self.id
    }
}

/// An entry in the message queue o the test network.
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

struct TestHarness<M>
where
    M: PartialEq + Eq + Ord,
{
    /// Maps node IDs to actual node instances.
    nodes_map: BTreeMap<NodeId, Node>,
    /// A collection of all network messages queued up for delivery.
    msg_queue: Queue<M>,
    /// The instant the network was created.
    start_time: u64,
    /// Duration time of the test.
    duration: u64,
}

impl<M> TestHarness<M>
where
    M: PartialEq + Eq + Ord,
{
    /// Schedules a message `message` to be delivered at `delivery_time` to `recipient` node.
    fn schedule_message(
        &mut self,
        delivery_time: u64,
        recipient: NodeId,
        message: M,
    ) -> Result<(), anyhow::Error> {
        if (delivery_time > self.end_time()) {
            Err(anyhow!(
                "Tried scheduling a message at {:?} while the test run finishes at {:?}",
                delivery_time,
                self.end_time()
            ))
        } else {
            let qe = QueueEntry::new(delivery_time, recipient, message);
            self.msg_queue.push(qe);
            Ok(())
        }
    }

    /// Returns the end time of the test run.
    fn end_time(&self) -> u64 {
        self.start_time + self.duration
    }
}
