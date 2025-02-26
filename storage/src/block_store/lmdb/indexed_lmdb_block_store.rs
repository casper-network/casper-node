use std::{
    borrow::Cow,
    collections::{btree_map, hash_map::Entry, BTreeMap, BTreeSet, HashMap, HashSet},
};

use super::{
    lmdb_block_store::LmdbBlockStore, lmdb_ext::LmdbExtError, temp_map::TempMap, DbTableId,
};
use datasize::DataSize;
use lmdb::{
    Environment, RoTransaction, RwCursor, RwTransaction, Transaction as LmdbTransaction, WriteFlags,
};

use tracing::info;

use super::versioned_databases::VersionedDatabases;
use crate::block_store::{
    block_provider::{BlockStoreTransaction, DataReader, DataWriter},
    types::{
        ApprovalsHashes, BlockExecutionResults, BlockHashHeightAndEra, BlockHeight, BlockTransfers,
        LatestSwitchBlock, StateStore, StateStoreKey, Tip, TransactionFinalizedApprovals,
    },
    BlockStoreError, BlockStoreProvider, DbRawBytesSpec,
};
use casper_types::{
    execution::ExecutionResult, Approval, Block, BlockBody, BlockHash, BlockHeader,
    BlockSignatures, Digest, EraId, ProtocolVersion, Transaction, TransactionHash, Transfer,
};

/// Indexed lmdb block store.
#[derive(DataSize, Debug)]
pub struct IndexedLmdbBlockStore {
    /// Block store
    block_store: LmdbBlockStore,
    /// A map of block height to block ID.
    block_height_index: BTreeMap<u64, BlockHash>,
    /// A map of era ID to switch block ID.
    switch_block_era_id_index: BTreeMap<EraId, BlockHash>,
    /// A map of transaction hashes to hashes, heights and era IDs of blocks containing them.
    transaction_hash_index: BTreeMap<TransactionHash, BlockHashHeightAndEra>,
}

