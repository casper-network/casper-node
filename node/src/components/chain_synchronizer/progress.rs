use std::sync::{Arc, Mutex, MutexGuard};

use datasize::DataSize;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::error;

use casper_hashing::Digest;

use crate::types::BlockHash;

/// The reason for syncing the trie store under a given state root hash.
//
// Note: this is used when calling `sync_trie_store`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, DataSize, Debug, JsonSchema)]
pub enum FetchingTriesReason {
    /// Performing a trie-store sync during fast-sync before moving to the "execute forwards"
    /// phase.
    #[serde(rename = "for fast sync")]
    FastSync,
    /// Performing a trie-store sync during fast-sync before performing an emergency upgrade.
    #[serde(rename = "preparing for emergency upgrade")]
    EmergencyUpgrade,
    /// Performing a trie-store sync during fast-sync before performing an upgrade.
    #[serde(rename = "preparing for upgrade")]
    Upgrade,
}

/// The progress of the fast-sync task, performed by all nodes while in joining mode.
///
/// The fast-sync task generally progresses from each variant to the next linearly.  An exception is
/// that it will cycle repeatedly from `FetchingBlockAndDeploysToExecute` to `ExecutingBlock` (and
/// possibly to `RetryingBlockExecution`) as it iterates forwards one block at a time, fetching then
/// executing.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, DataSize, Debug, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FastSync {
    /// Fast-syncing has not started yet.
    #[serde(rename = "not yet started")]
    NotYetStarted,
    /// Initial setup is being performed.
    Starting,
    /// Currently fetching the trusted block header.
    FetchingTrustedBlockHeader(BlockHash),
    /// Currently syncing the trie-store under the given state root hash.
    #[serde(rename = "fetching_global_state_under_given_block")]
    FetchingTries {
        /// The height of the block containing the given state root hash.
        block_height: u64,
        /// The global state root hash.
        state_root_hash: Digest,
        /// The reason for syncing the trie-store.
        reason: FetchingTriesReason,
        /// The number of remaining tries to fetch (this value can rise and fall as the task
        /// proceeds).
        #[serde(rename = "number_of_remaining_tries_to_fetch")]
        num_tries_to_fetch: usize,
    },
    /// Currently fetching the block at the given height and its deploys in preparation for
    /// executing it.
    #[serde(rename = "fetching_block_and_deploys_at_given_block_height_before_execution")]
    FetchingBlockAndDeploysToExecute(u64),
    /// Currently executing the block at the given height.
    #[serde(rename = "executing_block_at_height")]
    ExecutingBlock(u64),
    /// Currently retrying the execution of the block at the given height, due to an unexpected
    /// mismatch in the previous execution result (due to a mismatch in the deploys' approvals).
    RetryingBlockExecution {
        /// The height of the block being executed.
        block_height: u64,
        /// The retry attempt number.
        attempt: usize,
    },
    /// Fast-syncing has finished, and the node will shortly transition to participating mode.
    Finished,
}

/// The progress of a single sync-block task, many of which are performed in parallel during
/// sync-to-genesis.
///
/// The task progresses from each variant to the next linearly.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, DataSize, Debug, JsonSchema)]
pub enum SyncBlockFetching {
    /// Currently fetching the block and all its deploys.
    #[serde(rename = "block and deploys")]
    BlockAndDeploys,
    /// Currently syncing the trie-store under the state root hash held in the block.
    #[serde(rename = "global_state_under_given_block")]
    Tries {
        /// The global state root hash.
        state_root_hash: Digest,
        /// The number of remaining tries to fetch (this value can rise and fall as the task
        /// proceeds).
        #[serde(rename = "number_of_remaining_tries_to_fetch")]
        num_tries_to_fetch: usize,
    },
    /// Currently fetching the block's signatures.
    #[serde(rename = "block signatures")]
    BlockSignatures,
}

/// Container pairing the given [`SyncBlockFetching`] progress indicator with the height of the
/// relative block.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, DataSize, Debug, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SyncBlock {
    /// The height of the block being synced.
    block_height: u64,
    /// The progress of the sync-block task.
    fetching: SyncBlockFetching,
}

