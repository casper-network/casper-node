use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt::{self, Display, Formatter},
    iter::FromIterator,
};

use casper_execution_engine::{core::engine_state, storage::trie::TrieRaw};
use casper_hashing::Digest;
use datasize::DataSize;
use derive_more::From;
use serde::Serialize;
use tracing::{debug, error};

use crate::types::{TrieOrChunk, TrieOrChunkId};

use super::trie_acquisition::{Error as TrieAcquisitionError, TrieAcquisition};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, DataSize, From)]
pub(crate) struct RootHash(Digest);

impl RootHash {
    pub(crate) fn into_inner(self) -> Digest {
        self.0
    }
}

impl Display for RootHash {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, DataSize, From)]
pub(crate) struct TrieHash(Digest);

impl TrieHash {
    pub(crate) fn into_inner(self) -> Digest {
        self.0
    }
}

impl Display for TrieHash {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

/*
#[derive(Clone, PartialEq, Eq, DataSize, Debug)]
enum TrieAcquisitionState {
    Acquiring(TrieAcquisition),
    Acquired(TrieRaw),
    PendingStore(TrieRaw, Vec<TrieHash>),
}
*/

#[derive(Clone, Copy, PartialEq, Eq, DataSize, Debug)]
pub(crate) enum Error {
    TrieOrChunkApply(TrieAcquisitionError),
    UnexpectedTrieOrChunkRegister(Digest),
    TrieHashMismatch { expected: Digest, actual: Digest },
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::TrieOrChunkApply(e) => {
                write!(
                    f,
                    "failed to apply trie or chunk to pending trie acquisition; {}",
                    e
                )
            }
            Error::UnexpectedTrieOrChunkRegister(hash) => {
                write!(
                    f,
                    "attempted to register trie or chunk {} failed because trie or chunk was unexpected by Global State acquisition",
                    hash
                )
            }
            Error::TrieHashMismatch { expected, actual } => {
                write!(
                    f,
                    "trie hash mismatch; expected: {}, actual: {}",
                    expected, actual,
                )
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq, DataSize, Debug)]
pub(crate) struct GlobalStateAcquisition {
    root_hash: RootHash,
    fetch_queue: VecDeque<(TrieAcquisition, Option<Digest>)>,
    pending_tries: HashMap<TrieHash, (TrieAcquisition, Option<Digest>)>,
    acquired_tries: HashMap<TrieHash, (TrieRaw, Option<Digest>)>,
    tries_pending_store: HashMap<TrieHash, Option<Digest>>,
    tries_awaiting_children: HashMap<TrieHash, (TrieRaw, HashSet<Digest>, Option<Digest>)>,
}

impl GlobalStateAcquisition {
    pub(crate) fn new(root_hash: &Digest) -> GlobalStateAcquisition {
        debug!(%root_hash, "Creating GlobalStateAcquisition");
        GlobalStateAcquisition {
            root_hash: RootHash(*root_hash),
            fetch_queue: VecDeque::from([(
                TrieAcquisition::Needed {
                    trie_hash: *root_hash,
                },
                None,
            )]),
            pending_tries: HashMap::new(),
            acquired_tries: HashMap::new(),
            tries_pending_store: HashMap::new(),
            tries_awaiting_children: HashMap::new(),
        }
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.fetch_queue.is_empty()
            && self.pending_tries.is_empty()
            && self.acquired_tries.is_empty()
            && self.tries_pending_store.is_empty()
            && self.tries_awaiting_children.is_empty()
    }

    pub(crate) fn root_hash(&self) -> Digest {
        self.root_hash.into_inner()
    }

    pub(crate) fn tries_to_fetch(
        &mut self,
        max_parallel_trie_fetches: usize,
    ) -> Vec<TrieOrChunkId> {
        let mut tries_or_chunks_to_fetch: Vec<TrieOrChunkId> = Vec::new();
        let mut pending: HashMap<TrieHash, (TrieAcquisition, Option<Digest>)> = HashMap::new();

        for _ in 0..max_parallel_trie_fetches - self.pending_tries.len() {
            if let Some((acq, parent)) = self.fetch_queue.pop_back() {
                if let Some(trie_or_chunk) = acq.needs_value_or_chunk() {
                    tries_or_chunks_to_fetch.push(trie_or_chunk);
                    pending.insert(TrieHash(acq.trie_hash()), (acq, parent));
                } else {
                    match acq {
                        TrieAcquisition::Needed { .. } | TrieAcquisition::Acquiring { .. } => {
                            error!(trie_hash=%acq.trie_hash(), "GlobalStateAcquisition: Inconsistent trie acquisition state");
                        }
                        TrieAcquisition::Complete { trie_hash, trie } => {
                            self.acquired_tries
                                .insert(TrieHash(trie_hash), (*trie, parent));
                        }
                    }
                }
            } else {
                break;
            }
        }

        self.pending_tries.extend(pending);

        debug!(root_hash=%self.root_hash, ?tries_or_chunks_to_fetch, "GlobalStateAcquisition: returning tries to be fetched");
        tries_or_chunks_to_fetch
    }