impl IndexedLmdbBlockStore {
    fn get_reader(&self) -> Result<IndexedLmdbBlockStoreReadTransaction<'_>, BlockStoreError> {
        let txn = self
            .block_store
            .env
            .begin_ro_txn()
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        Ok(IndexedLmdbBlockStoreReadTransaction {
            txn,
            block_store: self,
        })
    }

    /// Inserts the relevant entries to the index.
    ///
    /// If a duplicate entry is encountered, index is not updated and an error is returned.
    fn insert_to_transaction_index(
        transaction_hash_index: &mut BTreeMap<TransactionHash, BlockHashHeightAndEra>,
        block_hash: BlockHash,
        block_height: u64,
        era_id: EraId,
        transaction_hashes: Vec<TransactionHash>,
    ) -> Result<(), BlockStoreError> {
        if let Some(hash) = transaction_hashes.iter().find(|hash| {
            transaction_hash_index
                .get(hash)
                .is_some_and(|old_details| old_details.block_hash != block_hash)
        }) {
            return Err(BlockStoreError::DuplicateTransaction {
                transaction_hash: *hash,
                first: transaction_hash_index[hash].block_hash,
                second: block_hash,
            });
        }

        for hash in transaction_hashes {
            transaction_hash_index.insert(
                hash,
                BlockHashHeightAndEra::new(block_hash, block_height, era_id),
            );
        }

        Ok(())
    }

    /// Inserts the relevant entries to the two indices.
    ///
    /// If a duplicate entry is encountered, neither index is updated and an error is returned.
    pub(super) fn insert_to_block_header_indices(
        block_height_index: &mut BTreeMap<u64, BlockHash>,
        switch_block_era_id_index: &mut BTreeMap<EraId, BlockHash>,
        block_header: &BlockHeader,
    ) -> Result<(), BlockStoreError> {
        let block_hash = block_header.block_hash();
        if let Some(first) = block_height_index.get(&block_header.height()) {
            if *first != block_hash {
                return Err(BlockStoreError::DuplicateBlock {
                    height: block_header.height(),
                    first: *first,
                    second: block_hash,
                });
            }
        }

        if block_header.is_switch_block() {
            match switch_block_era_id_index.entry(block_header.era_id()) {
                btree_map::Entry::Vacant(entry) => {
                    let _ = entry.insert(block_hash);
                }
                btree_map::Entry::Occupied(entry) => {
                    if *entry.get() != block_hash {
                        return Err(BlockStoreError::DuplicateEraId {
                            era_id: block_header.era_id(),
                            first: *entry.get(),
                            second: block_hash,
                        });
                    }
                }
            }
        }

        let _ = block_height_index.insert(block_header.height(), block_hash);
        Ok(())
    }

    /// Ctor.
    pub fn new(
        block_store: LmdbBlockStore,
        hard_reset_to_start_of_era: Option<EraId>,
        protocol_version: ProtocolVersion,
    ) -> Result<IndexedLmdbBlockStore, BlockStoreError> {
        // We now need to restore the block-height index. Log messages allow timing here.
        info!("indexing block store");
        let mut block_height_index = BTreeMap::new();
        let mut switch_block_era_id_index = BTreeMap::new();
        let mut transaction_hash_index = BTreeMap::new();
        let mut block_txn = block_store
            .env
            .begin_rw_txn()
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;

        let mut deleted_block_hashes = HashSet::new();
        // Map of all block body hashes, with their values representing whether to retain the
        // corresponding block bodies or not.
        let mut block_body_hashes = HashMap::new();
        let mut deleted_transaction_hashes = HashSet::<TransactionHash>::new();

        let mut init_fn =
            |cursor: &mut RwCursor, block_header: BlockHeader| -> Result<(), BlockStoreError> {
                let should_retain_block = match hard_reset_to_start_of_era {
                    Some(invalid_era) => {
                        // Retain blocks from eras before the hard reset era, and blocks after this
                        // era if they are from the current protocol version (as otherwise a node
                        // restart would purge them again, despite them being valid).
                        block_header.era_id() < invalid_era
                            || block_header.protocol_version() == protocol_version
                    }
                    None => true,
                };

                // If we don't already have the block body hash in the collection, insert it with
                // the value `should_retain_block`.
                //
                // If there is an existing value, the updated value should be `false` iff the
                // existing value and `should_retain_block` are both `false`.
                // Otherwise the updated value should be `true`.
                match block_body_hashes.entry(*block_header.body_hash()) {
                    Entry::Vacant(entry) => {
                        entry.insert(should_retain_block);
                    }
                    Entry::Occupied(entry) => {
                        let value = entry.into_mut();
                        *value = *value || should_retain_block;
                    }
                }

                let body_txn = block_store
                    .env
                    .begin_ro_txn()
                    .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
                let maybe_block_body = block_store
                    .block_body_dbs
                    .get(&body_txn, block_header.body_hash())
                    .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
                if !should_retain_block {
                    let _ = deleted_block_hashes.insert(block_header.block_hash());

                    match &maybe_block_body {
                        Some(BlockBody::V1(v1_body)) => deleted_transaction_hashes.extend(
                            v1_body
                                .deploy_and_transfer_hashes()
                                .map(TransactionHash::from),
                        ),
                        Some(BlockBody::V2(v2_body)) => {
                            let transactions = v2_body.all_transactions();
                            deleted_transaction_hashes.extend(transactions)
                        }
                        None => (),
                    }

                    cursor
                        .del(WriteFlags::empty())
                        .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
                    return Ok(());
                }

                Self::insert_to_block_header_indices(
                    &mut block_height_index,
                    &mut switch_block_era_id_index,
                    &block_header,
                )?;

                if let Some(block_body) = maybe_block_body {
                    let transaction_hashes = match block_body {
                        BlockBody::V1(v1) => v1
                            .deploy_and_transfer_hashes()
                            .map(TransactionHash::from)
                            .collect(),
                        BlockBody::V2(v2) => v2.all_transactions().copied().collect(),
                    };
                    Self::insert_to_transaction_index(
                        &mut transaction_hash_index,
                        block_header.block_hash(),
                        block_header.height(),
                        block_header.era_id(),
                        transaction_hashes,
                    )?;
                }

                Ok(())
            };

        block_store
            .block_header_dbs
            .for_each_value_in_current(&mut block_txn, &mut init_fn)?;
        block_store
            .block_header_dbs
            .for_each_value_in_legacy(&mut block_txn, &mut init_fn)?;

        info!("block store reindexing complete");
        block_txn
            .commit()
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;

        let deleted_block_body_hashes = block_body_hashes
            .into_iter()
            .filter_map(|(body_hash, retain)| (!retain).then_some(body_hash))
            .collect();
        initialize_block_body_dbs(
            &block_store.env,
            block_store.block_body_dbs,
            deleted_block_body_hashes,
        )?;
        initialize_block_metadata_dbs(
            &block_store.env,
            block_store.block_metadata_dbs,
            deleted_block_hashes,
        )?;
        initialize_execution_result_dbs(
            &block_store.env,
            block_store.execution_result_dbs,
            deleted_transaction_hashes,
        )
        .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;

        Ok(Self {
            block_store,
            block_height_index,
            switch_block_era_id_index,
            transaction_hash_index,
        })
    }
}

