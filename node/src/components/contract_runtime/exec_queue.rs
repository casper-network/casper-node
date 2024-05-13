use datasize::DataSize;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use crate::types::{ExecutableBlock, MetaBlockState};

#[derive(Default, Clone, DataSize)]
pub(super) struct ExecQueue(Arc<Mutex<BTreeMap<u64, QueueItem>>>);

impl ExecQueue {
    /// How many blocks are backed up in the queue
    pub fn len(&self) -> usize {
        self.0
            .lock()
            .expect(
                "components::contract_runtime: couldn't get execution queue size; mutex poisoned",
            )
            .len()
    }

    pub fn remove(&mut self, height: u64) -> Option<QueueItem> {
        self.0
            .lock()
            .expect("components::contract_runtime: couldn't remove from the queue; mutex poisoned")
            .remove(&height)
    }

    pub fn insert(&mut self, height: u64, item: QueueItem) {
        self.0
            .lock()
            .expect("components::contract_runtime: couldn't insert into the queue; mutex poisoned")
            .insert(height, item);
    }

    /// Remove every entry older than the given height, and return the new len.
    pub fn remove_older_then(&mut self, height: u64) -> i64 {
        let mut locked_queue = self.0
            .lock()
            .expect(
                "components::contract_runtime: couldn't initialize contract runtime block execution queue; mutex poisoned"
            );

        *locked_queue = locked_queue.split_off(&height);

        core::convert::TryInto::try_into(locked_queue.len()).unwrap_or(i64::MIN)
    }
}

// Should it be an enum?
pub(super) struct QueueItem {
    pub executable_block: ExecutableBlock,
    pub meta_block_state: MetaBlockState,
}
