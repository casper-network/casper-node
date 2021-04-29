use super::*;

use crate::components::consensus::{
    highway_core::{
        highway::tests::test_validators,
        highway_testing::TEST_INSTANCE_ID,
        state::{tests::*, State},
    },
    protocols::highway::tests::NodeId,
};

use std::collections::BTreeSet;

#[test]
fn purge_vertices() {
    let params = test_params(0);
    let mut state = State::new(WEIGHTS, params.clone(), vec![]);

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

    let peer0 = NodeId(0);

    // Create a synchronizer with a 0x20 ms timeout, and a Highway instance.
    let mut sync = Synchronizer::<NodeId, TestContext>::new(
        0x20.into(),
        0x20.into(),
        WEIGHTS.len(),
        TEST_INSTANCE_ID,
    );
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
    let (maybe_pv, outcomes) = sync.pop_vertex_to_add(&highway, &Default::default());
    assert!(maybe_pv.is_none());
    assert_targeted_message(
        &unwrap_single(outcomes),
        &peer0,
        HighwayMessage::RequestDependency(Dependency::Unit(c0)),
    );

    // At 0x23, c0 gets enqueued and added.
    // That puts c1 back into the main queue, since its dependency is satisfied.
    let now = 0x23.into();
    let outcomes = sync.schedule_add_vertex(peer0, pvv(c0), now);
    assert!(
        matches!(*outcomes, [ProtocolOutcome::QueueAction(ACTION_ID_VERTEX)]),
        "unexpected outcomes: {:?}",
        outcomes
    );
    let (maybe_pv, outcomes) = sync.pop_vertex_to_add(&highway, &Default::default());
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
    sync.purge_vertices(0x41.into());

    // The main queue should now contain only c1. If we remove it, the synchronizer is empty.
    let (maybe_pv, outcomes) = sync.pop_vertex_to_add(&highway, &Default::default());
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

    let mut state = State::new(WEIGHTS, params.clone(), vec![]);
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

    let peer0 = NodeId(0);

    // Create a synchronizer with a 0x20 ms timeout, and a Highway instance.
    let mut sync = Synchronizer::<NodeId, TestContext>::new(
        0x20.into(),
        0x20.into(),
        WEIGHTS.len(),
        TEST_INSTANCE_ID,
    );

    let mut highway = Highway::<TestContext>::new(TEST_INSTANCE_ID, test_validators(), params);
    let now = 0x20.into();

    assert!(matches!(
        *sync.schedule_add_vertex(peer0, pvv(c2), now),
        [ProtocolOutcome::QueueAction(ACTION_ID_VERTEX)]
    ));
    // `c2` can't be added to the protocol state yet b/c it's missing its `c1` dependency.
    let (pv, outcomes) = sync.pop_vertex_to_add(&highway, &Default::default());
    assert!(pv.is_none());
    assert_targeted_message(
        &unwrap_single(outcomes),
        &peer0,
        HighwayMessage::RequestDependency(Dependency::Unit(c1)),
    );
    // Simulate `c1` being downloaded…
    let c1_outcomes = sync.schedule_add_vertex(peer0, pvv(c1), now);
    assert!(matches!(
        *c1_outcomes,
        [ProtocolOutcome::QueueAction(ACTION_ID_VERTEX)]
    ));
    // `b0` can't be added to the protocol state b/c it's missing its `c1` dependency,
    // but `c1` has already been downloaded so we should not request it again. We will only request
    // `c0` as that's what `c1` depends on.
    let (pv, outcomes) = sync.pop_vertex_to_add(&highway, &Default::default());
    assert!(pv.is_none());
    assert_targeted_message(
        &unwrap_single(outcomes),
        &peer0,
        HighwayMessage::RequestDependency(Dependency::Unit(c0)),
    );
    // `c1` is now part of the synchronizer's state, we should not try requesting it if other
    // vertices depend on it.
    assert!(matches!(
        *sync.schedule_add_vertex(peer0, pvv(b0), now),
        [ProtocolOutcome::QueueAction(ACTION_ID_VERTEX)]
    ));
    let (pv, outcomes) = sync.pop_vertex_to_add(&highway, &Default::default());
    assert!(pv.is_none());
    // `b0` depends on `c1`, that is already in the synchronizer's state, but it also depends on
    // `c0` transitively that is not yet known. We should request it, even if we had already
    // done that for `c1`.
    assert_targeted_message(
        &unwrap_single(outcomes),
        &peer0,
        HighwayMessage::RequestDependency(Dependency::Unit(c0)),
    );
    // "Download" the last dependency.
    let _ = sync.schedule_add_vertex(peer0, pvv(c0), now);
    // Now, the whole chain can be added to the protocol state.
    let mut units: BTreeSet<Dependency<TestContext>> = vec![c0, c1, b0, c2]
        .into_iter()
        .map(Dependency::Unit)
        .collect();
    while let (Some(pv), outcomes) = sync.pop_vertex_to_add(&highway, &Default::default()) {
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
    outcome: &ProtocolOutcome<NodeId, TestContext>,
    peer: &NodeId,
    expected: HighwayMessage<TestContext>,
) {
    match outcome {
        ProtocolOutcome::CreatedTargetedMessage(msg, peer0) => {
            assert_eq!(peer, peer0);
            let highway_message: HighwayMessage<TestContext> =
                bincode::deserialize(msg.as_slice()).expect("deserialization to pass");
            assert_eq!(highway_message, expected);
        }
        _ => panic!("unexpected outcome: {:?}", outcome),
    }
}