/// Purges stale entries from the block body databases.
fn initialize_block_body_dbs(
    env: &Environment,
    block_body_dbs: VersionedDatabases<Digest, BlockBody>,
    deleted_block_body_hashes: HashSet<Digest>,
) -> Result<(), BlockStoreError> {
    info!("initializing block body databases");
    let mut txn = env
        .begin_rw_txn()
        .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
    for body_hash in deleted_block_body_hashes {
        block_body_dbs
            .delete(&mut txn, &body_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
    }
    txn.commit()
        .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
    info!("block body database initialized");
    Ok(())
}

/// Purges stale entries from the block metadata database.
fn initialize_block_metadata_dbs(
    env: &Environment,
    block_metadata_dbs: VersionedDatabases<BlockHash, BlockSignatures>,
    deleted_block_hashes: HashSet<BlockHash>,
) -> Result<(), BlockStoreError> {
    let block_count_to_be_deleted = deleted_block_hashes.len();
    info!(
        block_count_to_be_deleted,
        "initializing block metadata database"
    );
    let mut txn = env
        .begin_rw_txn()
        .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
    for block_hash in deleted_block_hashes {
        block_metadata_dbs
            .delete(&mut txn, &block_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?
    }
    txn.commit()
        .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
    info!("block metadata database initialized");
    Ok(())
}

/// Purges stale entries from the execution result databases.
fn initialize_execution_result_dbs(
    env: &Environment,
    execution_result_dbs: VersionedDatabases<TransactionHash, ExecutionResult>,
    deleted_transaction_hashes: HashSet<TransactionHash>,
) -> Result<(), LmdbExtError> {
    let exec_results_count_to_be_deleted = deleted_transaction_hashes.len();
    info!(
        exec_results_count_to_be_deleted,
        "initializing execution result databases"
    );
    let mut txn = env.begin_rw_txn()?;
    for hash in deleted_transaction_hashes {
        execution_result_dbs.delete(&mut txn, &hash)?;
    }
    txn.commit()?;
    info!("execution result databases initialized");
    Ok(())
}

pub struct IndexedLmdbBlockStoreRWTransaction<'t> {
    txn: RwTransaction<'t>,
    block_store: &'t LmdbBlockStore,
    block_height_index: TempMap<'t, u64, BlockHash>,
    switch_block_era_id_index: TempMap<'t, EraId, BlockHash>,
    transaction_hash_index: TempMap<'t, TransactionHash, BlockHashHeightAndEra>,
}

impl IndexedLmdbBlockStoreRWTransaction<'_> {
    /// Check if the block height index can be updated.
    fn should_update_block_height_index(
        &self,
        block_height: u64,
        block_hash: &BlockHash,
    ) -> Result<bool, BlockStoreError> {
        if let Some(first) = self.block_height_index.get(&block_height) {
            // There is a block in the index at this height
            if first != *block_hash {
                Err(BlockStoreError::DuplicateBlock {
                    height: block_height,
                    first,
                    second: *block_hash,
                })
            } else {
                // Same value already in index, no need to update it.
                Ok(false)
            }
        } else {
            // Value not in index, update.
            Ok(true)
        }
    }

    /// Check if the switch block index can be updated.
    fn should_update_switch_block_index(
        &self,
        block_header: &BlockHeader,
    ) -> Result<bool, BlockStoreError> {
        if block_header.is_switch_block() {
            let era_id = block_header.era_id();
            if let Some(entry) = self.switch_block_era_id_index.get(&era_id) {
                let block_hash = block_header.block_hash();
                if entry != block_hash {
                    Err(BlockStoreError::DuplicateEraId {
                        era_id,
                        first: entry,
                        second: block_hash,
                    })
                } else {
                    // already in index, no need to update.
                    Ok(false)
                }
            } else {
                // not in the index, update.
                Ok(true)
            }
        } else {
            // not a switch block.
            Ok(false)
        }
    }

    // Check if the transaction hash index can be updated.
    fn should_update_transaction_hash_index(
        &self,
        transaction_hashes: &[TransactionHash],
        block_hash: &BlockHash,
    ) -> Result<bool, BlockStoreError> {
        if let Some(hash) = transaction_hashes.iter().find(|hash| {
            self.transaction_hash_index
                .get(hash)
                .is_some_and(|old_details| old_details.block_hash != *block_hash)
        }) {
            return Err(BlockStoreError::DuplicateTransaction {
                transaction_hash: *hash,
                first: self.transaction_hash_index.get(hash).unwrap().block_hash,
                second: *block_hash,
            });
        }
        Ok(true)
    }
}

pub struct IndexedLmdbBlockStoreReadTransaction<'t> {
    txn: RoTransaction<'t>,
    block_store: &'t IndexedLmdbBlockStore,
}

enum LmdbBlockStoreIndex {
    BlockHeight(IndexPosition<u64>),
    SwitchBlockEraId(IndexPosition<EraId>),
}

enum IndexPosition<K> {
    Tip,
    Key(K),
}

enum DataType {
    Block,
    BlockHeader,
    ApprovalsHashes,
    BlockSignatures,
}

impl IndexedLmdbBlockStoreReadTransaction<'_> {
    fn block_hash_from_index(&self, index: LmdbBlockStoreIndex) -> Option<&BlockHash> {
        match index {
            LmdbBlockStoreIndex::BlockHeight(position) => match position {
                IndexPosition::Tip => self.block_store.block_height_index.values().last(),
                IndexPosition::Key(height) => self.block_store.block_height_index.get(&height),
            },
            LmdbBlockStoreIndex::SwitchBlockEraId(position) => match position {
                IndexPosition::Tip => self.block_store.switch_block_era_id_index.values().last(),
                IndexPosition::Key(era_id) => {
                    self.block_store.switch_block_era_id_index.get(&era_id)
                }
            },
        }
    }

    fn read_block_indexed(
        &self,
        index: LmdbBlockStoreIndex,
    ) -> Result<Option<Block>, BlockStoreError> {
        self.block_hash_from_index(index)
            .and_then(|block_hash| {
                self.block_store
                    .block_store
                    .get_single_block(&self.txn, block_hash)
                    .transpose()
            })
            .transpose()
    }

    fn read_block_header_indexed(
        &self,
        index: LmdbBlockStoreIndex,
    ) -> Result<Option<BlockHeader>, BlockStoreError> {
        self.block_hash_from_index(index)
            .and_then(|block_hash| {
                self.block_store
                    .block_store
                    .get_single_block_header(&self.txn, block_hash)
                    .transpose()
            })
            .transpose()
    }

    fn read_block_signatures_indexed(
        &self,
        index: LmdbBlockStoreIndex,
    ) -> Result<Option<BlockSignatures>, BlockStoreError> {
        self.block_hash_from_index(index)
            .and_then(|block_hash| {
                self.block_store
                    .block_store
                    .get_block_signatures(&self.txn, block_hash)
                    .transpose()
            })
            .transpose()
    }

    fn read_approvals_hashes_indexed(
        &self,
        index: LmdbBlockStoreIndex,
    ) -> Result<Option<ApprovalsHashes>, BlockStoreError> {
        self.block_hash_from_index(index)
            .and_then(|block_hash| {
                self.block_store
                    .block_store
                    .read_approvals_hashes(&self.txn, block_hash)
                    .transpose()
            })
            .transpose()
    }

    fn contains_data_indexed(
        &self,
        index: LmdbBlockStoreIndex,
        data_type: DataType,
    ) -> Result<bool, BlockStoreError> {
        self.block_hash_from_index(index)
            .map_or(Ok(false), |block_hash| match data_type {
                DataType::Block => self
                    .block_store
                    .block_store
                    .block_exists(&self.txn, block_hash),
                DataType::BlockHeader => self
                    .block_store
                    .block_store
                    .block_header_exists(&self.txn, block_hash),
                DataType::ApprovalsHashes => self
                    .block_store
                    .block_store
                    .approvals_hashes_exist(&self.txn, block_hash),
                DataType::BlockSignatures => self
                    .block_store
                    .block_store
                    .block_signatures_exist(&self.txn, block_hash),
            })
    }
}

