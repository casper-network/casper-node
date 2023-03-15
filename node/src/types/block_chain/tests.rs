//! Tests for the `block_chain` type.
#![cfg(test)]

use crate::types::{
    block_chain::{
        block_chain_entry::BlockChainEntry, indexed_block_chain::IndexedBlockChain, BlockChain,
        Error,
    },
    Block, BlockHash,
};
use casper_types::{testing::TestRng, EraId};
use num_traits::FromPrimitive;
use std::collections::HashMap;

#[test]
fn should_construct_empty() {
    let block_chain = BlockChain::new();
    assert!(block_chain.is_empty(), "should ctor with empty chain");
}

#[test]
fn should_be_vacant() {
    let block_chain = BlockChain::new();
    assert_eq!(
        block_chain.by_height(0),
        BlockChainEntry::vacant(0),
        "should be vacant"
    );
}

#[test]
fn should_be_proposed() {
    let mut block_chain = BlockChain::new();
    assert!(
        block_chain.register_proposed(0).is_ok(),
        "should register proposed"
    );
    assert_eq!(
        block_chain.by_height(0),
        BlockChainEntry::new_proposed(0),
        "should be proposed"
    );
}

#[test]
fn should_be_finalized() {
    let mut block_chain = BlockChain::new();
    assert!(
        block_chain.register_finalized(0, EraId::new(0)).is_ok(),
        "should register finalized"
    );
    assert_eq!(
        block_chain.by_height(0),
        BlockChainEntry::new_finalized(0, EraId::new(0)),
        "should be finalized"
    );
}

#[test]
fn should_be_incomplete() {
    let mut block_chain = BlockChain::new();
    let block_hash = BlockHash::default();
    block_chain.register_incomplete(0, block_hash, EraId::new(0));
    assert_eq!(
        block_chain.by_height(0),
        BlockChainEntry::new_incomplete(0, block_hash, EraId::new(0)),
        "should be incomplete"
    );
}

#[test]
fn should_be_complete() {
    let mut block_chain = BlockChain::new();
    let mut rng = TestRng::new();
    let block_header = {
        let tmp = Block::random(&mut rng);
        tmp.header().clone()
    };
    block_chain.register_complete(&block_header);
    assert_eq!(
        block_chain.by_height(block_header.height()),
        BlockChainEntry::new_complete(&block_header),
        "should be complete"
    );
}

#[test]
fn should_recognize_higher_status() {
    let mut entries = HashMap::new();
    let mut inserter = |bce: BlockChainEntry| {
        entries.insert(bce.status(), bce);
    };
    inserter(BlockChainEntry::vacant(0));
    inserter(BlockChainEntry::new_proposed(0));
    inserter(BlockChainEntry::new_finalized(0, EraId::new(0)));

    let mut rng = TestRng::new();
    let block_header = {
        let tmp = Block::random(&mut rng);
        tmp.header().clone()
    };
    inserter(BlockChainEntry::new_incomplete(
        block_header.height(),
        block_header.block_hash(),
        EraId::new(0),
    ));
    inserter(BlockChainEntry::new_complete(&block_header));
    let max = entries.keys().max().expect("should have entries");
    for key in entries.keys() {
        if key == max {
            break;
        }
        let x = entries.get(key).expect("x should exist");
        let y = entries.get(&(key + 1)).expect("y should exist");
        assert!(x.status() < y.status(), "should be lower status");
    }
}

#[test]
fn should_find_by_hash() {
    let mut block_chain = BlockChain::new();
    let mut rng = TestRng::new();
    let block_header1 = {
        let tmp = Block::random(&mut rng);
        tmp.header().clone()
    };
    block_chain.register_complete(&block_header1);
    assert_eq!(
        block_chain.by_hash(&block_header1.block_hash()),
        Some(BlockChainEntry::new_complete(&block_header1)),
        "should find complete block by hash"
    );
    let block_header2 = {
        let tmp = Block::random(&mut rng);
        tmp.header().clone()
    };
    block_chain.register_incomplete(
        block_header2.height(),
        block_header2.block_hash(),
        EraId::new(0),
    );
    assert_eq!(
        block_chain.by_hash(&block_header2.block_hash()),
        Some(BlockChainEntry::new_incomplete(
            block_header2.height(),
            block_header2.block_hash(),
            EraId::new(0)
        )),
        "should find incomplete block by hash"
    );
}

