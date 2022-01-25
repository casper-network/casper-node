use std::collections::BTreeSet;

use itertools::Itertools;

use crate::{
    components::consensus::{
        highway_core::{
            highway::{tests::test_validators, ValidVertex},
            highway_testing::TEST_INSTANCE_ID,
            state::{tests::*, State},
        },
        BlockContext,
    },
    types::NodeId,
};

use super::*;

#[test]
fn purge_vertices() {
    let params = test_params(0);
    let mut state = State::new(WEIGHTS, params.clone(), vec![], vec![]);

    // We use round exponent 4u8, so a round is 0x10 ms. With seed 0, Carol is the first leader.
    //
    // time:  0x00 0x0A 0x1A 0x2A 0x3A
    //
    // Carol   c0 — c1 — c2
    //            \
    // Bob          ————————— b0 — b1
    let c0 = add_unit!(state, CAROL, 0x00, 4u8, 0xA; N, N, N).unwrap();
    let c1 = add_unit!(state, CAROL, 0x0A, 4u8, None; N, N, c0).unwrap();
    let c2 = add_unit!(state, CAROL, 0x1A, 4u8, None; N, N, c1).unwrap();
    let b0 = add_unit!(state, BOB, 0x2A, 4u8, None; N, N, c0).unwrap();
    let b1 = add_unit!(state, BOB, 0x3A, 4u8, None; N, b0, c0).unwrap();

    // A Highway instance that's just used to create PreValidatedVertex instances below.
    let util_highway =
        Highway::<TestContext>::new(TEST_INSTANCE_ID, test_validators(), params.clone());

    // Returns the WireUnit with the specified hash.
    let unit = |hash: u64| Vertex::Unit(state.wire_unit(&hash, TEST_INSTANCE_ID).unwrap());
    // Returns the PreValidatedVertex with the specified hash.
    let pvv = |hash: u64| util_highway.pre_validate_vertex(unit(hash)).unwrap();

    let peer0 = NodeId::from([0; 64]);

    // Create a synchronizer with a 0x20 ms timeout, and a Highway instance.
    let max_requests_for_vertex = 5;
    let mut sync = Synchronizer::<TestContext>::new(WEIGHTS.len(), TEST_INSTANCE_ID);
    let mut highway = Highway::<TestContext>::new(TEST_INSTANCE_ID, test_validators(), params);

    // At time 0x20, we receive c2, b0 and b1 — the latter ahead of their timestamp.
    // Since c2 is the first entry in the main queue, processing is scheduled.
    let now = 0x20.into();
    assert!(matches!(
        *sync.schedule_add_vertex(peer0, pvv(c2), now),
        [ProtocolOutcome::QueueAction(ACTION_ID_VERTEX)]
    ));
    sync.store_vertex_for_addition_later(unit(b1).timestamp().unwrap(), now, peer0, pvv(b1));
    sync.store_vertex_for_addition_later(unit(b0).timestamp().unwrap(), now, peer0, pvv(b0));

    // At time 0x21, we receive c1.
    let now = 0x21.into();
    assert!(sync.schedule_add_vertex(peer0, pvv(c1), now).is_empty());

    // No new vertices can be added yet, because all are missing dependencies.
    // The missing dependencies of c1 and c2 are requested.
    let (maybe_pv, outcomes) =
        sync.pop_vertex_to_add(&highway, &Default::default(), max_requests_for_vertex);
    assert!(maybe_pv.is_none());
    assert_targeted_message(&unwrap_single(outcomes), &peer0, Dependency::Unit(c0));

    // At 0x23, c0 gets enqueued and added.
    // That puts c1 back into the main queue, since its dependency is satisfied.
    let now = 0x23.into();
    let outcomes = sync.schedule_add_vertex(peer0, pvv(c0), now);
    assert!(
        matches!(*outcomes, [ProtocolOutcome::QueueAction(ACTION_ID_VERTEX)]),
        "unexpected outcomes: {:?}",
        outcomes
    );
    let (maybe_pv, outcomes) =
        sync.pop_vertex_to_add(&highway, &Default::default(), max_requests_for_vertex);
    assert_eq!(Dependency::Unit(c0), maybe_pv.unwrap().vertex().id());
    assert!(outcomes.is_empty());
    let vv_c0 = highway.validate_vertex(pvv(c0)).expect("c0 is valid");
    highway.add_valid_vertex(vv_c0, now);
    let outcomes = sync.remove_satisfied_deps(&highway);
    assert!(
        matches!(*outcomes, [ProtocolOutcome::QueueAction(ACTION_ID_VERTEX)]),
        "unexpected outcomes: {:?}",
        outcomes
    );

    // At time 0x2A, the vertex b0 moves into the main queue.
    let now = 0x2A.into();
    assert!(sync.add_past_due_stored_vertices(now).is_empty());

    // At 0x41, all vertices received at 0x20 are expired, but c1 (received at 0x21) isn't.
    // This will remove:
    // * b1: still postponed due to future timestamp
    // * b0: in the main queue
    // * c2: waiting for dependency c1 to be added
    let purge_vertex_timeout = 0x20;
    #[allow(clippy::integer_arithmetic)]
    sync.purge_vertices((0x41 - purge_vertex_timeout).into());

    // The main queue should now contain only c1. If we remove it, the synchronizer is empty.
    let (maybe_pv, outcomes) =
        sync.pop_vertex_to_add(&highway, &Default::default(), max_requests_for_vertex);
    assert_eq!(Dependency::Unit(c1), maybe_pv.unwrap().vertex().id());
    assert!(outcomes.is_empty());
    assert!(sync.is_empty());
}