impl BlockStoreTransaction for IndexedLmdbBlockStoreReadTransaction<'_> {
    fn commit(self) -> Result<(), BlockStoreError> {
        Ok(())
    }

    fn rollback(self) {
        self.txn.abort();
    }
}

impl BlockStoreTransaction for IndexedLmdbBlockStoreRWTransaction<'_> {
    fn commit(self) -> Result<(), BlockStoreError> {
        self.txn
            .commit()
            .map_err(|e| BlockStoreError::InternalStorage(Box::new(LmdbExtError::from(e))))?;

        self.block_height_index.commit();
        self.switch_block_era_id_index.commit();
        self.transaction_hash_index.commit();
        Ok(())
    }

    fn rollback(self) {
        self.txn.abort();
    }
}

impl BlockStoreProvider for IndexedLmdbBlockStore {
    type Reader<'t> = IndexedLmdbBlockStoreReadTransaction<'t>;
    type ReaderWriter<'t> = IndexedLmdbBlockStoreRWTransaction<'t>;

    fn checkout_ro(&self) -> Result<Self::Reader<'_>, BlockStoreError> {
        self.get_reader()
    }

    fn checkout_rw(&mut self) -> Result<Self::ReaderWriter<'_>, BlockStoreError> {
        let txn = self
            .block_store
            .env
            .begin_rw_txn()
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;

        Ok(IndexedLmdbBlockStoreRWTransaction {
            txn,
            block_store: &self.block_store,
            block_height_index: TempMap::new(&mut self.block_height_index),
            switch_block_era_id_index: TempMap::new(&mut self.switch_block_era_id_index),
            transaction_hash_index: TempMap::new(&mut self.transaction_hash_index),
        })
    }
}

impl DataReader<BlockHash, Block> for IndexedLmdbBlockStoreReadTransaction<'_> {
    fn read(&self, key: BlockHash) -> Result<Option<Block>, BlockStoreError> {
        self.block_store
            .block_store
            .get_single_block(&self.txn, &key)
    }

    fn exists(&self, key: BlockHash) -> Result<bool, BlockStoreError> {
        self.block_store.block_store.block_exists(&self.txn, &key)
    }
}

impl DataReader<BlockHash, BlockHeader> for IndexedLmdbBlockStoreReadTransaction<'_> {
    fn read(&self, key: BlockHash) -> Result<Option<BlockHeader>, BlockStoreError> {
        self.block_store
            .block_store
            .get_single_block_header(&self.txn, &key)
    }

    fn exists(&self, key: BlockHash) -> Result<bool, BlockStoreError> {
        self.block_store
            .block_store
            .block_header_exists(&self.txn, &key)
    }
}

impl DataReader<BlockHash, ApprovalsHashes> for IndexedLmdbBlockStoreReadTransaction<'_> {
    fn read(&self, key: BlockHash) -> Result<Option<ApprovalsHashes>, BlockStoreError> {
        self.block_store
            .block_store
            .read_approvals_hashes(&self.txn, &key)
    }

    fn exists(&self, key: BlockHash) -> Result<bool, BlockStoreError> {
        self.block_store
            .block_store
            .block_header_exists(&self.txn, &key)
    }
}

impl DataReader<BlockHash, BlockSignatures> for IndexedLmdbBlockStoreReadTransaction<'_> {
    fn read(&self, key: BlockHash) -> Result<Option<BlockSignatures>, BlockStoreError> {
        self.block_store
            .block_store
            .get_block_signatures(&self.txn, &key)
    }

    fn exists(&self, key: BlockHash) -> Result<bool, BlockStoreError> {
        self.block_store
            .block_store
            .block_signatures_exist(&self.txn, &key)
    }
}

impl DataReader<BlockHeight, Block> for IndexedLmdbBlockStoreReadTransaction<'_> {
    fn read(&self, key: BlockHeight) -> Result<Option<Block>, BlockStoreError> {
        self.read_block_indexed(LmdbBlockStoreIndex::BlockHeight(IndexPosition::Key(key)))
    }

    fn exists(&self, key: BlockHeight) -> Result<bool, BlockStoreError> {
        self.contains_data_indexed(
            LmdbBlockStoreIndex::BlockHeight(IndexPosition::Key(key)),
            DataType::Block,
        )
    }
}

impl DataReader<BlockHeight, BlockHeader> for IndexedLmdbBlockStoreReadTransaction<'_> {
    fn read(&self, key: BlockHeight) -> Result<Option<BlockHeader>, BlockStoreError> {
        self.read_block_header_indexed(LmdbBlockStoreIndex::BlockHeight(IndexPosition::Key(key)))
    }

    fn exists(&self, key: BlockHeight) -> Result<bool, BlockStoreError> {
        self.contains_data_indexed(
            LmdbBlockStoreIndex::BlockHeight(IndexPosition::Key(key)),
            DataType::BlockHeader,
        )
    }
}