#[test]
fn should_not_find_by_hash() {
    let mut block_chain = BlockChain::new();
    let block_hash = BlockHash::default();
    assert!(block_chain.register_proposed(0).is_ok(), "should be ok");
    assert_eq!(
        block_chain.by_hash(&block_hash),
        None,
        "proposed should not be indexed by hash"
    );
    let _ = block_chain.register_finalized(1, EraId::new(1));
    assert_eq!(
        block_chain.by_hash(&block_hash),
        None,
        "finalized should not be indexed by hash"
    );
}

#[test]
fn should_remove() {
    let mut block_chain = BlockChain::new();
    let block_height = 0;
    assert!(
        block_chain.remove(block_height).is_none(),
        "nothing to remove"
    );
    assert!(
        block_chain.register_proposed(block_height).is_ok(),
        "should be ok"
    );
    assert!(false == block_chain.is_empty(), "chain should not be empty");
    let removed = block_chain.remove(block_height).expect("should exist");
    assert_eq!(removed.block_height(), block_height, "removed wrong item");
    assert!(block_chain.is_empty(), "chain should be empty");
}

#[test]
fn should_remove_index() {
    let mut rng = TestRng::new();
    let mut block_chain = BlockChain::new();
    let block_height = 0;
    block_chain.register_complete_from_parts(
        block_height,
        BlockHash::random(&mut rng),
        EraId::from(0),
        false,
    );
    assert!(false == block_chain.is_empty(), "chain should not be empty");
    let removed = block_chain.remove(block_height).expect("should exist");
    assert_eq!(removed.block_height(), block_height, "removed wrong item");
    assert!(block_chain.is_empty(), "chain should be empty");
}

fn irregular_sequence(lbound: u64, ubound: u64, start_break: u64, end_break: u64) -> BlockChain {
    let mut block_chain = BlockChain::new();
    for height in lbound..=ubound {
        if height >= start_break && height <= end_break {
            let _ = block_chain.register_finalized(height, EraId::new(0));
            continue;
        }
        assert!(
            block_chain.register_proposed(height).is_ok(),
            "should be ok {}",
            height
        );
    }
    block_chain
}

#[test]
fn should_find_low_sequence() {
    let lbound = 0;
    let ubound = 14;
    let start_break = 5;
    let block_chain = irregular_sequence(lbound, ubound, start_break, ubound - start_break);
    let low = block_chain.lowest_sequence(BlockChainEntry::is_proposed);
    assert!(false == low.is_empty(), "sequence should not be empty");
    assert_eq!(
        low.iter()
            .min_by(|x, y| x.block_height().cmp(&y.block_height()))
            .expect("should have entry")
            .block_height(),
        lbound,
        "expected first entry by predicate"
    );
    assert_eq!(
        low.iter()
            .max_by(|x, y| x.block_height().cmp(&y.block_height()))
            .expect("should have entry")
            .block_height(),
        start_break - 1,
        "expected last entry by predicate"
    );
}

#[test]
fn should_find_high_sequence() {
    let lbound = 0;
    let ubound = 14;
    let start_break = 5;
    let block_chain = irregular_sequence(lbound, ubound, start_break, ubound - start_break);
    let hi = block_chain.highest_sequence(BlockChainEntry::is_proposed);
    assert!(false == hi.is_empty(), "sequence should not be empty");
    assert_eq!(
        hi.iter()
            .min_by(|x, y| x.block_height().cmp(&y.block_height()))
            .expect("should have entry")
            .block_height(),
        ubound - start_break + 1,
        "expected first entry by predicate"
    );
    assert_eq!(
        hi.iter()
            .max_by(|x, y| x.block_height().cmp(&y.block_height()))
            .expect("should have entry")
            .block_height(),
        ubound,
        "expected last entry by predicate"
    );
}

fn complete_with_switches<F>(
    start: u64,
    end: u64,
    blocks_per_era: u64,
    is_switch_block: F,
) -> BlockChain
where
    F: FnOnce(u64) -> bool + Copy,
{
    let mut rng = TestRng::new();
    let mut block_chain = BlockChain::new();
    let mut era_id = EraId::new(start / blocks_per_era);
    let mut change_era = false;
    for height in start..=end {
        if change_era {
            era_id = era_id.successor();
        }
        let switch = is_switch_block(height);
        block_chain.register_complete_from_parts(
            height,
            BlockHash::random(&mut rng),
            era_id,
            switch,
        );
        change_era = switch;
    }
    block_chain
}

