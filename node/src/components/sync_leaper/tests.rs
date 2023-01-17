use casper_types::testing::TestRng;

use crate::types::{Block, SyncLeap};

pub(crate) fn make_default_sync_leap(rng: &mut TestRng) -> SyncLeap {
    let block = Block::random(rng);
    SyncLeap {
        trusted_ancestor_only: false,
        trusted_block_header: block.header().clone(),
        trusted_ancestor_headers: vec![],
        signed_block_headers: vec![],
    }
}