impl DataReader<BlockHeight, ApprovalsHashes> for IndexedLmdbBlockStoreReadTransaction<'_> {
    fn read(&self, key: BlockHeight) -> Result<Option<ApprovalsHashes>, BlockStoreError> {
        self.read_approvals_hashes_indexed(LmdbBlockStoreIndex::BlockHeight(IndexPosition::Key(
            key,
        )))
    }

    fn exists(&self, key: BlockHeight) -> Result<bool, BlockStoreError> {
        self.contains_data_indexed(
            LmdbBlockStoreIndex::BlockHeight(IndexPosition::Key(key)),
            DataType::ApprovalsHashes,
        )
    }
}

impl DataReader<BlockHeight, BlockSignatures> for IndexedLmdbBlockStoreReadTransaction<'_> {
    fn read(&self, key: BlockHeight) -> Result<Option<BlockSignatures>, BlockStoreError> {
        self.read_block_signatures_indexed(LmdbBlockStoreIndex::BlockHeight(IndexPosition::Key(
            key,
        )))
    }

    fn exists(&self, key: BlockHeight) -> Result<bool, BlockStoreError> {
        self.contains_data_indexed(
            LmdbBlockStoreIndex::BlockHeight(IndexPosition::Key(key)),
            DataType::BlockSignatures,
        )
    }
}

/// Retrieves single switch block by era ID by looking it up in the index and returning it.
impl DataReader<EraId, Block> for IndexedLmdbBlockStoreReadTransaction<'_> {
    fn read(&self, key: EraId) -> Result<Option<Block>, BlockStoreError> {
        self.read_block_indexed(LmdbBlockStoreIndex::SwitchBlockEraId(IndexPosition::Key(
            key,
        )))
    }

    fn exists(&self, key: EraId) -> Result<bool, BlockStoreError> {
        self.contains_data_indexed(
            LmdbBlockStoreIndex::SwitchBlockEraId(IndexPosition::Key(key)),
            DataType::Block,
        )
    }
}

/// Retrieves single switch block header by era ID by looking it up in the index and returning
/// it.
impl DataReader<EraId, BlockHeader> for IndexedLmdbBlockStoreReadTransaction<'_> {
    fn read(&self, key: EraId) -> Result<Option<BlockHeader>, BlockStoreError> {
        self.read_block_header_indexed(LmdbBlockStoreIndex::SwitchBlockEraId(IndexPosition::Key(
            key,
        )))
    }

    fn exists(&self, key: EraId) -> Result<bool, BlockStoreError> {
        self.contains_data_indexed(
            LmdbBlockStoreIndex::SwitchBlockEraId(IndexPosition::Key(key)),
            DataType::BlockHeader,
        )
    }
}

impl DataReader<EraId, ApprovalsHashes> for IndexedLmdbBlockStoreReadTransaction<'_> {
    fn read(&self, key: EraId) -> Result<Option<ApprovalsHashes>, BlockStoreError> {
        self.read_approvals_hashes_indexed(LmdbBlockStoreIndex::SwitchBlockEraId(
            IndexPosition::Key(key),
        ))
    }

    fn exists(&self, key: EraId) -> Result<bool, BlockStoreError> {
        self.contains_data_indexed(
            LmdbBlockStoreIndex::SwitchBlockEraId(IndexPosition::Key(key)),
            DataType::ApprovalsHashes,
        )
    }
}

impl DataReader<EraId, BlockSignatures> for IndexedLmdbBlockStoreReadTransaction<'_> {
    fn read(&self, key: EraId) -> Result<Option<BlockSignatures>, BlockStoreError> {
        self.read_block_signatures_indexed(LmdbBlockStoreIndex::SwitchBlockEraId(
            IndexPosition::Key(key),
        ))
    }

    fn exists(&self, key: EraId) -> Result<bool, BlockStoreError> {
        self.contains_data_indexed(
            LmdbBlockStoreIndex::SwitchBlockEraId(IndexPosition::Key(key)),
            DataType::BlockSignatures,
        )
    }
}

impl DataReader<Tip, BlockHeader> for IndexedLmdbBlockStoreReadTransaction<'_> {
    fn read(&self, _key: Tip) -> Result<Option<BlockHeader>, BlockStoreError> {
        self.read_block_header_indexed(LmdbBlockStoreIndex::BlockHeight(IndexPosition::Tip))
    }

    fn exists(&self, _key: Tip) -> Result<bool, BlockStoreError> {
        self.contains_data_indexed(
            LmdbBlockStoreIndex::BlockHeight(IndexPosition::Tip),
            DataType::BlockHeader,
        )
    }
}

impl DataReader<Tip, Block> for IndexedLmdbBlockStoreReadTransaction<'_> {
    fn read(&self, _key: Tip) -> Result<Option<Block>, BlockStoreError> {
        self.read_block_indexed(LmdbBlockStoreIndex::BlockHeight(IndexPosition::Tip))
    }

    fn exists(&self, _key: Tip) -> Result<bool, BlockStoreError> {
        self.contains_data_indexed(
            LmdbBlockStoreIndex::BlockHeight(IndexPosition::Tip),
            DataType::Block,
        )
    }
}