    pub(super) fn register_trie_or_chunk(
        &mut self,
        trie_hash: Digest,
        trie_or_chunk: TrieOrChunk,
    ) -> Result<(), Error> {
        debug!(
            root_hash=%self.root_hash,
            ?trie_or_chunk,
            "GlobalStateAcquisition: received a trie or chunk"
        );

        let fetched_trie_or_chunk_hash = *trie_or_chunk.trie_hash();
        if trie_hash != fetched_trie_or_chunk_hash {
            if let Some((acq, parent)) = self.pending_tries.remove(&TrieHash(trie_hash)) {
                self.fetch_queue.push_back((acq, parent));
            } else {
                return Err(Error::UnexpectedTrieOrChunkRegister(trie_hash));
            }
            return Err(Error::TrieHashMismatch {
                expected: trie_hash,
                actual: fetched_trie_or_chunk_hash,
            });
        }

        if let Some((acq, parent)) = self.pending_tries.remove(&TrieHash(trie_hash)) {
            match acq.apply_trie_or_chunk(trie_or_chunk) {
                Ok(acq) => match acq {
                    TrieAcquisition::Needed { .. } | TrieAcquisition::Acquiring { .. } => {
                        self.fetch_queue.push_back((acq.clone(), parent));
                        debug!(
                            root_hash=%self.root_hash,
                            ?acq,
                            "GlobalStateAcquisition: trie still incomplete; moving to fetch queue"
                        );
                        Ok(())
                    }
                    TrieAcquisition::Complete { trie_hash, trie } => {
                        self.acquired_tries
                            .insert(TrieHash(trie_hash), (*trie, parent));
                        debug!(
                            root_hash=%self.root_hash,
                            %trie_hash,
                            "GlobalStateAcquisition: trie fetch complete; moving to acquired"
                        );
                        Ok(())
                    }
                },
                Err(e) => {
                    debug!(
                        root_hash=%self.root_hash,
                        error=?e,
                        "GlobalStateAcquisition: error when trying to apply trie or chunk"
                    );
                    Err(Error::TrieOrChunkApply(e))
                }
            }
        } else {
            debug!(
                root_hash=%self.root_hash,
                ?trie_or_chunk,
                "GlobalStateAcquisition: trie or chunk was not pending;"
            );
            Err(Error::UnexpectedTrieOrChunkRegister(trie_hash))
        }
    }

    pub(super) fn register_trie_or_chunk_fetch_error(
        &mut self,
        trie_hash: Digest,
    ) -> Result<(), Error> {
        if let Some((acq, parent)) = self.pending_tries.remove(&TrieHash(trie_hash)) {
            self.fetch_queue.push_back((acq.clone(), parent));
            debug!(
                root_hash=%self.root_hash,
                ?acq,
                "GlobalStateAcquisition: Failed to fetch trie; moving to fetch queue"
            );
            Ok(())
        } else {
            debug!(
                root_hash=%self.root_hash,
                ?trie_hash,
                "GlobalStateAcquisition: trie or chunk was not pending;"
            );
            Err(Error::UnexpectedTrieOrChunkRegister(trie_hash))
        }
    }

    pub(crate) fn tries_to_store(&self) -> Vec<(Digest, TrieRaw)> {
        let tries_to_store: Vec<(Digest, TrieRaw)> = self
            .acquired_tries
            .iter()
            .map(|(trie_hash, (trie_raw, _))| (trie_hash.into_inner(), trie_raw.clone()))
            .collect();

        debug!(
            root_hash=%self.root_hash,
            ?tries_to_store,
            "GlobalStateAcquisition: tries to store from acquired state"
        );

        tries_to_store
    }

