use super::*;

use crate::components::consensus::{
    highway_core::{
        highway::tests::test_validators,
        highway_testing::TEST_INSTANCE_ID,
        state::{tests::*, State},
    },
    protocols::highway::tests::NodeId,
};

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
    let mut sync = Synchronizer::<NodeId, TestContext>::new(0x20.into(), TEST_INSTANCE_ID);
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
    let (maybe_pv, outcomes) = sync.pop_vertex_to_add(&highway);
    assert!(maybe_pv.is_none());
    assert!(matches!(
        *outcomes,
        [
            ProtocolOutcome::CreatedTargetedMessage(_, NodeId(0)),
            ProtocolOutcome::CreatedTargetedMessage(_, NodeId(0)),
        ]
    ));

    // At 0x23, c0 gets enqueued and added.
    // That puts c1 back into the main queue, since its dependency is satisfied.
    let now = 0x23.into();
    let outcomes = sync.schedule_add_vertex(peer0, pvv(c0), now);
    assert!(
        matches!(*outcomes, [ProtocolOutcome::QueueAction(ACTION_ID_VERTEX)]),
        "unexpected outcomes: {:?}",
        outcomes
    );
    let (maybe_pv, outcomes) = sync.pop_vertex_to_add(&highway);
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
    let (maybe_pv, outcomes) = sync.pop_vertex_to_add(&highway);
    assert_eq!(Dependency::Unit(c1), maybe_pv.unwrap().vertex().id());
    assert!(outcomes.is_empty());
    assert!(sync.is_empty());
}