impl DataReader<LatestSwitchBlock, BlockHeader> for IndexedLmdbBlockStoreReadTransaction<'_> {
    fn read(&self, _key: LatestSwitchBlock) -> Result<Option<BlockHeader>, BlockStoreError> {
        self.read_block_header_indexed(LmdbBlockStoreIndex::SwitchBlockEraId(IndexPosition::Tip))
    }

    fn exists(&self, _key: LatestSwitchBlock) -> Result<bool, BlockStoreError> {
        self.contains_data_indexed(
            LmdbBlockStoreIndex::SwitchBlockEraId(IndexPosition::Tip),
            DataType::BlockHeader,
        )
    }
}

impl DataReader<TransactionHash, BlockHashHeightAndEra>
    for IndexedLmdbBlockStoreReadTransaction<'_>
{
    fn read(&self, key: TransactionHash) -> Result<Option<BlockHashHeightAndEra>, BlockStoreError> {
        Ok(self.block_store.transaction_hash_index.get(&key).copied())
    }

    fn exists(&self, key: TransactionHash) -> Result<bool, BlockStoreError> {
        Ok(self.block_store.transaction_hash_index.contains_key(&key))
    }
}

impl DataReader<TransactionHash, Transaction> for IndexedLmdbBlockStoreReadTransaction<'_> {
    fn read(&self, key: TransactionHash) -> Result<Option<Transaction>, BlockStoreError> {
        self.block_store
            .block_store
            .transaction_dbs
            .get(&self.txn, &key)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    fn exists(&self, key: TransactionHash) -> Result<bool, BlockStoreError> {
        self.block_store
            .block_store
            .transaction_exists(&self.txn, &key)
    }
}

impl DataReader<TransactionHash, BTreeSet<Approval>> for IndexedLmdbBlockStoreReadTransaction<'_> {
    fn read(&self, key: TransactionHash) -> Result<Option<BTreeSet<Approval>>, BlockStoreError> {
        self.block_store
            .block_store
            .finalized_transaction_approvals_dbs
            .get(&self.txn, &key)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    fn exists(&self, key: TransactionHash) -> Result<bool, BlockStoreError> {
        self.block_store
            .block_store
            .finalized_transaction_approvals_dbs
            .exists(&self.txn, &key)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }
}

impl DataReader<TransactionHash, ExecutionResult> for IndexedLmdbBlockStoreReadTransaction<'_> {
    fn read(&self, key: TransactionHash) -> Result<Option<ExecutionResult>, BlockStoreError> {
        self.block_store
            .block_store
            .execution_result_dbs
            .get(&self.txn, &key)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    fn exists(&self, key: TransactionHash) -> Result<bool, BlockStoreError> {
        self.block_store
            .block_store
            .execution_result_dbs
            .exists(&self.txn, &key)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }
}

impl DataReader<StateStoreKey, Vec<u8>> for IndexedLmdbBlockStoreReadTransaction<'_> {
    fn read(&self, StateStoreKey(key): StateStoreKey) -> Result<Option<Vec<u8>>, BlockStoreError> {
        self.block_store
            .block_store
            .read_state_store(&self.txn, &key)
    }

    fn exists(&self, StateStoreKey(key): StateStoreKey) -> Result<bool, BlockStoreError> {
        self.block_store
            .block_store
            .state_store_key_exists(&self.txn, &key)
    }
}

impl DataReader<(DbTableId, Vec<u8>), DbRawBytesSpec> for IndexedLmdbBlockStoreReadTransaction<'_> {
    fn read(
        &self,
        (id, key): (DbTableId, Vec<u8>),
    ) -> Result<Option<DbRawBytesSpec>, BlockStoreError> {
        if key.is_empty() {
            return Ok(None);
        }
        let store = &self.block_store.block_store;
        let res = match id {
            DbTableId::BlockHeader => store.block_header_dbs.get_raw(&self.txn, &key),
            DbTableId::BlockBody => store.block_body_dbs.get_raw(&self.txn, &key),
            DbTableId::ApprovalsHashes => store.approvals_hashes_dbs.get_raw(&self.txn, &key),
            DbTableId::BlockMetadata => store.block_metadata_dbs.get_raw(&self.txn, &key),
            DbTableId::Transaction => store.transaction_dbs.get_raw(&self.txn, &key),
            DbTableId::ExecutionResult => store.execution_result_dbs.get_raw(&self.txn, &key),
            DbTableId::Transfer => store.transfer_dbs.get_raw(&self.txn, &key),
            DbTableId::FinalizedTransactionApprovals => store
                .finalized_transaction_approvals_dbs
                .get_raw(&self.txn, &key),
        };
        res.map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    fn exists(&self, key: (DbTableId, Vec<u8>)) -> Result<bool, BlockStoreError> {
        self.read(key).map(|res| res.is_some())
    }
}

