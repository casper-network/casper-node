use super::consensus_des_testing::{Message, ValidatorId};
use std::{cmp::Ordering, collections::BinaryHeap, fmt::Debug};

use casper_types::Timestamp;

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
    pub(crate) delivery_time: Timestamp,
    /// Recipient of the message.
    pub(crate) recipient: ValidatorId,
    /// The message.
    pub(crate) message: Message<M>,
}

impl<M> QueueEntry<M>
where
    M: MessageT,
{
    pub(crate) fn new(
        delivery_time: Timestamp,
        recipient: ValidatorId,
        message: Message<M>,
    ) -> Self {
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
    use super::{Message, QueueEntry, ValidatorId};
    use std::cmp::Ordering;

    #[test]
    fn delivery_time_ord() {
        let sender = ValidatorId(2);
        let recipient1 = ValidatorId(1);
        let recipient2 = ValidatorId(3);
        let message = Message::new(sender, 1u8);
        let m1 = QueueEntry::new(1.into(), recipient1, message.clone());
        let m2 = QueueEntry::new(2.into(), recipient1, message.clone());
        assert_eq!(m1.cmp(&m2), Ordering::Greater);
        let m3 = QueueEntry::new(1.into(), recipient2, message);
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

    /// Returns a reference to next message without removing it.
    /// Returns `None` if there aren't any.
    pub(crate) fn peek(&self) -> Option<&QueueEntry<M>> {
        self.0.peek()
    }

    /// Pushes new message to the queue.
    pub(crate) fn push(&mut self, item: QueueEntry<M>) {
        self.0.push(item)
    }

    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod queue_tests {
    use super::{Message, Queue, QueueEntry, ValidatorId};

    #[test]
    fn pop_earliest_delivery() {
        let mut queue: Queue<u8> = Queue::default();
        let recipient_a = ValidatorId(1);
        let recipient_b = ValidatorId(3);
        let sender = ValidatorId(2);
        let message_a = Message::new(sender, 1u8);
        let message_b = Message::new(sender, 2u8);

        let first = QueueEntry::new(1.into(), recipient_a, message_b);
        let second = QueueEntry::new(1.into(), recipient_a, message_a.clone());
        let third = QueueEntry::new(3.into(), recipient_b, message_a);

        queue.push(first.clone());
        queue.push(third.clone());
        queue.push(second.clone());

        assert_eq!(queue.pop(), Some(first));
        assert_eq!(queue.pop(), Some(second));
        assert_eq!(queue.pop(), Some(third));
    }
}