    pub(super) fn register_pending_put_tries(
        &mut self,
        register_pending_put_tries: HashSet<Digest>,
    ) {
        for trie_hash in register_pending_put_tries {
            if let Some((_, parent)) = self.acquired_tries.remove(&TrieHash(trie_hash)) {
                self.tries_pending_store.insert(TrieHash(trie_hash), parent);
            }
        }
    }

    // Process the result of the `PutTrie` request
    pub(super) fn register_put_trie(
        &mut self,
        trie_hash: Digest,
        trie_raw: TrieRaw,
        put_trie_result: Result<Digest, engine_state::Error>,
    ) -> Result<(), Error> {
        debug!(
            root_hash=%self.root_hash,
            %trie_hash,
            "GlobalStateAcquisition: received put_trie result"
        );

        if let Some(parent) = self.tries_pending_store.remove(&TrieHash(trie_hash)) {
            match put_trie_result {
                Ok(stored_trie_hash) => {
                    // Got a notification that the trie was successfully stored
                    debug!(
                        root_hash=%self.root_hash,
                        trie_hash=%stored_trie_hash,
                        "GlobalStateAcquisition: put_trie was successful"
                    );

                    // If this trie was a dependency for storing another trie then mark it as stored
                    // for the parent trie in order for it to be enqueued for storing if possible
                    if let Some(parent_hash) = parent {
                        debug!(
                            root_hash=%self.root_hash,
                            trie_hash=%stored_trie_hash,
                            %parent_hash,
                            "GlobalStateAcquisition: found trie to be the child of some other trie"
                        );
                        let can_store_parent = if let Some((_, children, _)) =
                            self.tries_awaiting_children.get_mut(&TrieHash(parent_hash))
                        {
                            debug!(
                                root_hash=%self.root_hash,
                                trie_hash=%stored_trie_hash,
                                %parent_hash,
                                "GlobalStateAcquisition: removing child from parent dependencies"
                            );
                            children.remove(&stored_trie_hash);
                            children.is_empty()
                        } else {
                            false
                        };

                        // All the dependencies of the parent are met so enqueue it to be stored
                        // again
                        if can_store_parent {
                            if let Some((trie_raw, _, parent)) =
                                self.tries_awaiting_children.remove(&TrieHash(parent_hash))
                            {
                                self.acquired_tries
                                    .insert(TrieHash(parent_hash), (trie_raw, parent));
                            }
                        }
                    }
                    // Trie is stored so we don't need to hold on to it anymore
                    Ok(())
                }
                Err(engine_state::Error::MissingTrieNodeChildren(missing_children)) => {
                    // Could not store the trie because it has missing children
                    debug!(
                        root_hash=%self.root_hash,
                        %trie_hash,
                        ?missing_children,
                        "GlobalStateAcquisition: trie_result shows that we are missing some children"
                    );

                    // Try to fetch the missing children for this try
                    // Mark this trie as the parent of all the missing children; when the children
                    // are stored, this trie will be re-enqueued for storing
                    missing_children.iter().for_each(|child| {
                        debug!(
                            root_hash=%self.root_hash,
                            %child,
                            %trie_hash,
                            "GlobalStateAcquisition: add child to fetch queue"
                        );

                        self.fetch_queue.push_back((
                            TrieAcquisition::Needed { trie_hash: *child },
                            Some(trie_hash), // parent is the current trie hash
                        ))
                    });

                    debug!(
                        root_hash=%self.root_hash,
                        %trie_hash,
                        ?parent,
                        "GlobalStateAcquisition: putting trie on wait queue until all children are stored"
                    );
                    // Buffer this trie until all children are stored
                    self.tries_awaiting_children.insert(
                        TrieHash(trie_hash),
                        (trie_raw, HashSet::from_iter(missing_children), parent),
                    );
                    Ok(())
                }
                Err(e) => {
                    debug!(
                        root_hash=%self.root_hash,
                        %trie_hash,
                        error=%e,
                        "GlobalStateAcquisition: trie_result shows that a different error has occurred"
                    );

                    // Retry to store this trie
                    self.acquired_tries
                        .insert(TrieHash(trie_hash), (trie_raw, parent));

                    Ok(())
                }
            }
        } else {
            debug!(
                root_hash=%self.root_hash,
                %trie_hash,
                "GlobalStateAcquisition: could not find trie hash in pending stores"
            );
            Err(Error::UnexpectedTrieOrChunkRegister(trie_hash))
        }
    }
}