impl DataWriter<BlockHash, Block> for IndexedLmdbBlockStoreRWTransaction<'_> {
    /// Writes a block to storage.
    ///
    /// Returns `Ok(true)` if the block has been successfully written, `Ok(false)` if a part of it
    /// couldn't be written because it already existed, and `Err(_)` if there was an error.
    fn write(&mut self, data: &Block) -> Result<BlockHash, BlockStoreError> {
        let block_header = data.clone_header();
        let block_hash = data.hash();
        let block_height = data.height();
        let era_id = data.era_id();
        let transaction_hashes: Vec<TransactionHash> = match &data {
            Block::V1(v1) => v1
                .deploy_and_transfer_hashes()
                .map(TransactionHash::from)
                .collect(),
            Block::V2(v2) => v2.all_transactions().copied().collect(),
        };

        let update_height_index =
            self.should_update_block_height_index(block_height, block_hash)?;
        let update_switch_block_index = self.should_update_switch_block_index(&block_header)?;
        let update_transaction_hash_index =
            self.should_update_transaction_hash_index(&transaction_hashes, block_hash)?;

        let key = self.block_store.write_block(&mut self.txn, data)?;

        if update_height_index {
            self.block_height_index.insert(block_height, *block_hash);
        }

        if update_switch_block_index {
            self.switch_block_era_id_index.insert(era_id, *block_hash);
        }

        if update_transaction_hash_index {
            for hash in transaction_hashes {
                self.transaction_hash_index.insert(
                    hash,
                    BlockHashHeightAndEra::new(*block_hash, block_height, era_id),
                );
            }
        }

        Ok(key)
    }

    fn delete(&mut self, key: BlockHash) -> Result<(), BlockStoreError> {
        let maybe_block = self.block_store.get_single_block(&self.txn, &key)?;

        if let Some(block) = maybe_block {
            let transaction_hashes: Vec<TransactionHash> = match &block {
                Block::V1(v1) => v1
                    .deploy_and_transfer_hashes()
                    .map(TransactionHash::from)
                    .collect(),
                Block::V2(v2) => v2.all_transactions().copied().collect(),
            };

            self.block_store.delete_block_header(&mut self.txn, &key)?;

            /*
            TODO: currently we don't delete the block body since other blocks may reference it.
            self.block_store
                .delete_block_body(&mut self.txn, block.body_hash())?;
            */

            self.block_height_index.remove(block.height());

            if block.is_switch_block() {
                self.switch_block_era_id_index.remove(block.era_id());
            }

            for hash in transaction_hashes {
                self.transaction_hash_index.remove(hash);
            }

            self.block_store
                .delete_finality_signatures(&mut self.txn, &key)?;
        }
        Ok(())
    }
}

impl DataWriter<BlockHash, ApprovalsHashes> for IndexedLmdbBlockStoreRWTransaction<'_> {
    fn write(&mut self, data: &ApprovalsHashes) -> Result<BlockHash, BlockStoreError> {
        self.block_store.write_approvals_hashes(&mut self.txn, data)
    }

    fn delete(&mut self, key: BlockHash) -> Result<(), BlockStoreError> {
        self.block_store
            .delete_approvals_hashes(&mut self.txn, &key)
    }
}

impl DataWriter<BlockHash, BlockSignatures> for IndexedLmdbBlockStoreRWTransaction<'_> {
    fn write(&mut self, data: &BlockSignatures) -> Result<BlockHash, BlockStoreError> {
        self.block_store
            .write_finality_signatures(&mut self.txn, data)
    }

    fn delete(&mut self, key: BlockHash) -> Result<(), BlockStoreError> {
        self.block_store
            .delete_finality_signatures(&mut self.txn, &key)
    }
}

impl DataWriter<BlockHash, BlockHeader> for IndexedLmdbBlockStoreRWTransaction<'_> {
    fn write(&mut self, data: &BlockHeader) -> Result<BlockHash, BlockStoreError> {
        let block_hash = data.block_hash();
        let block_height = data.height();
        let era_id = data.era_id();

        let update_height_index =
            self.should_update_block_height_index(block_height, &block_hash)?;
        let update_switch_block_index = self.should_update_switch_block_index(data)?;

        let key = self.block_store.write_block_header(&mut self.txn, data)?;

        if update_height_index {
            self.block_height_index.insert(block_height, block_hash);
        }

        if update_switch_block_index {
            self.switch_block_era_id_index.insert(era_id, block_hash);
        }

        Ok(key)
    }

    fn delete(&mut self, key: BlockHash) -> Result<(), BlockStoreError> {
        let maybe_block_header = self.block_store.get_single_block_header(&self.txn, &key)?;

        if let Some(block_header) = maybe_block_header {
            self.block_store.delete_block_header(&mut self.txn, &key)?;

            if block_header.is_switch_block() {
                self.switch_block_era_id_index.remove(block_header.era_id());
            }

            self.block_height_index.remove(block_header.height());
        }
        Ok(())
    }
}

impl DataWriter<TransactionHash, Transaction> for IndexedLmdbBlockStoreRWTransaction<'_> {
    fn write(&mut self, data: &Transaction) -> Result<TransactionHash, BlockStoreError> {
        self.block_store.write_transaction(&mut self.txn, data)
    }

    fn delete(&mut self, key: TransactionHash) -> Result<(), BlockStoreError> {
        self.block_store.delete_transaction(&mut self.txn, &key)
    }
}

