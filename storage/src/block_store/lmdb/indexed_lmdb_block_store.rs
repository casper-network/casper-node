use std::{
    borrow::Cow,
    collections::{btree_map, hash_map::Entry, BTreeMap, HashMap, HashSet},
};

use super::{lmdb_block_store::LmdbBlockStore, lmdb_ext::LmdbExtError, temp_map::TempMap};
use datasize::DataSize;
use lmdb::{
    Database, Environment, RoTransaction, RwCursor, RwTransaction, Transaction as LmdbTransaction,
    WriteFlags,
};

use tracing::{debug, info, trace};

use super::versioned_databases::VersionedDatabases;
use crate::block_store::{
    block_provider::BlockStoreTransaction,
    indices::{BlockHeight, IndexedBy, SwitchBlock, SwitchBlockHeader},
    types::{ApprovalsHashes, BlockHashHeightAndEra},
    BlockStoreError, BlockStoreProvider, BlockStoreReader, BlockStoreWriter,
    IndexedBlockStoreProvider,
};
use casper_types::{
    execution::ExecutionResult, Block, BlockBody, BlockHash, BlockHeader, BlockSignatures, Digest,
    EraId, FinalitySignature, FinalizedApprovals, ProtocolVersion, Transaction, TransactionHash,
    TransactionHeader, TransactionWithFinalizedApprovals, Transfer,
};

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
                .map_or(false, |old_details| old_details.block_hash != block_hash)
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

    /// Retrieves single switch block by era ID by looking it up in the index and returning it.
    fn get_switch_block_by_era_id<Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
        era_id: EraId,
    ) -> Result<Option<Block>, BlockStoreError> {
        self.switch_block_era_id_index
            .get(&era_id)
            .and_then(|block_hash| {
                self.block_store
                    .get_single_block(txn, block_hash)
                    .transpose()
            })
            .transpose()
    }

    /// Retrieves single switch block header by era ID by looking it up in the index and returning
    /// it.
    fn get_switch_block_header_by_era_id<Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
        era_id: EraId,
    ) -> Result<Option<BlockHeader>, BlockStoreError> {
        trace!(switch_block_era_id_index = ?self.switch_block_era_id_index);
        let ret = self
            .switch_block_era_id_index
            .get(&era_id)
            .and_then(|block_hash| {
                self.block_store
                    .get_single_block_header(txn, block_hash)
                    .transpose()
            })
            .transpose();
        if let Ok(maybe) = &ret {
            debug!(
                "Storage: get_switch_block_header_by_era_id({:?}) has entry:{}",
                era_id,
                maybe.is_some()
            )
        }
        ret
    }

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

                let mut body_txn = block_store
                    .env
                    .begin_ro_txn()
                    .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
                let maybe_block_body = block_store
                    .block_body_dbs
                    .get(&mut body_txn, block_header.body_hash())
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
                            deleted_transaction_hashes.extend(v2_body.all_transactions().copied())
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
        initialize_block_metadata_db(
            &block_store.env,
            block_store.block_metadata_db,
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
fn initialize_block_metadata_db(
    env: &Environment,
    block_metadata_db: Database,
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
        match txn.del(block_metadata_db, &block_hash, None) {
            Ok(()) | Err(lmdb::Error::NotFound) => {}
            Err(error) => return Err(BlockStoreError::InternalStorage(Box::new(error))),
        }
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
pub struct IndexedLmdbBlockStoreRWTransaction<'a> {
    txn: RwTransaction<'a>,
    block_store: &'a LmdbBlockStore,
    block_height_index: TempMap<'a, u64, BlockHash>,
    switch_block_era_id_index: TempMap<'a, EraId, BlockHash>,
    transaction_hash_index: TempMap<'a, TransactionHash, BlockHashHeightAndEra>,
}

impl<'a> IndexedLmdbBlockStoreRWTransaction<'a> {
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
                .map_or(false, |old_details| old_details.block_hash != *block_hash)
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

pub struct IndexedLmdbBlockStoreReadTransaction<'a> {
    txn: RoTransaction<'a>,
    block_store: &'a IndexedLmdbBlockStore,
}

impl<'a> BlockStoreTransaction for IndexedLmdbBlockStoreReadTransaction<'a> {
    fn commit(self) -> Result<(), BlockStoreError> {
        Ok(())
    }

    fn rollback(self) {
        self.txn.abort();
    }
}

impl<'a> BlockStoreReader for IndexedLmdbBlockStoreReadTransaction<'a> {
    fn read_block(&mut self, block_hash: &BlockHash) -> Result<Option<Block>, BlockStoreError> {
        self.block_store
            .block_store
            .get_single_block(&mut self.txn, block_hash)
    }

    fn read_block_header(
        &mut self,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockHeader>, BlockStoreError> {
        self.block_store
            .block_store
            .get_single_block_header(&mut self.txn, block_hash)
    }

    fn read_block_signatures(
        &mut self,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockSignatures>, BlockStoreError> {
        self.block_store
            .block_store
            .get_block_signatures(&mut self.txn, block_hash)
    }

    fn read_transaction_with_finalized_approvals(
        &mut self,
        transaction_hash: &TransactionHash,
    ) -> Result<Option<TransactionWithFinalizedApprovals>, BlockStoreError> {
        self.block_store
            .block_store
            .get_transaction_with_finalized_approvals(&mut self.txn, transaction_hash)
    }

    fn read_execution_result(
        &mut self,
        transaction_hash: &TransactionHash,
    ) -> Result<Option<ExecutionResult>, BlockStoreError> {
        self.block_store
            .block_store
            .execution_result_dbs
            .get(&mut self.txn, transaction_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    fn read_transaction(
        &mut self,
        transaction_hash: &TransactionHash,
    ) -> Result<Option<Transaction>, BlockStoreError> {
        self.block_store
            .block_store
            .transaction_dbs
            .get(&mut self.txn, transaction_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    fn read_transfers(
        &mut self,
        block_hash: &BlockHash,
    ) -> Result<Option<Vec<Transfer>>, BlockStoreError> {
        self.block_store
            .block_store
            .get_transfers(&mut self.txn, block_hash)
    }

    fn read_finalized_approvals(
        &mut self,
        transaction_hash: &TransactionHash,
    ) -> Result<Option<FinalizedApprovals>, BlockStoreError> {
        self.block_store
            .block_store
            .finalized_transaction_approvals_dbs
            .get(&mut self.txn, transaction_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    fn read_state_store<K: AsRef<[u8]>>(
        &self,
        key: &K,
    ) -> Result<Option<Vec<u8>>, BlockStoreError> {
        self.block_store
            .block_store
            .read_state_store(&self.txn, key)
    }

    fn read_approvals_hashes(
        &mut self,
        block_hash: &BlockHash,
    ) -> Result<Option<ApprovalsHashes>, BlockStoreError> {
        self.block_store
            .block_store
            .read_approvals_hashes(&mut self.txn, block_hash)
    }

    fn block_exists(&mut self, block_hash: &BlockHash) -> Result<bool, BlockStoreError> {
        self.block_store
            .block_store
            .block_exists(&mut self.txn, block_hash)
    }

    fn transaction_exists(
        &mut self,
        transaction_hash: &TransactionHash,
    ) -> Result<bool, BlockStoreError> {
        self.block_store
            .block_store
            .transaction_exists(&mut self.txn, transaction_hash)
    }

    fn read_execution_results(
        &mut self,
        block_hash: &BlockHash,
    ) -> Result<Option<Vec<(TransactionHash, TransactionHeader, ExecutionResult)>>, BlockStoreError>
    {
        self.block_store
            .block_store
            .read_execution_results(&mut self.txn, block_hash)
    }
}

impl<'a> BlockStoreTransaction for IndexedLmdbBlockStoreRWTransaction<'a> {
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

impl<'a> BlockStoreReader for IndexedLmdbBlockStoreRWTransaction<'a> {
    fn read_block(&mut self, block_hash: &BlockHash) -> Result<Option<Block>, BlockStoreError> {
        self.block_store.get_single_block(&mut self.txn, block_hash)
    }

    fn read_block_header(
        &mut self,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockHeader>, BlockStoreError> {
        self.block_store
            .get_single_block_header(&mut self.txn, block_hash)
    }

    fn read_block_signatures(
        &mut self,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockSignatures>, BlockStoreError> {
        self.block_store
            .get_block_signatures(&mut self.txn, block_hash)
    }

    fn read_transaction_with_finalized_approvals(
        &mut self,
        transaction_hash: &TransactionHash,
    ) -> Result<Option<TransactionWithFinalizedApprovals>, BlockStoreError> {
        self.block_store
            .get_transaction_with_finalized_approvals(&mut self.txn, transaction_hash)
    }

    fn read_execution_result(
        &mut self,
        transaction_hash: &TransactionHash,
    ) -> Result<Option<ExecutionResult>, BlockStoreError> {
        self.block_store
            .execution_result_dbs
            .get(&mut self.txn, transaction_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    fn read_transaction(
        &mut self,
        transaction_hash: &TransactionHash,
    ) -> Result<Option<Transaction>, BlockStoreError> {
        self.block_store
            .transaction_dbs
            .get(&mut self.txn, transaction_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    fn read_transfers(
        &mut self,
        block_hash: &BlockHash,
    ) -> Result<Option<Vec<Transfer>>, BlockStoreError> {
        self.block_store.get_transfers(&mut self.txn, block_hash)
    }

    fn read_finalized_approvals(
        &mut self,
        transaction_hash: &TransactionHash,
    ) -> Result<Option<FinalizedApprovals>, BlockStoreError> {
        self.block_store
            .finalized_transaction_approvals_dbs
            .get(&mut self.txn, transaction_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    fn read_state_store<K: AsRef<[u8]>>(
        &self,
        key: &K,
    ) -> Result<Option<Vec<u8>>, BlockStoreError> {
        self.block_store.read_state_store(&self.txn, key)
    }

    fn read_approvals_hashes(
        &mut self,
        block_hash: &BlockHash,
    ) -> Result<Option<ApprovalsHashes>, BlockStoreError> {
        self.block_store
            .read_approvals_hashes(&mut self.txn, block_hash)
    }

    fn block_exists(&mut self, block_hash: &BlockHash) -> Result<bool, BlockStoreError> {
        self.block_store.block_exists(&mut self.txn, block_hash)
    }

    fn transaction_exists(
        &mut self,
        transaction_hash: &TransactionHash,
    ) -> Result<bool, BlockStoreError> {
        self.block_store
            .transaction_exists(&mut self.txn, transaction_hash)
    }

    fn read_execution_results(
        &mut self,
        block_hash: &BlockHash,
    ) -> Result<Option<Vec<(TransactionHash, TransactionHeader, ExecutionResult)>>, BlockStoreError>
    {
        self.block_store
            .read_execution_results(&mut self.txn, block_hash)
    }
}

impl<'a> BlockStoreWriter for IndexedLmdbBlockStoreRWTransaction<'a> {
    /// Writes a block to storage.
    ///
    /// Returns `Ok(true)` if the block has been successfully written, `Ok(false)` if a part of it
    /// couldn't be written because it already existed, and `Err(_)` if there was an error.
    fn write_block(&mut self, block: &Block) -> Result<bool, BlockStoreError> {
        let block_header = block.clone_header();
        let block_hash = block.hash();
        let block_height = block.height();
        let era_id = block.era_id();
        let transaction_hashes: Vec<TransactionHash> = match block {
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

        if !self.block_store.write_block(&mut self.txn, block)? {
            return Ok(false);
        }

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

        Ok(true)
    }

    fn write_approvals_hashes(
        &mut self,
        approvals_hashes: &ApprovalsHashes,
    ) -> Result<bool, BlockStoreError> {
        self.block_store
            .write_approvals_hashes(&mut self.txn, approvals_hashes)
    }

    fn write_execution_results(
        &mut self,
        block_hash: &BlockHash,
        block_height: u64,
        era_id: EraId,
        execution_results: HashMap<TransactionHash, ExecutionResult>,
    ) -> Result<bool, BlockStoreError> {
        let transaction_hashes: Vec<TransactionHash> = execution_results.keys().copied().collect();

        if let Some(hash) = transaction_hashes.iter().find(|hash| {
            self.transaction_hash_index
                .get(hash)
                .map_or(false, |old_details| old_details.block_hash != *block_hash)
        }) {
            return Err(BlockStoreError::DuplicateTransaction {
                transaction_hash: *hash,
                first: self.transaction_hash_index.get(hash).unwrap().block_hash,
                second: *block_hash,
            });
        }

        let update_transaction_hash_index =
            self.should_update_transaction_hash_index(&transaction_hashes, block_hash)?;

        if !self.block_store.write_execution_results(
            &mut self.txn,
            block_hash,
            execution_results,
        )? {
            return Ok(false);
        }

        if update_transaction_hash_index {
            for hash in transaction_hashes {
                self.transaction_hash_index.insert(
                    hash,
                    BlockHashHeightAndEra::new(*block_hash, block_height, era_id),
                );
            }
        }

        Ok(true)
    }

    fn write_block_signatures(
        &mut self,
        block_signatures: &BlockSignatures,
    ) -> Result<bool, BlockStoreError> {
        self.block_store
            .write_finality_signatures(&mut self.txn, block_signatures)
    }

    fn write_finalized_approvals(
        &mut self,
        transaction_hash: &TransactionHash,
        finalized_approvals: &FinalizedApprovals,
    ) -> Result<bool, BlockStoreError> {
        self.block_store
            .finalized_transaction_approvals_dbs
            .put(&mut self.txn, transaction_hash, finalized_approvals, true)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    fn write_state_store(
        &mut self,
        key: Cow<'static, [u8]>,
        data: &Vec<u8>,
    ) -> Result<(), BlockStoreError> {
        self.block_store.write_state_store(&mut self.txn, key, data)
    }

    fn write_block_header(&mut self, block_header: BlockHeader) -> Result<bool, BlockStoreError> {
        let block_hash = block_header.block_hash();
        let block_height = block_header.height();
        let era_id = block_header.era_id();

        let update_height_index =
            self.should_update_block_height_index(block_height, &block_hash)?;
        let update_switch_block_index = self.should_update_switch_block_index(&block_header)?;

        if !self
            .block_store
            .write_block_header(&mut self.txn, &block_header)?
        {
            return Ok(false);
        };

        if update_height_index {
            self.block_height_index.insert(block_height, block_hash);
        }

        if update_switch_block_index {
            self.switch_block_era_id_index.insert(era_id, block_hash);
        }

        Ok(true)
    }

    fn write_finality_signature(
        &mut self,
        signature: Box<FinalitySignature>,
    ) -> Result<bool, BlockStoreError> {
        self.block_store
            .write_finality_signature(&mut self.txn, signature)
    }

    fn write_transaction(&mut self, transaction: &Transaction) -> Result<bool, BlockStoreError> {
        self.block_store
            .write_transaction(&mut self.txn, transaction)
    }

    fn write_transfers(
        &mut self,
        block_hash: &BlockHash,
        transfers: &Vec<Transfer>,
    ) -> Result<bool, BlockStoreError> {
        self.block_store
            .write_transfers(&mut self.txn, block_hash, transfers)
    }
}

impl BlockStoreProvider for IndexedLmdbBlockStore {
    type Reader<'a> = IndexedLmdbBlockStoreReadTransaction<'a>;
    type Writer<'a> = IndexedLmdbBlockStoreRWTransaction<'a>;
    type ReaderWriter<'a> = IndexedLmdbBlockStoreRWTransaction<'a>;

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

    fn checkout_w(&mut self) -> Result<Self::Writer<'_>, BlockStoreError> {
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

impl IndexedBlockStoreProvider<EraId, SwitchBlockHeader> for IndexedLmdbBlockStore {
    type IndexedBlockStoreReader<'a> = IndexedLmdbBlockStoreReadTransaction<'a>;

    fn checkout_ro_indexed(&self) -> Result<Self::IndexedBlockStoreReader<'_>, BlockStoreError> {
        self.get_reader()
    }
}

impl IndexedBlockStoreProvider<EraId, SwitchBlock> for IndexedLmdbBlockStore {
    type IndexedBlockStoreReader<'a> = IndexedLmdbBlockStoreReadTransaction<'a>;

    fn checkout_ro_indexed(&self) -> Result<Self::IndexedBlockStoreReader<'_>, BlockStoreError> {
        self.get_reader()
    }
}

impl IndexedBlockStoreProvider<TransactionHash, EraId> for IndexedLmdbBlockStore {
    type IndexedBlockStoreReader<'a> = IndexedLmdbBlockStoreReadTransaction<'a>;

    fn checkout_ro_indexed(&self) -> Result<Self::IndexedBlockStoreReader<'_>, BlockStoreError> {
        self.get_reader()
    }
}

impl IndexedBlockStoreProvider<BlockHeight, BlockHash> for IndexedLmdbBlockStore {
    type IndexedBlockStoreReader<'a> = IndexedLmdbBlockStoreReadTransaction<'a>;

    fn checkout_ro_indexed(&self) -> Result<Self::IndexedBlockStoreReader<'_>, BlockStoreError> {
        self.get_reader()
    }
}

impl IndexedBlockStoreProvider<TransactionHash, BlockHashHeightAndEra> for IndexedLmdbBlockStore {
    type IndexedBlockStoreReader<'a> = IndexedLmdbBlockStoreReadTransaction<'a>;

    fn checkout_ro_indexed(&self) -> Result<Self::IndexedBlockStoreReader<'_>, BlockStoreError> {
        self.get_reader()
    }
}

impl IndexedBlockStoreProvider<BlockHeight, BlockHeader> for IndexedLmdbBlockStore {
    type IndexedBlockStoreReader<'a> = IndexedLmdbBlockStoreReadTransaction<'a>;

    fn checkout_ro_indexed(&self) -> Result<Self::IndexedBlockStoreReader<'_>, BlockStoreError> {
        self.get_reader()
    }
}

impl IndexedBlockStoreProvider<BlockHeight, Block> for IndexedLmdbBlockStore {
    type IndexedBlockStoreReader<'a> = IndexedLmdbBlockStoreReadTransaction<'a>;

    fn checkout_ro_indexed(&self) -> Result<Self::IndexedBlockStoreReader<'_>, BlockStoreError> {
        self.get_reader()
    }
}

pub struct BlockHeightIterator<'a> {
    iterator: btree_map::Keys<'a, BlockHeight, BlockHash>,
}

impl<'a> Iterator for BlockHeightIterator<'a> {
    type Item = BlockHeight;

    fn next(&mut self) -> Option<Self::Item> {
        self.iterator.next().copied()
    }
}

impl<'a> DoubleEndedIterator for BlockHeightIterator<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.iterator.next_back().copied()
    }
}

impl<'a> IndexedBy<BlockHeight, BlockHash> for IndexedLmdbBlockStoreReadTransaction<'a> {
    type KeysIterator = BlockHeightIterator<'a>;

    fn get(&mut self, key: &BlockHeight) -> Result<Option<BlockHash>, BlockStoreError> {
        Ok(self.block_store.block_height_index.get(key).copied())
    }

    fn keys(&self) -> Self::KeysIterator {
        BlockHeightIterator {
            iterator: self.block_store.block_height_index.keys(),
        }
    }
}

impl<'a> IndexedBy<BlockHeight, BlockHeader> for IndexedLmdbBlockStoreReadTransaction<'a> {
    type KeysIterator = BlockHeightIterator<'a>;

    fn get(&mut self, key: &BlockHeight) -> Result<Option<BlockHeader>, BlockStoreError> {
        self.block_store
            .block_height_index
            .get(key)
            .and_then(|block_hash| {
                self.block_store
                    .block_store
                    .get_single_block_header(&mut self.txn, block_hash)
                    .transpose()
            })
            .transpose()
    }

    fn keys(&self) -> Self::KeysIterator {
        BlockHeightIterator {
            iterator: self.block_store.block_height_index.keys(),
        }
    }
}

impl<'a> IndexedBy<BlockHeight, Block> for IndexedLmdbBlockStoreReadTransaction<'a> {
    type KeysIterator = BlockHeightIterator<'a>;

    fn get(&mut self, key: &BlockHeight) -> Result<Option<Block>, BlockStoreError> {
        self.block_store
            .block_height_index
            .get(key)
            .and_then(|block_hash| {
                self.block_store
                    .block_store
                    .get_single_block(&mut self.txn, block_hash)
                    .transpose()
            })
            .transpose()
    }

    fn keys(&self) -> Self::KeysIterator {
        BlockHeightIterator {
            iterator: self.block_store.block_height_index.keys(),
        }
    }
}

pub struct EraIdIterator<'a> {
    iterator: btree_map::Keys<'a, EraId, BlockHash>,
}

impl<'a> Iterator for EraIdIterator<'a> {
    type Item = EraId;

    fn next(&mut self) -> Option<Self::Item> {
        self.iterator.next().copied()
    }
}

impl<'a> IndexedBy<EraId, SwitchBlockHeader> for IndexedLmdbBlockStoreReadTransaction<'a> {
    type KeysIterator = EraIdIterator<'a>;

    fn get(&mut self, key: &EraId) -> Result<Option<SwitchBlockHeader>, BlockStoreError> {
        self.block_store
            .get_switch_block_header_by_era_id(&mut self.txn, *key)
    }

    fn keys(&self) -> Self::KeysIterator {
        EraIdIterator {
            iterator: self.block_store.switch_block_era_id_index.keys(),
        }
    }
}

impl<'a> IndexedBy<EraId, SwitchBlock> for IndexedLmdbBlockStoreReadTransaction<'a> {
    type KeysIterator = EraIdIterator<'a>;

    fn get(&mut self, key: &EraId) -> Result<Option<SwitchBlock>, BlockStoreError> {
        self.block_store
            .get_switch_block_by_era_id(&mut self.txn, *key)
    }

    fn keys(&self) -> Self::KeysIterator {
        EraIdIterator {
            iterator: self.block_store.switch_block_era_id_index.keys(),
        }
    }
}
pub struct TransactionHashIterator<'a> {
    iterator: btree_map::Keys<'a, TransactionHash, BlockHashHeightAndEra>,
}

impl<'a> Iterator for TransactionHashIterator<'a> {
    type Item = TransactionHash;

    fn next(&mut self) -> Option<Self::Item> {
        self.iterator.next().copied()
    }
}

impl<'a> IndexedBy<TransactionHash, EraId> for IndexedLmdbBlockStoreReadTransaction<'a> {
    type KeysIterator = TransactionHashIterator<'a>;

    fn get(&mut self, key: &TransactionHash) -> Result<Option<EraId>, BlockStoreError> {
        let res = self
            .block_store
            .transaction_hash_index
            .get(key)
            .map(|block_hash_height_and_era| block_hash_height_and_era.era_id);
        Ok(res)
    }

    fn keys(&self) -> Self::KeysIterator {
        TransactionHashIterator {
            iterator: self.block_store.transaction_hash_index.keys(),
        }
    }
}

impl<'a> IndexedBy<TransactionHash, BlockHashHeightAndEra>
    for IndexedLmdbBlockStoreReadTransaction<'a>
{
    type KeysIterator = TransactionHashIterator<'a>;

    fn get(
        &mut self,
        key: &TransactionHash,
    ) -> Result<Option<BlockHashHeightAndEra>, BlockStoreError> {
        Ok(self.block_store.transaction_hash_index.get(key).copied())
    }

    fn keys(&self) -> Self::KeysIterator {
        TransactionHashIterator {
            iterator: self.block_store.transaction_hash_index.keys(),
        }
    }
}