#[test]
/// Test that when a vertex depends on a dependency that has already been synchronized, and is
/// waiting in the synchronizer queue state, but is not yet added to the protocol state – that we
/// don't request it again.
fn do_not_download_synchronized_dependencies() {
    let params = test_params(0);
    // A Highway and state instances that are used to create PreValidatedVertex instances below.

    let mut state = State::new(WEIGHTS, params.clone(), vec![], vec![]);
    let util_highway =
        Highway::<TestContext>::new(TEST_INSTANCE_ID, test_validators(), params.clone());

    // We use round exponent 4u8, so a round is 0x10 ms. With seed 0, Carol is the first leader.
    //
    // time:  0x00 0x0A 0x1A 0x2A 0x3A
    //
    // Carol   c0 — c1 — c2
    //                \
    // Bob             — b0

    let c0 = add_unit!(state, CAROL, 0x00, 4u8, 0xA; N, N, N).unwrap();
    let c1 = add_unit!(state, CAROL, 0x0A, 4u8, None; N, N, c0).unwrap();
    let c2 = add_unit!(state, CAROL, 0x1A, 4u8, None; N, N, c1).unwrap();
    let b0 = add_unit!(state, BOB, 0x2A, 4u8, None; N, N, c1).unwrap();

    // Returns the WireUnit with the specified hash.
    let unit = |hash: u64| Vertex::Unit(state.wire_unit(&hash, TEST_INSTANCE_ID).unwrap());
    // Returns the PreValidatedVertex with the specified hash.
    let pvv = |hash: u64| util_highway.pre_validate_vertex(unit(hash)).unwrap();

    let peer0 = NodeId::from([0; 64]);
    let peer1 = NodeId::from([1; 64]);

    // Create a synchronizer with a 0x20 ms timeout, and a Highway instance.
    let max_requests_for_vertex = 5;
    let mut sync = Synchronizer::<TestContext>::new(WEIGHTS.len(), TEST_INSTANCE_ID);

    let mut highway = Highway::<TestContext>::new(TEST_INSTANCE_ID, test_validators(), params);
    let now = 0x20.into();

    assert!(matches!(
        *sync.schedule_add_vertex(peer0, pvv(c2), now),
        [ProtocolOutcome::QueueAction(ACTION_ID_VERTEX)]
    ));
    // `c2` can't be added to the protocol state yet b/c it's missing its `c1` dependency.
    let (pv, outcomes) =
        sync.pop_vertex_to_add(&highway, &Default::default(), max_requests_for_vertex);
    assert!(pv.is_none());
    assert_targeted_message(&unwrap_single(outcomes), &peer0, Dependency::Unit(c1));
    // Simulate `c1` being downloaded…
    let c1_outcomes = sync.schedule_add_vertex(peer0, pvv(c1), now);
    assert!(matches!(
        *c1_outcomes,
        [ProtocolOutcome::QueueAction(ACTION_ID_VERTEX)]
    ));
    // `b0` can't be added to the protocol state b/c it's missing its `c1` dependency,
    // but `c1` has already been downloaded so we should not request it again. We will only request
    // `c0` as that's what `c1` depends on.
    let (pv, outcomes) =
        sync.pop_vertex_to_add(&highway, &Default::default(), max_requests_for_vertex);
    assert!(pv.is_none());
    assert_targeted_message(&unwrap_single(outcomes), &peer0, Dependency::Unit(c0));
    // `c1` is now part of the synchronizer's state, we should not try requesting it if other
    // vertices depend on it.
    assert!(matches!(
        *sync.schedule_add_vertex(peer1, pvv(b0), now),
        [ProtocolOutcome::QueueAction(ACTION_ID_VERTEX)]
    ));
    let (pv, outcomes) =
        sync.pop_vertex_to_add(&highway, &Default::default(), max_requests_for_vertex);
    assert!(pv.is_none());
    // `b0` depends on `c1`, that is already in the synchronizer's state, but it also depends on
    // `c0` transitively that is not yet known. We should request it, even if we had already
    // done that for `c1`.
    assert_targeted_message(&unwrap_single(outcomes), &peer1, Dependency::Unit(c0));
    // "Download" the last dependency.
    let _ = sync.schedule_add_vertex(peer0, pvv(c0), now);
    // Now, the whole chain can be added to the protocol state.
    let mut units: BTreeSet<Dependency<TestContext>> = vec![c0, c1, b0, c2]
        .into_iter()
        .map(Dependency::Unit)
        .collect();
    while let (Some(pv), outcomes) =
        sync.pop_vertex_to_add(&highway, &Default::default(), max_requests_for_vertex)
    {
        // Verify that we don't request any dependency now.
        assert!(
            !outcomes
                .iter()
                .any(|outcome| matches!(outcome, ProtocolOutcome::CreatedTargetedMessage(_, _))),
            "unexpected dependency request {:?}",
            outcomes
        );
        let pv_dep = pv.vertex().id();
        assert!(units.remove(&pv_dep), "unexpected dependency");
        match pv_dep {
            Dependency::Unit(hash) => {
                let vv = highway
                    .validate_vertex(pvv(hash))
                    .unwrap_or_else(|_| panic!("{:?} unit is valid", hash));
                highway.add_valid_vertex(vv, now);
                let _ = sync.remove_satisfied_deps(&highway);
            }
            _ => panic!("expected unit"),
        }
    }
    assert!(sync.is_empty());
}