impl DataWriter<TransactionHash, TransactionFinalizedApprovals>
    for IndexedLmdbBlockStoreRWTransaction<'_>
{
    fn write(
        &mut self,
        data: &TransactionFinalizedApprovals,
    ) -> Result<TransactionHash, BlockStoreError> {
        self.block_store
            .finalized_transaction_approvals_dbs
            .put(
                &mut self.txn,
                &data.transaction_hash,
                &data.finalized_approvals,
                true,
            )
            .map(|_| data.transaction_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    fn delete(&mut self, key: TransactionHash) -> Result<(), BlockStoreError> {
        self.block_store
            .finalized_transaction_approvals_dbs
            .delete(&mut self.txn, &key)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }
}

impl DataWriter<BlockHashHeightAndEra, BlockExecutionResults>
    for IndexedLmdbBlockStoreRWTransaction<'_>
{
    fn write(
        &mut self,
        data: &BlockExecutionResults,
    ) -> Result<BlockHashHeightAndEra, BlockStoreError> {
        let transaction_hashes: Vec<TransactionHash> = data.exec_results.keys().copied().collect();
        let block_hash = data.block_info.block_hash;
        let block_height = data.block_info.block_height;
        let era_id = data.block_info.era_id;

        let update_transaction_hash_index =
            self.should_update_transaction_hash_index(&transaction_hashes, &block_hash)?;

        let _ = self.block_store.write_execution_results(
            &mut self.txn,
            &block_hash,
            data.exec_results.clone(),
        )?;

        if update_transaction_hash_index {
            for hash in transaction_hashes {
                self.transaction_hash_index.insert(
                    hash,
                    BlockHashHeightAndEra::new(block_hash, block_height, era_id),
                );
            }
        }

        Ok(data.block_info)
    }

    fn delete(&mut self, _key: BlockHashHeightAndEra) -> Result<(), BlockStoreError> {
        Err(BlockStoreError::UnsupportedOperation)
    }
}

impl DataWriter<BlockHash, BlockTransfers> for IndexedLmdbBlockStoreRWTransaction<'_> {
    fn write(&mut self, data: &BlockTransfers) -> Result<BlockHash, BlockStoreError> {
        self.block_store
            .write_transfers(&mut self.txn, &data.block_hash, &data.transfers)
            .map(|_| data.block_hash)
    }

    fn delete(&mut self, key: BlockHash) -> Result<(), BlockStoreError> {
        self.block_store.delete_transfers(&mut self.txn, &key)
    }
}

impl DataWriter<Cow<'static, [u8]>, StateStore> for IndexedLmdbBlockStoreRWTransaction<'_> {
    fn write(&mut self, data: &StateStore) -> Result<Cow<'static, [u8]>, BlockStoreError> {
        self.block_store
            .write_state_store(&mut self.txn, data.key.clone(), &data.value)?;
        Ok(data.key.clone())
    }

    fn delete(&mut self, key: Cow<'static, [u8]>) -> Result<(), BlockStoreError> {
        self.block_store.delete_state_store(&mut self.txn, key)
    }
}

impl DataReader<TransactionHash, Transaction> for IndexedLmdbBlockStoreRWTransaction<'_> {
    fn read(&self, query: TransactionHash) -> Result<Option<Transaction>, BlockStoreError> {
        self.block_store
            .transaction_dbs
            .get(&self.txn, &query)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    fn exists(&self, query: TransactionHash) -> Result<bool, BlockStoreError> {
        self.block_store.transaction_exists(&self.txn, &query)
    }
}

impl DataReader<BlockHash, BlockSignatures> for IndexedLmdbBlockStoreRWTransaction<'_> {
    fn read(&self, key: BlockHash) -> Result<Option<BlockSignatures>, BlockStoreError> {
        self.block_store.get_block_signatures(&self.txn, &key)
    }

    fn exists(&self, key: BlockHash) -> Result<bool, BlockStoreError> {
        self.block_store.block_signatures_exist(&self.txn, &key)
    }
}

impl DataReader<TransactionHash, BTreeSet<Approval>> for IndexedLmdbBlockStoreRWTransaction<'_> {
    fn read(&self, query: TransactionHash) -> Result<Option<BTreeSet<Approval>>, BlockStoreError> {
        self.block_store
            .finalized_transaction_approvals_dbs
            .get(&self.txn, &query)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    fn exists(&self, query: TransactionHash) -> Result<bool, BlockStoreError> {
        self.block_store
            .finalized_transaction_approvals_dbs
            .exists(&self.txn, &query)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }
}

impl DataReader<BlockHash, Block> for IndexedLmdbBlockStoreRWTransaction<'_> {
    fn read(&self, key: BlockHash) -> Result<Option<Block>, BlockStoreError> {
        self.block_store.get_single_block(&self.txn, &key)
    }

    fn exists(&self, key: BlockHash) -> Result<bool, BlockStoreError> {
        self.block_store.block_exists(&self.txn, &key)
    }
}

impl DataReader<BlockHash, BlockHeader> for IndexedLmdbBlockStoreRWTransaction<'_> {
    fn read(&self, key: BlockHash) -> Result<Option<BlockHeader>, BlockStoreError> {
        self.block_store.get_single_block_header(&self.txn, &key)
    }

    fn exists(&self, key: BlockHash) -> Result<bool, BlockStoreError> {
        self.block_store.block_header_exists(&self.txn, &key)
    }
}

impl DataReader<TransactionHash, ExecutionResult> for IndexedLmdbBlockStoreRWTransaction<'_> {
    fn read(&self, query: TransactionHash) -> Result<Option<ExecutionResult>, BlockStoreError> {
        self.block_store
            .execution_result_dbs
            .get(&self.txn, &query)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    fn exists(&self, query: TransactionHash) -> Result<bool, BlockStoreError> {
        self.block_store
            .execution_result_dbs
            .exists(&self.txn, &query)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }
}

impl DataReader<BlockHash, Vec<Transfer>> for IndexedLmdbBlockStoreRWTransaction<'_> {
    fn read(&self, key: BlockHash) -> Result<Option<Vec<Transfer>>, BlockStoreError> {
        self.block_store.get_transfers(&self.txn, &key)
    }

    fn exists(&self, key: BlockHash) -> Result<bool, BlockStoreError> {
        self.block_store.has_transfers(&self.txn, &key)
    }
}