/// The progress of the sync-to-genesis task, only performed by nodes configured to do this, and
/// performed by them while in participating mode.
///
/// The sync-to-genesis task progresses from each variant to the next linearly.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, DataSize, Debug, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SyncToGenesis {
    /// Syncing-to-genesis has not started yet.
    #[serde(rename = "not yet started")]
    NotYetStarted,
    /// Initial setup is being performed.
    Starting,
    /// Currently fetching block headers in batches back towards the genesis block.
    FetchingHeadersBackToGenesis {
        /// The current lowest block header retrieved by this fetch-headers-to-genesis task.
        lowest_block_height: u64,
    },
    /// Currently syncing all blocks from genesis towards the tip of the chain.
    ///
    /// This is done via many parallel sync-block tasks, with each such ongoing task being
    /// represented by an entry in the wrapped `Vec`.  The set is sorted ascending by block height.
    SyncingForwardFromGenesis(Vec<SyncBlock>),
    /// Syncing-to-genesis has finished.
    Finished,
}

/// The progress of the chain-synchronizer task.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, DataSize, Debug, JsonSchema)]
#[serde(untagged)]
pub enum Progress {
    /// The chain-synchronizer is performing the fast-sync task.
    FastSync(FastSync),
    /// The chain-synchronizer is performing the sync-to-genesis task.
    SyncToGenesis(SyncToGenesis),
}

impl Progress {
    pub(super) fn is_finished(&self) -> bool {
        match self {
            Progress::FastSync(progress) => *progress == FastSync::Finished,
            Progress::SyncToGenesis(progress) => *progress == SyncToGenesis::Finished,
        }
    }
}

#[derive(Clone, DataSize, Debug)]
pub(super) struct ProgressHolder {
    inner: Arc<Mutex<Progress>>,
}

/// This impl is specific to fast-sync progress.
impl ProgressHolder {
    pub(super) fn new_fast_sync() -> Self {
        ProgressHolder {
            inner: Arc::new(Mutex::new(Progress::FastSync(FastSync::NotYetStarted))),
        }
    }

    pub(super) fn start_fetching_trusted_block_header(&self, trusted_hash: BlockHash) {
        let mut inner = self.get_inner_while_fast_syncing("fetching_trusted_block_header");
        *inner = Progress::FastSync(FastSync::FetchingTrustedBlockHeader(trusted_hash));
    }

    pub(super) fn start_fetching_tries_for_fast_sync(
        &self,
        block_height: u64,
        state_root_hash: Digest,
    ) {
        let mut inner = self.get_inner_while_fast_syncing("fetching_tries_for_fast_sync");
        *inner = Progress::FastSync(FastSync::FetchingTries {
            block_height,
            state_root_hash,
            reason: FetchingTriesReason::FastSync,
            num_tries_to_fetch: 0,
        });
    }

    pub(super) fn start_fetching_block_and_deploys_to_execute(&self, block_height: u64) {
        let mut inner = self.get_inner_while_fast_syncing("fetching_block_and_deploys_to_execute");
        *inner = Progress::FastSync(FastSync::FetchingBlockAndDeploysToExecute(block_height));
    }

    pub(super) fn start_executing_block(&self, block_height: u64) {
        let mut inner = self.get_inner_while_fast_syncing("executing_block");
        *inner = Progress::FastSync(FastSync::ExecutingBlock(block_height));
    }

    pub(super) fn retry_executing_block(&self, block_height: u64, attempt: usize) {
        let mut inner = self.get_inner_while_fast_syncing("retrying_execute_block");
        *inner = Progress::FastSync(FastSync::RetryingBlockExecution {
            block_height,
            attempt,
        });
    }

    fn get_inner_while_fast_syncing(&self, new_state: &str) -> MutexGuard<Progress> {
        let inner = self.inner.lock().expect("lock poisoned");
        match *inner {
            Progress::SyncToGenesis(_) => {
                error!(
                    current_progress=?inner,
                    "should be fast syncing if setting progress to {}",
                    new_state
                );
            }
            Progress::FastSync(_) => (),
        }
        inner
    }
}

/// This impl is specific to sync-to-genesis progress.
impl ProgressHolder {
    pub(super) fn new_sync_to_genesis() -> Self {
        ProgressHolder {
            inner: Arc::new(Mutex::new(Progress::SyncToGenesis(
                SyncToGenesis::NotYetStarted,
            ))),
        }
    }

    pub(super) fn set_fetching_headers_back_to_genesis(&self, lowest_block_height: u64) {
        let inner = &mut *self.inner.lock().expect("lock poisoned");
        *inner = Progress::SyncToGenesis(SyncToGenesis::FetchingHeadersBackToGenesis {
            lowest_block_height,
        })
    }

