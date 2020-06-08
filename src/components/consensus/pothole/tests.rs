mod mock_network;

use std::collections::BTreeMap;

use mock_network::NodeSet;

const NODE_NAMES: [&str; 6] = ["Alice", "Bob", "Carol", "Dave", "Eric", "Fred"];

// This test initializes a few nodes, proposes a few transactions and checks whether all nodes have
// the same chain of finalized blocks in the end.
#[test]
fn test_consensus() {
    let mut nodes = NodeSet::new(&NODE_NAMES[..]);

    nodes.propose_transaction("Bob", "Bob's Transaction".to_owned());

    while nodes.busy() {
        nodes.step();
    }

    nodes.propose_transaction("Carol", "Carol's Transaction".to_owned());

    while nodes.busy() {
        nodes.step();
    }

    nodes.propose_transaction("Alice", "Alice's Transaction".to_owned());
    nodes.propose_transaction("Fred", "Fred's Transaction".to_owned());

    while nodes.busy() {
        nodes.step();
    }

    let blocks = nodes
        .nodes()
        .iter()
        .map(|(node_id, node)| (*node_id, node.consensused_blocks().collect::<Vec<_>>()))
        .collect::<BTreeMap<_, _>>();

    assert!(blocks.iter().all(|(_, blocks)| blocks.len() == 3));

    let blocks_at_first_node = blocks.iter().next().unwrap().1;

    assert!(blocks
        .iter()
        .all(|(_, blocks)| blocks == blocks_at_first_node));
}
