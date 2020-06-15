use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, VecDeque},
    mem,
    rc::Rc,
    time::{Duration, Instant},
};

use super::super::super::TimerId;

use super::{NetworkMessage, NodeId};

/// A message in a recipient's queue
#[derive(Debug)]
pub(crate) struct MsgQueueEntry {
    pub(crate) sender: NodeId,
    pub(crate) message: NetworkMessage,
}

/// A structure representing the outside world that the node interacts with - in this case,
/// providing network operations and timers.
#[derive(Debug)]
pub(crate) struct World {
    current_time: Instant,
    message_queue: HashMap<NodeId, VecDeque<MsgQueueEntry>>,
    timers: HashMap<NodeId, BTreeMap<Instant, TimerId>>,
}

impl World {
    /// Creates a new World object
    pub(crate) fn new() -> Self {
        Self {
            current_time: Instant::now(),
            message_queue: Default::default(),
            timers: Default::default(),
        }
    }

    /// Adds a message from `sender` to `recipient` to the message queue.
    pub(crate) fn send_message(
        &mut self,
        sender: NodeId,
        recipient: NodeId,
        message: NetworkMessage,
    ) {
        self.message_queue
            .entry(recipient)
            .or_default()
            .push_back(MsgQueueEntry { sender, message });
    }

    /// Attempts to receive a message from `recipient`'s queue.
    pub(crate) fn recv_message(&mut self, recipient: NodeId) -> Option<MsgQueueEntry> {
        self.message_queue
            .get_mut(&recipient)
            .and_then(|queue| queue.pop_front())
    }

    /// Advances the world time by a given duration.
    pub(crate) fn advance_time(&mut self, duration: Duration) {
        self.current_time += duration;
    }

    /// Schedules a timer with ID `timer` to fire on `instant` or later. Node with ID `node` will
    /// be notified about the event.
    pub(crate) fn schedule_timer(&mut self, node: NodeId, timer: TimerId, instant: Instant) {
        self.timers.entry(node).or_default().insert(instant, timer);
    }

    /// Return all the timers for `node` that should currently fire.
    pub(crate) fn fire_timers(&mut self, node: NodeId) -> Vec<TimerId> {
        let timers_ref = self.timers.entry(node).or_insert_with(Default::default);
        let timers_to_remain =
            // adding 1 ms so that timers scheduled for now will also fire
            timers_ref.split_off(&(self.current_time + Duration::from_millis(1)));
        mem::replace(timers_ref, timers_to_remain)
            .into_iter()
            .map(|(_, val)| val)
            .collect()
    }

    /// Checks whether the message queue is empty.
    pub(crate) fn is_queue_empty(&self) -> bool {
        self.message_queue.iter().all(|(_, queue)| queue.is_empty())
    }

    /// Returns the duration that has to pass for at least one timer to fire (if any timers are
    /// scheduled).
    pub(crate) fn time_to_earliest_timer(&self) -> Option<Duration> {
        self.timers
            .iter()
            .filter_map(|(_, timers)| timers.keys().next())
            .min()
            .map(|instant| instant.saturating_duration_since(self.current_time))
    }
}

/// A handle providing access to the World, to be kept by a Node.
#[derive(Debug)]
pub(crate) struct WorldHandle {
    world: Rc<RefCell<World>>,
    node_id: NodeId,
}

impl WorldHandle {
    /// Creates a new handle
    pub(crate) fn new(world: Rc<RefCell<World>>, node_id: NodeId) -> Self {
        Self { world, node_id }
    }

    /// Sends a message to a given destination
    pub(crate) fn send_message(&self, dst: NodeId, msg: NetworkMessage) {
        self.world.borrow_mut().send_message(self.node_id, dst, msg);
    }

    /// Attempts to receive a message sent to the handle's owner
    pub(crate) fn recv_message(&self) -> Option<MsgQueueEntry> {
        self.world.borrow_mut().recv_message(self.node_id)
    }

    /// Schedules a timer to fire at a given instant
    pub(crate) fn schedule_timer(&self, timer: TimerId, instant: Instant) {
        self.world
            .borrow_mut()
            .schedule_timer(self.node_id, timer, instant);
    }

    /// Returns all the timers that should currently fire
    pub(crate) fn fire_timers(&self) -> Vec<TimerId> {
        self.world.borrow_mut().fire_timers(self.node_id)
    }
}

mod tests {
    use super::World;
    use std::time::{Duration, Instant};

    #[test]
    fn test_timers() {
        let mut world = World::new();
        let instant = Instant::now();

        let one_second = Duration::from_millis(1000);
        let two_seconds = Duration::from_millis(2000);

        let node_id = "TestNode";

        assert!(world.fire_timers(node_id).is_empty());

        world.schedule_timer(node_id, 0, instant + two_seconds);

        assert!(world.fire_timers(node_id).is_empty());

        world.advance_time(one_second);

        assert!(world.fire_timers(node_id).is_empty());

        world.advance_time(two_seconds);

        assert!(world.fire_timers(node_id).len() == 1);
    }
}