    pub(super) fn start_syncing_block_for_sync_forward(&self, block_height: u64) {
        let inner = &mut *self.inner.lock().expect("lock poisoned");
        if !matches!(
            inner,
            Progress::SyncToGenesis(SyncToGenesis::SyncingForwardFromGenesis(_))
        ) {
            *inner = Progress::SyncToGenesis(SyncToGenesis::SyncingForwardFromGenesis(Vec::new()))
        }

        let tasks =
            if let Progress::SyncToGenesis(SyncToGenesis::SyncingForwardFromGenesis(tasks)) = inner
            {
                tasks
            } else {
                error!(
                    %block_height,
                    current_progress=?inner,
                    "should be syncing forward from genesis if setting progress to fetching block"
                );
                return;
            };

        match tasks.binary_search_by(|task| task.block_height.cmp(&block_height)) {
            Ok(_) => error!(
                %block_height,
                existing_progress=?tasks,
                "expected to only start fetching block and deploys once"
            ),
            Err(index) => tasks.insert(
                index,
                SyncBlock {
                    block_height,
                    fetching: SyncBlockFetching::BlockAndDeploys,
                },
            ),
        }
    }

    pub(super) fn start_fetching_tries_for_sync_forward(
        &self,
        block_height: u64,
        state_root_hash: Digest,
    ) {
        let inner = &mut *self.inner.lock().expect("lock poisoned");
        let tasks =
            if let Progress::SyncToGenesis(SyncToGenesis::SyncingForwardFromGenesis(tasks)) = inner
            {
                tasks
            } else {
                error!(
                    %block_height,
                    current_progress=?inner,
                    "should be syncing forward from genesis if setting progress to fetching tries"
                );
                return;
            };

        let existing_progress =
            match tasks.binary_search_by(|task| task.block_height.cmp(&block_height)) {
                Ok(index) => match tasks.get_mut(index) {
                    Some(existing_progress) => existing_progress,
                    None => {
                        error!(?tasks, "binary search of tasks failed while fetching tries");
                        return;
                    }
                },
                Err(_) => {
                    error!(
                        %block_height,
                        current_progress=?inner,
                        "should be syncing this block if setting progress to fetching tries"
                    );
                    return;
                }
            };

        if !matches!(
            existing_progress.fetching,
            SyncBlockFetching::BlockAndDeploys { .. }
        ) {
            error!(
                %block_height,
                current_progress=?existing_progress,
                "should be fetching block and deploys if setting progress to fetching tries"
            );
        }
        existing_progress.fetching = SyncBlockFetching::Tries {
            state_root_hash,
            num_tries_to_fetch: 0,
        };
    }

    pub(super) fn start_fetching_block_signatures_for_sync_forward(&self, block_height: u64) {
        let inner = &mut *self.inner.lock().expect("lock poisoned");
        let tasks =
            if let Progress::SyncToGenesis(SyncToGenesis::SyncingForwardFromGenesis(tasks)) = inner
            {
                tasks
            } else {
                error!(
                    %block_height,
                    current_progress=?inner,
                    "should be syncing forward from genesis if setting progress to fetching block \
                    signatures"
                );
                return;
            };

        let existing_progress =
            match tasks.binary_search_by(|task| task.block_height.cmp(&block_height)) {
                Ok(index) => match tasks.get_mut(index) {
                    Some(existing_progress) => existing_progress,
                    None => {
                        error!(
                            ?tasks,
                            "binary search of tasks failed while fetching block signatures"
                        );
                        return;
                    }
                },
                Err(_) => {
                    error!(
                        %block_height,
                        current_progress=?inner,
                        "should be syncing this block if setting progress to fetching block \
                        signatures"
                    );
                    return;
                }
            };

        if !matches!(existing_progress.fetching, SyncBlockFetching::Tries { .. }) {
            error!(
                %block_height,
                current_progress=?existing_progress,
                "should be fetching tries if setting progress to fetching block signatures"
            );
        }
        existing_progress.fetching = SyncBlockFetching::BlockSignatures;
    }

    pub(super) fn finish_syncing_block_for_sync_forward(&self, block_height: u64) {
        let inner = &mut *self.inner.lock().expect("lock poisoned");
        let tasks =
            if let Progress::SyncToGenesis(SyncToGenesis::SyncingForwardFromGenesis(tasks)) = inner
            {
                tasks
            } else {
                error!(
                    %block_height,
                    current_progress=?inner,
                    "should be syncing forward from genesis if finishing sync block"
                );
                return;
            };

        match tasks.binary_search_by(|task| task.block_height.cmp(&block_height)) {
            Ok(index) => {
                let existing_progress = tasks.remove(index);
                if !matches!(
                    existing_progress.fetching,
                    SyncBlockFetching::BlockSignatures
                ) {
                    error!(
                        %block_height,
                        current_progress=?existing_progress,
                        "should be fetching block signatures if finishing sync block"
                    );
                }
            }
            Err(_) => {
                error!(
                    %block_height,
                    current_progress=?inner,
                    "should be syncing this block if finishing sync block"
                );
            }
        }
    }
}