#[test]
fn should_find_switch_blocks() {
    let (start, end, blocks_per_era) = (0, 10, 5);
    let block_chain = complete_with_switches(start, end, blocks_per_era, |h: u64| {
        h == start || h % blocks_per_era == 0
    });
    assert_eq!(
        block_chain
            .lowest(BlockChainEntry::is_complete_switch_block)
            .expect("should have switch blocks")
            .block_height(),
        start,
        "block at height {} should be lowest switch",
        start
    );
    assert_eq!(
        block_chain
            .highest(BlockChainEntry::is_complete_switch_block)
            .expect("should have switch blocks")
            .block_height(),
        end,
        "block at height {} should be highest switch",
        end
    );
    let actual: u64 = u64::from_usize(
        block_chain
            .all_by(BlockChainEntry::is_complete_switch_block)
            .len(),
    )
    .expect("should have value");
    let expected = end / blocks_per_era + 1;
    assert_eq!(actual, expected, "unexpected number of switch blocks");
}

#[test]
fn should_find_immediate_switch_blocks() {
    let (start, end, blocks_per_era) = (0, 10, 5);
    let block_chain = complete_with_switches(start, end, blocks_per_era, |h: u64| {
        h == 0 || h % blocks_per_era == 0 || h - 1 % blocks_per_era == 0
    });
    for height in start..=end {
        let expected = Some(height == 0 || height - 1 % blocks_per_era == 0);
        let actual = block_chain.is_immediate_switch_block(height);
        assert_eq!(
            expected, actual,
            "at height {} expected: {:?} actual: {:?}",
            height, expected, actual
        );
    }
}

#[test]
fn should_find_switch_block_by_era() {
    let (start, end, blocks_per_era) = (0, 15, 5);
    let block_chain = complete_with_switches(start, end, blocks_per_era, |h: u64| {
        h == start || h % blocks_per_era == 0
    });
    let all_switch_blocks = block_chain.all_by(BlockChainEntry::is_complete_switch_block);
    let expected_era_count = end / blocks_per_era + 1;
    let actual_era_count: u64 = u64::from_usize(all_switch_blocks.len()).expect("should convert");
    assert_eq!(
        expected_era_count, actual_era_count,
        "should have {} switch blocks but found {}",
        expected_era_count, actual_era_count
    );
    for era_id in start..=expected_era_count {
        assert!(
            block_chain
                .switch_block_by_era(EraId::new(era_id))
                .is_some(),
            "should have era entry for {}",
            era_id
        );
    }
}

#[test]
fn should_find_blocks_by_era() {
    let (start, end, blocks_per_era) = (0, 15, 5);
    let block_chain = complete_with_switches(start, end, blocks_per_era, |h: u64| {
        h == start || h % blocks_per_era == 0
    });
    let expected_era_count = end / blocks_per_era;
    // era 0 only has genesis
    for era_id in 1..=expected_era_count {
        let all_in_era = block_chain.all_by(|bce| bce.is_definitely_in_era(EraId::new(era_id)));
        let actual = u64::from_usize(all_in_era.len()).expect("should have value");
        assert_eq!(
            blocks_per_era, actual,
            "should have {} blocks per era but found {}",
            blocks_per_era, actual
        );
    }
}

#[test]
fn should_find_by_parent() {
    let mut rng = TestRng::new();
    let mut block_chain = BlockChain::new();
    let era_id = EraId::new(0);
    for height in 0..5 {
        block_chain.register_complete_from_parts(
            height,
            BlockHash::random(&mut rng),
            era_id,
            false,
        );
    }
    for height in 0..4 {
        let parent = block_chain.by_height(height);
        assert!(parent.is_complete(), "should be complete");
        let child = block_chain
            .by_parent(&parent.block_hash().expect("should have block_hash"))
            .expect("should have child");
        assert_eq!(
            child.block_height().saturating_sub(1),
            parent.block_height(),
            "invalid child detected"
        )
    }
}

#[test]
fn should_find_by_child() {
    let mut rng = TestRng::new();
    let mut block_chain = BlockChain::new();
    let era_id = EraId::new(0);
    for height in 0..5 {
        block_chain.register_complete_from_parts(
            height,
            BlockHash::random(&mut rng),
            era_id,
            false,
        );
    }
    for height in (1..5).rev() {
        let child = block_chain.by_height(height);
        assert!(child.is_complete(), "should be complete");
        let parent = block_chain
            .by_child(&child.block_hash().expect("should have block_hash"))
            .expect("should have child");
        assert_eq!(
            parent.block_height() + 1,
            child.block_height(),
            "invalid child detected"
        )
    }
}

