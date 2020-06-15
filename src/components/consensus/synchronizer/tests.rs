mod structs;

use std::collections::HashMap;

use super::{Synchronizer, SynchronizerMessage};

use structs::{Dag, DagIndex, DagNode, MissingDeps, NodeId};

#[derive(Debug)]
struct Node {
    our_id: NodeId,
    dag: Dag,
    synchronizer: Synchronizer<NodeId, Dag>,
}

impl Node {
    fn new(our_id: NodeId) -> Self {
        Self {
            our_id,
            dag: Dag::new(),
            synchronizer: Synchronizer::new(),
        }
    }

    fn create_vertex(&mut self, data: Option<String>) -> (DagIndex, DagNode) {
        self.dag.create_node(self.our_id, data)
    }

    fn handle_message(
        &mut self,
        sender: NodeId,
        msg: SynchronizerMessage<MissingDeps>,
    ) -> Vec<(NodeId, SynchronizerMessage<MissingDeps>)> {
        self.synchronizer.handle_message(&mut self.dag, sender, msg)
    }
}

#[derive(Debug)]
struct Nodes(HashMap<&'static str, Node>);

impl Nodes {
    fn new(ids: Vec<&'static str>) -> Self {
        Self(
            ids.into_iter()
                .map(|nid| (nid, Node::new(nid.into())))
                .collect(),
        )
    }

    fn node(&self, id: &'static str) -> &Node {
        self.0.get(id).unwrap()
    }

    fn node_mut(&mut self, id: &'static str) -> &mut Node {
        self.0.get_mut(id).unwrap()
    }
}

#[test]
fn test_synchronizer() {
    let mut nodes = Nodes::new(vec!["Alice", "Bob", "Carol"]);

    let (index, item) = nodes
        .node_mut("Alice")
        .create_vertex(Some("alice_data".to_owned()));

    // let Bob handle the new item from Alice - should get accepted and no new messages should be
    // generated
    assert!(nodes
        .node_mut("Bob")
        .handle_message("Alice".into(), SynchronizerMessage::NewItem(index, item))
        .is_empty());

    let (index, item) = nodes
        .node_mut("Bob")
        .create_vertex(Some("bob_data".to_owned()));
    assert_eq!(item.parents.len(), 1);

    // Carol will handle Bob's message - but his item will depend on Alice's item, which Carol
    // doesn't have yet, so she should request it
    let mut messages = nodes
        .node_mut("Carol")
        .handle_message("Bob".into(), SynchronizerMessage::NewItem(index, item));
    assert_eq!(messages.len(), 1);
    let (target, message) = messages.pop().unwrap();
    assert_eq!(target, "Bob".into());

    // Bob will now handle Carol's request - he should respond with a new item message
    let mut response = nodes
        .node_mut(target.0)
        .handle_message("Carol".into(), message);
    assert_eq!(response.len(), 1);
    let (target, message) = response.pop().unwrap();
    assert_eq!(target, "Carol".into());

    // Carol should now handle Bob's response and complete her DAG
    assert!(nodes
        .node_mut(target.0)
        .handle_message("Bob".into(), message)
        .is_empty());

    // Bob's and Carol's DAGs should now be consistent
    assert_eq!(nodes.node("Bob").dag, nodes.node("Carol").dag);
}