/// This impl has functionality common to fast-sync and sync-to-genesis.
impl ProgressHolder {
    pub(super) fn start(&self) {
        match &mut *self.inner.lock().expect("lock poisoned") {
            Progress::FastSync(progress) => *progress = FastSync::Starting,
            Progress::SyncToGenesis(progress) => *progress = SyncToGenesis::Starting,
        }
    }

    pub(super) fn set_num_tries_to_fetch(&self, block_height: u64, num_tries: usize) {
        match &mut *self.inner.lock().expect("lock poisoned") {
            Progress::FastSync(FastSync::FetchingTries {
                num_tries_to_fetch, ..
            }) => *num_tries_to_fetch = num_tries,
            Progress::SyncToGenesis(SyncToGenesis::SyncingForwardFromGenesis(tasks)) => {
                match tasks.binary_search_by(|task| task.block_height.cmp(&block_height)) {
                    Ok(index) => {
                        if let Some(SyncBlock {
                            fetching:
                                SyncBlockFetching::Tries {
                                    num_tries_to_fetch, ..
                                },
                            ..
                        }) = tasks.get_mut(index)
                        {
                            *num_tries_to_fetch = num_tries;
                        } else {
                            error!(
                                block_height,
                                "not currently fetching tries for this block during sync to genesis"
                            );
                        }
                    }
                    Err(_) => {
                        error!(
                            block_height,
                            "not currently syncing block during sync to genesis"
                        );
                    }
                }
            }
            _ => error!(
                block_height,
                "failed to set num tries to fetch as not currently syncing"
            ),
        }
    }

    pub(super) fn finish(&self) {
        match &mut *self.inner.lock().expect("lock poisoned") {
            Progress::FastSync(progress) => *progress = FastSync::Finished,
            Progress::SyncToGenesis(progress) => *progress = SyncToGenesis::Finished,
        }
    }

    pub(super) fn progress(&self) -> Progress {
        self.inner.lock().expect("lock poisoned").clone()
    }
}

/// This impl is specific to functionality used for `debug_assert`s.
#[cfg_attr(not(debug_assertions), allow(unused))]
impl ProgressHolder {
    pub(super) fn is_fast_sync(&self) -> bool {
        matches!(
            *self.inner.lock().expect("lock poisoned"),
            Progress::FastSync(_)
        )
    }

    pub(super) fn is_sync_to_genesis(&self) -> bool {
        matches!(
            *self.inner.lock().expect("lock poisoned"),
            Progress::SyncToGenesis(_)
        )
    }

    pub(super) fn is_fetching_tries(&self, block_height: u64) -> bool {
        match &*self.inner.lock().expect("lock poisoned") {
            Progress::FastSync(FastSync::FetchingTries { .. }) => true,
            Progress::SyncToGenesis(SyncToGenesis::SyncingForwardFromGenesis(tasks)) => {
                match tasks.binary_search_by(|task| task.block_height.cmp(&block_height)) {
                    Ok(index) => {
                        matches!(
                            tasks.get(index),
                            Some(SyncBlock {
                                fetching: SyncBlockFetching::Tries { .. },
                                ..
                            })
                        )
                    }
                    Err(_) => false,
                }
            }
            Progress::FastSync(FastSync::NotYetStarted)
            | Progress::FastSync(FastSync::Starting)
            | Progress::FastSync(FastSync::FetchingTrustedBlockHeader(_))
            | Progress::FastSync(FastSync::FetchingBlockAndDeploysToExecute(_))
            | Progress::FastSync(FastSync::ExecutingBlock(_))
            | Progress::FastSync(FastSync::RetryingBlockExecution { .. })
            | Progress::FastSync(FastSync::Finished)
            | Progress::SyncToGenesis(SyncToGenesis::NotYetStarted)
            | Progress::SyncToGenesis(SyncToGenesis::Starting)
            | Progress::SyncToGenesis(SyncToGenesis::FetchingHeadersBackToGenesis { .. })
            | Progress::SyncToGenesis(SyncToGenesis::Finished) => false,
        }
    }
}