#[test]
fn should_have_childless_tail() {
    let mut rng = TestRng::new();
    let mut block_chain = BlockChain::new();
    let era_id = EraId::new(0);
    for height in 0..5 {
        block_chain.register_complete_from_parts(
            height,
            BlockHash::random(&mut rng),
            era_id,
            false,
        );
    }
    let highest = block_chain
        .highest(BlockChainEntry::all_non_vacant)
        .expect("should have some");
    let child = block_chain
        .by_parent(&highest.block_hash().expect("should have block_hash"))
        .expect("should return vacant entry");
    assert!(child.is_vacant(), "tail should not have child");
}

#[test]
fn should_have_parentless_head() {
    let mut rng = TestRng::new();
    let mut block_chain = BlockChain::new();
    let era_id = EraId::new(0);
    for height in 0..5 {
        block_chain.register_complete_from_parts(
            height,
            BlockHash::random(&mut rng),
            era_id,
            false,
        );
    }
    let lowest = block_chain
        .lowest(BlockChainEntry::all_non_vacant)
        .expect("should have some");
    let parent = block_chain.by_child(&lowest.block_hash().expect("should have block_hash"));
    assert!(parent.is_none(), "head should not have parent");
}

#[test]
fn should_not_allow_proposed_with_higher_status_descendants() {
    let mut rng = TestRng::new();
    let mut block_chain = BlockChain::new();
    let era_id = EraId::new(0);
    let block_to_skip = 3;
    for height in 0..5 {
        if height == block_to_skip {
            continue;
        }
        block_chain.register_complete_from_parts(
            height,
            BlockHash::random(&mut rng),
            era_id,
            false,
        );
    }
    let result = block_chain.register_proposed(block_to_skip);
    assert_eq!(
        Err(Error::AttemptToProposeAboveTail),
        result,
        "should detect non-tail proposal"
    )
}

#[test]
fn should_not_allow_finalized_with_higher_status_descendants() {
    let mut rng = TestRng::new();
    let mut block_chain = BlockChain::new();
    let era_id = EraId::new(0);
    let block_to_skip = 3;
    for height in 0..5 {
        if height == block_to_skip {
            continue;
        }
        block_chain.register_complete_from_parts(
            height,
            BlockHash::random(&mut rng),
            era_id,
            false,
        );
    }
    let result = block_chain.register_finalized(block_to_skip, era_id);
    assert_eq!(
        Err(Error::AttemptToFinalizeAboveTail),
        result,
        "should detect non-tail finalization"
    )
}

#[test]
fn should_find_all_actual_entries() {
    let mut rng = TestRng::new();
    let mut block_chain = BlockChain::new();
    let mut expected = vec![];
    for height in 5..15 {
        if (8..10).contains(&height) {
            continue;
        }
        let block_hash = BlockHash::random(&mut rng);
        block_chain.register_incomplete(height, block_hash, EraId::new(0));
        expected.push(BlockChainEntry::new_incomplete(
            height,
            block_hash,
            EraId::new(0),
        ));
    }
    let all = block_chain.all_by(BlockChainEntry::all);
    assert_eq!(expected, all, "should have actual entries only");
}

#[test]
fn should_find_all_virtual_entries() {
    let mut rng = TestRng::new();
    let mut block_chain = BlockChain::new();
    let mut expected = vec![];
    let lbound = 0;
    let ubound = 14;
    for height in lbound..=ubound {
        if height == 0 || height % 2 == 0 {
            expected.push(BlockChainEntry::Vacant {
                block_height: height,
            });
            continue;
        }
        let block_hash = BlockHash::random(&mut rng);
        block_chain.register_incomplete(height, block_hash, EraId::new(0));
        expected.push(BlockChainEntry::new_incomplete(
            height,
            block_hash,
            EraId::new(0),
        ));
    }
    let all = block_chain.all_by(BlockChainEntry::all);
    let range = block_chain.range(Some(lbound), Some(ubound));
    assert!(all.len() < range.len(), "all should not include vacancies");
    assert_ne!(all, range, "range should include vacancies");
    assert_eq!(expected, range, "should have entire range");
    let range = block_chain.range(None, Some(ubound));
    assert_eq!(expected, range, "should have entire range default lbound");

    expected.pop(); // get rid of tail vacancy
    let range = block_chain.range(Some(lbound), None);
    assert_eq!(expected, range, "should have entire range default ubound");
    let range = block_chain.range(None, None);
    assert_eq!(expected, range, "should have entire range unbounded");
}
