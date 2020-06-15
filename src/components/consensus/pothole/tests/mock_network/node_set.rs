use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    time::Duration,
};

use super::{Node, NodeId, Transaction, World, WorldHandle};

/// A set of simulated nodes.
#[derive(Debug)]
pub(crate) struct NodeSet {
    world: Rc<RefCell<World>>,
    nodes: BTreeMap<NodeId, Node>,
}

impl NodeSet {
    /// Creates a new set of nodes with the given IDs.
    pub(crate) fn new(nodes: &[NodeId]) -> Self {
        let ids: BTreeSet<_> = nodes.iter().cloned().collect();
        let world = Rc::new(RefCell::new(World::new()));
        Self {
            nodes: nodes
                .iter()
                .map(|id| {
                    (
                        NodeId::clone(id),
                        Node::new(
                            NodeId::clone(id),
                            ids.clone(),
                            WorldHandle::new(world.clone(), NodeId::clone(id)),
                        ),
                    )
                })
                .collect(),
            world,
        }
    }

    /// Makes all nodes take a simulated step.
    /// If the message queue is empty, the time is advanced so that at least one timer fires (if
    /// any are scheduled). If it's not, the time will be advanced by 250 ms.
    pub(crate) fn step(&mut self) {
        let world_ref = self.world.borrow();
        let queue_empty = world_ref.is_queue_empty();
        let dur_to_timer = world_ref.time_to_earliest_timer();
        drop(world_ref); // explicit drop to avoid issues with RefCell

        if queue_empty {
            // if there are no messages, advance time so that some timer fires and the nodes will do
            // something
            if let Some(duration) = dur_to_timer {
                self.world.borrow_mut().advance_time(duration);
            }
        } else {
            self.world
                .borrow_mut()
                .advance_time(Duration::from_millis(250));
        }

        for node in self.nodes.values_mut() {
            node.step();
        }
    }

    /// Makes `node` propose a transaction to the network.
    pub(crate) fn propose_transaction(&mut self, node: NodeId, transaction: Transaction) {
        if let Some(node) = self.nodes.get_mut(&node) {
            node.propose_transaction(transaction);
        }
    }

    /// Returns a reference to the set of nodes.
    pub(crate) fn nodes(&self) -> &BTreeMap<NodeId, Node> {
        &self.nodes
    }

    /// Checks whether any of the nodes are busy (which means either there being messages in
    /// flight, or nodes still having unfinalized transactions).
    pub(crate) fn busy(&self) -> bool {
        !self.world.borrow().is_queue_empty()
            || self
                .nodes
                .iter()
                .any(|(_, node)| node.has_pending_transactions())
    }
}
