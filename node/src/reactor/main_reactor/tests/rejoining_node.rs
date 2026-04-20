use crate::reactor::main_reactor::{
    tests::{
        configs_override::ConfigsOverride, fixture::TestFixture, initial_stakes::InitialStakes,
        Nodes, ERA_TWO, ONE_MIN,
    },
    ReactorState,
};

/*
  Scenario:
  In Era_1 the node crashes towards the end of the era.
  The node comes back up in Era_2 and needs to validate a finality signature from Era_1
  The node should have enough validator awareness for the BlockValidator not to crash the
  node (it was a bug before)
*/
#[tokio::test]
async fn should_recover_after_rejoin_when_node_has_unfinished_business_in_previous_era() {
    let alice_stake = 100_000_000_000_u64;
    let bob_stake = 100_000_000_000_u64;
    let charlie_stake = 100_000_000_000_u64;
    let fourth_stake = 120_000_000_000_u64;
    let initial_stakes = InitialStakes::FromVec(vec![
        alice_stake.into(),
        bob_stake.into(),
        charlie_stake.into(),
        fourth_stake.into(),
    ]);

    let mut fixture = TestFixture::new(
        initial_stakes,
        Some(ConfigsOverride {
            minimum_block_time: "5second".parse().unwrap(),
            minimum_era_height: 5,
            ..Default::default()
        }),
    )
    .await;
    fixture.run_until_block_height(3, ONE_MIN).await;

    // Node 4 goes down - when it comes back up it will
    // still want to sign over at least 2 of the blocks from Era 1
    fixture.remove_node_by_idx(3);
    fixture.run_until_block_height(6, ONE_MIN).await;

    // We should be now in Era 2. Node 4 is still down.
    // When node 4 will go back up - we don't know which of the nodes will be proposing
    // the new block which cites node_4 signatures over block_4 and block_5. That is a
    // problem because the test would be non-deterministic (node 1-3 have their
    // validator matrix filled with Era 1 and Era 2 data). To push the network into the edge
    // case we force a restart on all of the nodes to see if the validator matrixes will
    // get reinitialized correctly.
    fixture.remove_node_by_idx(0);
    fixture.remove_node_by_idx(1);
    fixture.remove_node_by_idx(2);
    fixture.add_node_from_context_idx(0).await;
    fixture.add_node_from_context_idx(1).await;
    fixture.add_node_from_context_idx(2).await;
    fixture.add_node_from_context_idx(3).await;

    // Wait for the network to start validating
    fixture
        .run_until(
            move |nodes: &Nodes| {
                nodes.values().all(|runner| {
                    let state = runner.main_reactor().state;
                    matches!(state, ReactorState::Validate)
                })
            },
            ONE_MIN,
        )
        .await;

    // Node 4 should eventually send out it's signatures over B_4 and B_5.
    // Whichever node will produce the block citing those signatures should be after a fresh restart
    // and if we progress to Era 3 without error it means that we are in the clear.
    fixture
        .run_until_stored_switch_block_header(ERA_TWO, ONE_MIN)
        .await;
}
