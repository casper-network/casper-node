use casper_types::testing::TestRng;
use prometheus::Registry;

use crate::components::consensus::tests::utils::{ALICE_NODE_ID, ALICE_SECRET_KEY, BOB_NODE_ID};

use super::*;

#[test]
fn upsert_acceptor() {
    let mut rng = TestRng::new();
    let config = Config::default();
    let era0 = EraId::from(0);
    let validator_matrix = ValidatorMatrix::new_with_validator(ALICE_SECRET_KEY.clone());
    let recent_era_interval = 1;
    let block_time = config.purge_interval() / 2;
    let metrics_registry = Registry::new();
    let mut accumulator = BlockAccumulator::new(
        config,
        validator_matrix,
        recent_era_interval,
        block_time,
        &metrics_registry,
    )
    .unwrap();

    accumulator.register_local_tip(0, EraId::new(0));

    let max_block_count =
        PEER_RATE_LIMIT_MULTIPLIER * ((config.purge_interval() / block_time) as usize);

    for _ in 0..max_block_count {
        accumulator.upsert_acceptor(
            BlockHash::random(&mut rng),
            Some(era0),
            Some(*ALICE_NODE_ID),
        );
    }

    assert_eq!(accumulator.block_acceptors.len(), max_block_count);

    let block_hash = BlockHash::random(&mut rng);

    // Alice has sent us too many blocks; we don't register this one.
    accumulator.upsert_acceptor(block_hash, Some(era0), Some(*ALICE_NODE_ID));
    assert_eq!(accumulator.block_acceptors.len(), max_block_count);
    assert!(!accumulator.block_acceptors.contains_key(&block_hash));

    // Bob hasn't sent us anything yet. But we don't insert without an era ID.
    accumulator.upsert_acceptor(block_hash, None, Some(*BOB_NODE_ID));
    assert_eq!(accumulator.block_acceptors.len(), max_block_count);
    assert!(!accumulator.block_acceptors.contains_key(&block_hash));

    // With an era ID he's allowed to tell us about this one.
    accumulator.upsert_acceptor(block_hash, Some(era0), Some(*BOB_NODE_ID));
    assert_eq!(accumulator.block_acceptors.len(), max_block_count + 1);
    assert!(accumulator.block_acceptors.contains_key(&block_hash));

    // And if Alice tells us about it _now_, we'll register her as a peer.
    accumulator.upsert_acceptor(block_hash, None, Some(*ALICE_NODE_ID));
    assert!(accumulator.block_acceptors[&block_hash]
        .peers()
        .contains(&ALICE_NODE_ID));
}