#[test]
fn transitive_proposal_dependency() {
    let params = test_params(0);
    // A Highway and state instances that are used to create PreValidatedVertex instances below.

    let mut state = State::new(WEIGHTS, params.clone(), vec![], vec![]);
    let util_highway =
        Highway::<TestContext>::new(TEST_INSTANCE_ID, test_validators(), params.clone());

    // Alice   a0 — a1
    //             /  \
    // Bob        /    b0
    //           /
    // Carol   c0

    let a0 = add_unit!(state, ALICE, 0xA; N, N, N).unwrap();
    let c0 = add_unit!(state, CAROL, 0xC; N, N, N).unwrap();
    let a1 = add_unit!(state, ALICE, None; a0, N, c0).unwrap();
    let b0 = add_unit!(state, BOB, None; a1, N, c0).unwrap();

    // Returns the WireUnit with the specified hash.
    let unit = |hash: u64| Vertex::Unit(state.wire_unit(&hash, TEST_INSTANCE_ID).unwrap());
    // Returns the PreValidatedVertex with the specified hash.
    let pvv = |hash: u64| util_highway.pre_validate_vertex(unit(hash)).unwrap();

    let peer0 = NodeId::from([0; 64]);
    let peer1 = NodeId::from([1; 64]);

    // Create a synchronizer with a 0x200 ms timeout, and a Highway instance.
    let max_requests_for_vertex = 5;
    let mut sync = Synchronizer::<TestContext>::new(WEIGHTS.len(), TEST_INSTANCE_ID);

    let mut highway = Highway::<TestContext>::new(TEST_INSTANCE_ID, test_validators(), params);
    let now = 0x100.into();

    assert!(matches!(
        *sync.schedule_add_vertex(peer0, pvv(a1), now),
        [ProtocolOutcome::QueueAction(ACTION_ID_VERTEX)]
    ));
    // `a1` can't be added to the protocol state yet b/c it's missing its `a0` dependency.
    let (pv, outcomes) =
        sync.pop_vertex_to_add(&highway, &Default::default(), max_requests_for_vertex);
    assert!(pv.is_none());
    assert_targeted_message(&unwrap_single(outcomes), &peer0, Dependency::Unit(a0));

    // "Download" and schedule addition of a0.
    let a0_outcomes = sync.schedule_add_vertex(peer0, pvv(a0), now);
    assert!(matches!(
        *a0_outcomes,
        [ProtocolOutcome::QueueAction(ACTION_ID_VERTEX)]
    ));
    // `a0` has no dependencies so we can try adding it to the protocol state.
    let (maybe_pv, outcomes) =
        sync.pop_vertex_to_add(&highway, &Default::default(), max_requests_for_vertex);
    let pv = maybe_pv.expect("expected a0 vertex");
    assert_eq!(pv.vertex(), &unit(a0));
    assert!(outcomes.is_empty());

    // `b0` can't be added either b/c it's relying on `a1` and `c0`.
    assert!(matches!(
        *sync.schedule_add_vertex(peer1, pvv(b0), now),
        [ProtocolOutcome::QueueAction(ACTION_ID_VERTEX)]
    ));
    let a0_pending_values = {
        let mut tmp = HashMap::new();
        let vv = ValidVertex(unit(a0));
        let proposed_block = ProposedBlock::new(1u32, BlockContext::new(now, Vec::new()));
        let mut set = HashSet::new();
        set.insert((vv, peer0));
        tmp.insert(proposed_block, set);
        tmp
    };
    let (maybe_pv, outcomes) =
        sync.pop_vertex_to_add(&highway, &a0_pending_values, max_requests_for_vertex);
    // `peer1` is added as a holder of `a0`'s deploys due to the indirect dependency.
    let pv = maybe_pv.unwrap();
    assert!(pv.sender() == &peer1);
    assert!(pv.vertex() == &unit(a0));
    // `b0` depends on `a0` transitively but `a0`'s deploys are being downloaded,
    // so we don't re-request it.
    assert!(outcomes.is_empty());

    // If we add `a0` to the protocol state, `a1`'s dependency is satisfied.
    // `a1`'s other dependency is `c0`. Since both peers have it we request it from both.
    let vv = highway.validate_vertex(pvv(a0)).expect("a0 is valid");
    highway.add_valid_vertex(vv, now);
    assert!(matches!(
        *sync.remove_satisfied_deps(&highway),
        [ProtocolOutcome::QueueAction(ACTION_ID_VERTEX)]
    ));
    let (maybe_pv, outcomes) =
        sync.pop_vertex_to_add(&highway, &Default::default(), max_requests_for_vertex);
    assert!(maybe_pv.is_none());
    match &*outcomes {
        [ProtocolOutcome::CreatedTargetedMessage(msg0, p0), ProtocolOutcome::CreatedTargetedMessage(msg1, p1)] =>
        {
            assert_eq!(
                vec![&peer0, &peer1],
                vec![p0, p1].into_iter().sorted().collect_vec(),
                "expected to request dependency from exactly two different peers",
            );
            let msg0_parsed: HighwayMessage<TestContext> =
                bincode::deserialize(msg0.as_slice()).expect("deserialization to pass");
            let msg1_parsed =
                bincode::deserialize(msg1.as_slice()).expect("deserialization to pass");

            match (msg0_parsed, msg1_parsed) {
                (
                    HighwayMessage::RequestDependency(_, dep0),
                    HighwayMessage::RequestDependency(_, dep1),
                ) => {
                    assert_eq!(
                        dep0,
                        Dependency::Unit(c0),
                        "unexpected dependency requested"
                    );
                    assert_eq!(
                        dep0, dep1,
                        "we should have requested the same dependency from two different peers"
                    );
                }
                other => panic!("unexpected HighwayMessage variant {:?}", other),
            }
        }
        outcomes => panic!("unexpected outcomes: {:?}", outcomes),
    }
}

fn unwrap_single<T: Debug>(vec: Vec<T>) -> T {
    assert_eq!(
        vec.len(),
        1,
        "expected single element in the vector {:?}",
        vec
    );
    vec.into_iter().next().unwrap()
}

fn assert_targeted_message(
    outcome: &ProtocolOutcome<TestContext>,
    peer: &NodeId,
    expected: Dependency<TestContext>,
) {
    match outcome {
        ProtocolOutcome::CreatedTargetedMessage(msg, peer0) => {
            assert_eq!(peer, peer0);
            let highway_message: HighwayMessage<TestContext> =
                bincode::deserialize(msg.as_slice()).expect("deserialization to pass");
            match highway_message {
                HighwayMessage::RequestDependency(_, got) => assert_eq!(got, expected),
                other => panic!("unexpected variant: {:?}", other),
            }
        }
        _ => panic!("unexpected outcome: {:?}", outcome),
    }
}
