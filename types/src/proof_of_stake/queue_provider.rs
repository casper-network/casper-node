use crate::proof_of_stake::queue::Queue;

/// Provider of a [`Queue`] access methods.
pub trait QueueProvider {
    /// Reads bonding queue.
    fn read_bonding(&mut self) -> Queue;

    /// Reads unbonding queue.
    fn read_unbonding(&mut self) -> Queue;

    /// Writes bonding queue.
    fn write_bonding(&mut self, queue: Queue);

    /// Writes unbonding queue.
    fn write_unbonding(&mut self, queue: Queue);
}
