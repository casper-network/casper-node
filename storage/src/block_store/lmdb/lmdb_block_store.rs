use std::{
    borrow::Cow,
    collections::{btree_map, BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
};

use datasize::DataSize;
use tracing::{debug, error, info};

use casper_types::{
    execution::{execution_result_v1, ExecutionResult, ExecutionResultV1},
    Approval, Block, BlockBody, BlockHash, BlockHeader, BlockSignatures, Digest, EraId,
    Transaction, TransactionHash, Transfer,
};

use super::{
    lmdb_ext::{
        append_by_be_u64_key, append_value_bytesrepr, delete_by_be_u64_key, delete_value_bytesrepr,
        get_by_be_u64_key, get_last_by_be_u64_key, put_by_be_u64_key, LmdbExtError, TransactionExt,
        WriteTransactionExt,
    },
    versioned_databases::VersionedDatabases,
    DbTableId,
};
use crate::block_store::{
    error::BlockStoreError,
    types::{
        ApprovalsHashes, BlockExecutionResults, BlockHashHeightAndEra, BlockHeight, BlockTransfers,
        LatestSwitchBlock, StateStore, StateStoreKey, Tip, TransactionFinalizedApprovals,
        Transfers,
    },
    BlockStoreProvider, BlockStoreTransaction, DataReader, DataWriter, DbRawBytesSpec,
};
use lmdb::{
    Database, DatabaseFlags, Environment, EnvironmentFlags, RoTransaction, RwCursor, RwTransaction,
    Transaction as LmdbTransaction, WriteFlags,
};

/// Filename for the LMDB database created by the Storage component.
const STORAGE_DB_FILENAME: &str = "storage.lmdb";

/// We can set this very low, as there is only a single reader/writer accessing the component at any
/// one time.
const MAX_TRANSACTIONS: u32 = 5;

/// Maximum number of allowed dbs.
const MAX_DB_COUNT: u32 = 20;

/// OS-specific lmdb flags.
#[cfg(not(target_os = "macos"))]
const OS_FLAGS: EnvironmentFlags = EnvironmentFlags::WRITE_MAP;

/// OS-specific lmdb flags.
///
/// Mac OS X exhibits performance regressions when `WRITE_MAP` is used.
#[cfg(target_os = "macos")]
const OS_FLAGS: EnvironmentFlags = EnvironmentFlags::empty();

/// Lmdb block store.
#[derive(DataSize, Debug)]
pub struct LmdbBlockStore {
    /// Storage location.
    root: PathBuf,
    /// Environment holding LMDB databases.
    #[data_size(skip)]
    pub(super) env: Arc<Environment>,
    /// The block header databases.
    pub(super) block_header_dbs: VersionedDatabases<BlockHash, BlockHeader>,
    /// The block body databases.
    pub(super) block_body_dbs: VersionedDatabases<Digest, BlockBody>,
    /// The approvals hashes databases.
    pub(super) approvals_hashes_dbs: VersionedDatabases<BlockHash, ApprovalsHashes>,
    /// The block metadata db.
    pub(super) block_metadata_dbs: VersionedDatabases<BlockHash, BlockSignatures>,
    /// The transaction databases.
    pub(super) transaction_dbs: VersionedDatabases<TransactionHash, Transaction>,
    /// Databases of `ExecutionResult`s indexed by transaction hash for current DB or by deploy
    /// hash for legacy DB.
    pub(super) execution_result_dbs: VersionedDatabases<TransactionHash, ExecutionResult>,
    /// The transfer databases.
    pub(super) transfer_dbs: VersionedDatabases<BlockHash, Transfers>,
    /// The state storage database.
    #[data_size(skip)]
    state_store_db: Database,
    /// The finalized transaction approvals databases.
    pub(super) finalized_transaction_approvals_dbs:
        VersionedDatabases<TransactionHash, BTreeSet<Approval>>,
    /// Disk-backed index of block height to block hash.
    #[data_size(skip)]
    pub(super) block_height_index_db: Database,
    /// Disk-backed index of era ID to switch block hash.
    #[data_size(skip)]
    pub(super) switch_block_era_id_index_db: Database,
    /// Disk-backed index of transaction hash to the hash, height and era of the block containing
    /// it.
    #[data_size(skip)]
    pub(super) transaction_hash_index_db: Database,
}

impl LmdbBlockStore {
    /// Ctor.
    pub fn new(root_path: &Path, total_size: usize) -> Result<Self, BlockStoreError> {
        // Create the environment and databases.
        let env = new_environment(total_size, root_path)?;

        let block_header_dbs = VersionedDatabases::new(&env, "block_header", "block_header_v2")
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        let block_body_dbs =
            VersionedDatabases::<_, BlockBody>::new(&env, "block_body", "block_body_v2")
                .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        let block_metadata_dbs =
            VersionedDatabases::new(&env, "block_metadata", "block_metadata_v2")
                .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        let transaction_dbs = VersionedDatabases::new(&env, "deploys", "transactions")
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        let execution_result_dbs =
            VersionedDatabases::new(&env, "deploy_metadata", "execution_results")
                .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        let transfer_dbs = VersionedDatabases::new(&env, "transfer", "versioned_transfers")
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        let state_store_db = env
            .create_db(Some("state_store"), DatabaseFlags::empty())
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;

        let finalized_transaction_approvals_dbs =
            VersionedDatabases::new(&env, "finalized_approvals", "versioned_finalized_approvals")
                .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        let approvals_hashes_dbs =
            VersionedDatabases::new(&env, "approvals_hashes", "versioned_approvals_hashes")
                .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;

        let block_height_index_db = env
            .create_db(Some("block_height_index"), DatabaseFlags::empty())
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        let switch_block_era_id_index_db = env
            .create_db(Some("switch_block_era_id_index"), DatabaseFlags::empty())
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        let transaction_hash_index_db = env
            .create_db(Some("transaction_hash_index"), DatabaseFlags::empty())
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;

        Ok(Self {
            root: root_path.to_path_buf(),
            env: Arc::new(env),
            block_header_dbs,
            block_body_dbs,
            approvals_hashes_dbs,
            block_metadata_dbs,
            transaction_dbs,
            execution_result_dbs,
            transfer_dbs,
            state_store_db,
            finalized_transaction_approvals_dbs,
            block_height_index_db,
            switch_block_era_id_index_db,
            transaction_hash_index_db,
        })
    }

    /// Initializes the disk-backed indexes. This operation can be time
    /// consuming because it needs to go through all entries in block
    /// headers db. If the index has data it assumes that no reindexing
    /// is needed.
    pub fn init(&mut self) -> Result<(), BlockStoreError> {
        let ro_txn = self
            .env
            .begin_ro_txn()
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        let index_is_empty = ro_txn
            .stat(self.block_height_index_db)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?
            .entries()
            == 0;
        let headers_exist = header_count(&ro_txn, self)? > 0;
        ro_txn
            .commit()
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;

        if headers_exist && index_is_empty {
            info!("block store indexes appear to be missing; building them from a full scan");
            self.rebuild_indexes()?;
        }

        Ok(())
    }

    /// Performs an unconditional one-off full rebuild of the disk-backed block-height/
    /// switch-block-era-id/transaction-hash indexes, by scanning every block header currently in
    /// storage. Exposed for tests; startup code should use [`Self::init`], which only rebuilds
    /// when necessary.
    #[cfg(test)]
    pub fn reindex(&mut self) -> Result<(), BlockStoreError> {
        self.rebuild_indexes()
    }

    fn rebuild_indexes(&mut self) -> Result<(), BlockStoreError> {
        info!("reindexing block store");

        let mut block_height_index = BTreeMap::new();
        let mut switch_block_era_id_index = BTreeMap::new();
        let mut transaction_hash_index = BTreeMap::new();

        let mut block_txn = self
            .env
            .begin_rw_txn()
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;

        let total_headers = header_count(&block_txn, self)?;
        let progress_step = (total_headers / 20).max(1);
        let mut processed: usize = 0;

        // First pass: scan every header through the cursor to build the height/switch-block
        // indexes and collect the headers for the second pass below.
        let mut headers = Vec::new();
        let mut collect_fn =
            |_cursor: &mut RwCursor, block_header: BlockHeader| -> Result<(), BlockStoreError> {
                processed += 1;
                if processed.is_multiple_of(progress_step) {
                    info!(
                        percent_complete = (processed * 100 / total_headers.max(1)),
                        processed, total_headers, "reindexing block store: scanning headers"
                    );
                }

                Self::insert_to_block_header_indices(
                    &mut block_height_index,
                    &mut switch_block_era_id_index,
                    &block_header,
                )?;
                headers.push(block_header);

                Ok(())
            };

        self.block_header_dbs
            .for_each_value_in_current(&mut block_txn, &mut collect_fn)?;
        self.block_header_dbs
            .for_each_value_in_legacy(&mut block_txn, &mut collect_fn)?;

        // Second pass: the cursor's borrow of `block_txn` has ended, so each block body can be
        // read through the transaction we already have open, instead of a fresh one per header.
        let total_bodies = headers.len();
        let body_progress_step = (total_bodies / 20).max(1);
        for (processed, block_header) in headers.iter().enumerate() {
            if processed.is_multiple_of(body_progress_step) {
                info!(
                    percent_complete = (processed * 100 / total_bodies.max(1)),
                    processed, total_bodies, "reindexing block store: scanning bodies"
                );
            }

            let maybe_block_body = self
                .block_body_dbs
                .get(&block_txn, block_header.body_hash())
                .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
            if let Some(block_body) = &maybe_block_body {
                let transaction_hashes = block_transaction_hashes(block_body);
                Self::insert_to_transaction_index(
                    &mut transaction_hash_index,
                    block_header.block_hash(),
                    block_header.height(),
                    block_header.era_id(),
                    transaction_hashes,
                )?;
            }
        }

        // The scan above makes no changes to the header dbs (unlike `prune`), so this can just be
        // rolled back rather than committed.
        block_txn.abort();

        let mut index_txn = self
            .env
            .begin_rw_txn()
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;

        index_txn
            .clear_db(self.block_height_index_db)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        index_txn
            .clear_db(self.switch_block_era_id_index_db)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        index_txn
            .clear_db(self.transaction_hash_index_db)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;

        // `block_height_index`/`switch_block_era_id_index`/`transaction_hash_index` are
        // `BTreeMap`s, so iterating them yields ascending key order; combined with the `clear_db`
        // calls above, this lets us use LMDB's `APPEND` flag to skip the usual B-tree
        // search/rebalance per insert (a significant speedup for a full rebuild). This is safe
        // because: the two `u64`/`EraId`-keyed indexes use `append_by_be_u64_key`, whose
        // big-endian key encoding is specifically chosen so ascending numeric order is ascending
        // byte order; and `TransactionHash`'s derived `Ord` (variant tag, then digest bytes)
        // matches its `bytesrepr` encoding (tag byte, then raw digest bytes) byte-for-byte.
        for (height, block_hash) in block_height_index {
            append_by_be_u64_key(
                &mut index_txn,
                self.block_height_index_db,
                height,
                &block_hash,
            )
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        }
        for (era_id, block_hash) in switch_block_era_id_index {
            append_by_be_u64_key(
                &mut index_txn,
                self.switch_block_era_id_index_db,
                era_id.value(),
                &block_hash,
            )
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        }
        for (transaction_hash, block_info) in transaction_hash_index {
            append_value_bytesrepr(
                &mut index_txn,
                self.transaction_hash_index_db,
                &transaction_hash,
                &block_info,
            )
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        }

        index_txn
            .commit()
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;

        info!("block store reindexing complete");
        Ok(())
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
    fn insert_to_block_header_indices(
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

    /// Write finality signatures.
    pub fn write_finality_signatures(
        &self,
        txn: &mut RwTransaction,
        signatures: &BlockSignatures,
    ) -> Result<BlockHash, BlockStoreError> {
        let block_hash = signatures.block_hash();
        let _ = self
            .block_metadata_dbs
            .put(txn, block_hash, signatures, true)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;

        Ok(*block_hash)
    }

    pub(crate) fn delete_finality_signatures(
        &self,
        txn: &mut RwTransaction,
        block_hash: &BlockHash,
    ) -> Result<(), BlockStoreError> {
        self.block_metadata_dbs
            .delete(txn, block_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    pub(crate) fn transaction_exists<Tx: lmdb::Transaction>(
        &self,
        txn: &Tx,
        transaction_hash: &TransactionHash,
    ) -> Result<bool, BlockStoreError> {
        self.transaction_dbs
            .exists(txn, transaction_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    /// Returns `true` if the given block's header and body are stored.
    pub(crate) fn block_exists<Tx: lmdb::Transaction>(
        &self,
        txn: &Tx,
        block_hash: &BlockHash,
    ) -> Result<bool, BlockStoreError> {
        let block_header = match self.get_single_block_header(txn, block_hash)? {
            Some(block_header) => block_header,
            None => {
                return Ok(false);
            }
        };
        self.block_body_dbs
            .exists(txn, block_header.body_hash())
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    /// Returns `true` if the given block's header is stored.
    pub(crate) fn block_header_exists<Tx: lmdb::Transaction>(
        &self,
        txn: &Tx,
        block_hash: &BlockHash,
    ) -> Result<bool, BlockStoreError> {
        self.block_header_dbs
            .exists(txn, block_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    pub(crate) fn get_transfers<Tx: lmdb::Transaction>(
        &self,
        txn: &Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<Vec<Transfer>>, BlockStoreError> {
        Ok(self
            .transfer_dbs
            .get(txn, block_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?
            .map(Transfers::into_owned))
    }

    pub(crate) fn has_transfers<Tx: lmdb::Transaction>(
        &self,
        txn: &Tx,
        block_hash: &BlockHash,
    ) -> Result<bool, BlockStoreError> {
        self.transfer_dbs
            .exists(txn, block_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    pub(crate) fn read_state_store<K: AsRef<[u8]>, Tx: lmdb::Transaction>(
        &self,
        txn: &Tx,
        key: &K,
    ) -> Result<Option<Vec<u8>>, BlockStoreError> {
        let bytes = match txn.get(self.state_store_db, &key) {
            Ok(slice) => Some(slice.to_owned()),
            Err(lmdb::Error::NotFound) => None,
            Err(err) => return Err(BlockStoreError::InternalStorage(Box::new(err))),
        };
        Ok(bytes)
    }

    /// Retrieves approvals hashes by block hash.
    pub(crate) fn read_approvals_hashes<Tx: lmdb::Transaction>(
        &self,
        txn: &Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<ApprovalsHashes>, BlockStoreError> {
        self.approvals_hashes_dbs
            .get(txn, block_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    pub(crate) fn approvals_hashes_exist<Tx: lmdb::Transaction>(
        &self,
        txn: &Tx,
        block_hash: &BlockHash,
    ) -> Result<bool, BlockStoreError> {
        self.approvals_hashes_dbs
            .exists(txn, block_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    /// Put a single transaction into storage.
    pub(crate) fn write_transaction(
        &self,
        txn: &mut RwTransaction,
        transaction: &Transaction,
    ) -> Result<TransactionHash, BlockStoreError> {
        let transaction_hash = transaction.hash();
        self.transaction_dbs
            .put(txn, &transaction_hash, transaction, false)
            .map(|_| transaction_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    pub(crate) fn delete_transaction(
        &self,
        txn: &mut RwTransaction,
        transaction_hash: &TransactionHash,
    ) -> Result<(), BlockStoreError> {
        self.transaction_dbs
            .delete(txn, transaction_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    pub(crate) fn write_transfers(
        &self,
        txn: &mut RwTransaction,
        block_hash: &BlockHash,
        transfers: &[Transfer],
    ) -> Result<bool, BlockStoreError> {
        self.transfer_dbs
            .put(
                txn,
                block_hash,
                &Transfers::from(transfers.to_owned()),
                true,
            )
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    pub(crate) fn delete_transfers(
        &self,
        txn: &mut RwTransaction,
        block_hash: &BlockHash,
    ) -> Result<(), BlockStoreError> {
        self.transfer_dbs
            .delete(txn, block_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    /// Writes a key to the state storage database.
    // See note below why `key` and `data` are not `&[u8]`s.
    pub(crate) fn write_state_store(
        &self,
        txn: &mut RwTransaction,
        key: Cow<'static, [u8]>,
        data: &Vec<u8>,
    ) -> Result<(), BlockStoreError> {
        // Note: The interface of `lmdb` seems suboptimal: `&K` and `&V` could simply be `&[u8]` for
        //       simplicity. At the very least it seems to be missing a `?Sized` trait bound. For
        //       this reason, we need to use actual sized types in the function signature above.
        txn.put(self.state_store_db, &key, data, WriteFlags::default())
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;

        Ok(())
    }

    pub(crate) fn state_store_key_exists<K: AsRef<[u8]>, Tx: lmdb::Transaction>(
        &self,
        txn: &Tx,
        key: &K,
    ) -> Result<bool, BlockStoreError> {
        txn.value_exists(self.state_store_db, &key)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    pub(crate) fn delete_state_store(
        &self,
        txn: &mut RwTransaction,
        key: Cow<'static, [u8]>,
    ) -> Result<(), BlockStoreError> {
        txn.del(self.state_store_db, &key, None)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    /// Retrieves a single block header in a given transaction from storage.
    pub(crate) fn get_single_block_header<Tx: LmdbTransaction>(
        &self,
        txn: &Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockHeader>, BlockStoreError> {
        let block_header = match self
            .block_header_dbs
            .get(txn, block_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?
        {
            Some(block_header) => block_header,
            None => return Ok(None),
        };
        block_header.set_block_hash(*block_hash);
        Ok(Some(block_header))
    }

    /// Retrieves block signatures for a block with a given block hash.
    pub(crate) fn get_block_signatures<Tx: LmdbTransaction>(
        &self,
        txn: &Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockSignatures>, BlockStoreError> {
        self.block_metadata_dbs
            .get(txn, block_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    pub(crate) fn block_signatures_exist<Tx: LmdbTransaction>(
        &self,
        txn: &Tx,
        block_hash: &BlockHash,
    ) -> Result<bool, BlockStoreError> {
        self.block_metadata_dbs
            .exists(txn, block_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    /// Retrieves a single block from storage.
    pub(crate) fn get_single_block<Tx: LmdbTransaction>(
        &self,
        txn: &Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<Block>, BlockStoreError> {
        let block_header: BlockHeader = match self.get_single_block_header(txn, block_hash)? {
            Some(block_header) => block_header,
            None => {
                debug!(
                    ?block_hash,
                    "get_single_block: missing block header for {}", block_hash
                );
                return Ok(None);
            }
        };

        let maybe_block_body = self
            .block_body_dbs
            .get(txn, block_header.body_hash())
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)));
        let block_body = match maybe_block_body? {
            Some(block_body) => block_body,
            None => {
                debug!(
                    ?block_header,
                    "get_single_block: missing block body for {}",
                    block_header.block_hash()
                );
                return Ok(None);
            }
        };
        let block = Block::new_from_header_and_body(block_header, block_body)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        Ok(Some(block))
    }

    /// Writes a block to storage.
    ///
    /// Returns `Ok(true)` if the block has been successfully written, `Ok(false)` if a part of it
    /// couldn't be written because it already existed, and `Err(_)` if there was an error.
    pub(crate) fn write_block(
        &self,
        txn: &mut RwTransaction,
        block: &Block,
    ) -> Result<BlockHash, BlockStoreError> {
        let block_hash = *block.hash();
        let _ = self
            .block_body_dbs
            .put(txn, block.body_hash(), &block.clone_body(), true)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;

        let block_header = block.clone_header();
        let _ = self
            .block_header_dbs
            .put(txn, block.hash(), &block_header, true)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;

        Ok(block_hash)
    }

    pub(crate) fn write_block_header(
        &self,
        txn: &mut RwTransaction,
        block_header: &BlockHeader,
    ) -> Result<BlockHash, BlockStoreError> {
        let block_hash = block_header.block_hash();
        self.block_header_dbs
            .put(txn, &block_hash, block_header, true)
            .map(|_| block_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    pub(crate) fn delete_block_header(
        &self,
        txn: &mut RwTransaction,
        block_hash: &BlockHash,
    ) -> Result<(), BlockStoreError> {
        self.block_header_dbs
            .delete(txn, block_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    pub(crate) fn delete_block_body(
        &self,
        txn: &mut RwTransaction,
        block_body_hash: &Digest,
    ) -> Result<(), BlockStoreError> {
        self.block_body_dbs
            .delete(txn, block_body_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    /// Writes approvals hashes to storage.
    pub(crate) fn write_approvals_hashes(
        &self,
        txn: &mut RwTransaction,
        approvals_hashes: &ApprovalsHashes,
    ) -> Result<BlockHash, BlockStoreError> {
        let block_hash = approvals_hashes.block_hash();
        let _ = self
            .approvals_hashes_dbs
            .put(txn, block_hash, approvals_hashes, true)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        Ok(*block_hash)
    }

    pub(crate) fn delete_approvals_hashes(
        &self,
        txn: &mut RwTransaction,
        block_hash: &BlockHash,
    ) -> Result<(), BlockStoreError> {
        self.approvals_hashes_dbs
            .delete(txn, block_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    pub(crate) fn write_execution_results(
        &self,
        txn: &mut RwTransaction,
        block_hash: &BlockHash,
        execution_results: HashMap<TransactionHash, ExecutionResult>,
    ) -> Result<bool, BlockStoreError> {
        let mut transfers: Vec<Transfer> = vec![];
        for (transaction_hash, execution_result) in execution_results.into_iter() {
            transfers.extend(successful_transfers(&execution_result));

            let maybe_stored_execution_result: Option<ExecutionResult> = self
                .checkout_ro()
                .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?
                .read(transaction_hash)?;

            // If we have a previous execution result, we can continue if it is the same.
            match maybe_stored_execution_result {
                Some(stored_execution_result) if stored_execution_result == execution_result => {
                    continue
                }
                Some(_) | None => (),
            }

            let was_written = self
                .execution_result_dbs
                .put(txn, &transaction_hash, &execution_result, true)
                .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;

            if !was_written {
                error!(
                    ?block_hash,
                    ?transaction_hash,
                    "failed to write execution results"
                );
                debug_assert!(was_written);
            }
        }

        let was_written = self
            .transfer_dbs
            .put(txn, block_hash, &Transfers::from(transfers), true)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        if !was_written {
            error!(?block_hash, "failed to write transfers");
            debug_assert!(was_written);
        }
        Ok(was_written)
    }

    pub(crate) fn delete_execution_results(
        &self,
        txn: &mut RwTransaction,
        block_hash: &BlockHash,
    ) -> Result<bool, BlockStoreError> {
        let block = self.get_single_block(txn, block_hash)?;

        if let Some(block) = block {
            for txn_hash in block.all_transaction_hashes() {
                self.execution_result_dbs
                    .delete(txn, &txn_hash)
                    .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
            }
        }
        Ok(true)
    }
}

fn header_count<Tx: LmdbTransaction>(
    txn: &Tx,
    block_store: &LmdbBlockStore,
) -> Result<usize, BlockStoreError> {
    let current = txn
        .stat(block_store.block_header_dbs.current)
        .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?
        .entries();
    let legacy = txn
        .stat(block_store.block_header_dbs.legacy)
        .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?
        .entries();
    Ok(current + legacy)
}

/// Returns the transaction hashes referenced by a block body.
fn block_transaction_hashes(block_body: &BlockBody) -> Vec<TransactionHash> {
    match block_body {
        BlockBody::V1(v1) => v1
            .deploy_and_transfer_hashes()
            .map(TransactionHash::from)
            .collect(),
        BlockBody::V2(v2) => v2.all_transactions().copied().collect(),
    }
}

pub(crate) fn new_environment(
    total_size: usize,
    root: &Path,
) -> Result<Environment, BlockStoreError> {
    Environment::new()
        .set_flags(
            OS_FLAGS
                // We manage our own directory.
                | EnvironmentFlags::NO_SUB_DIR
                // Disable thread local storage, strongly suggested for operation with tokio.
                | EnvironmentFlags::NO_TLS
                // Disable read-ahead. Our data is not stored/read in sequence that would benefit from the read-ahead.
                | EnvironmentFlags::NO_READAHEAD,
        )
        .set_max_readers(MAX_TRANSACTIONS)
        .set_max_dbs(MAX_DB_COUNT)
        .set_map_size(total_size)
        .open(&root.join(STORAGE_DB_FILENAME))
        .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
}

/// Returns all `Transform::WriteTransfer`s from the execution effects if this is an
/// `ExecutionResult::Success`, or an empty `Vec` if `ExecutionResult::Failure`.
fn successful_transfers(execution_result: &ExecutionResult) -> Vec<Transfer> {
    let mut all_transfers: Vec<Transfer> = vec![];
    match execution_result {
        ExecutionResult::V1(ExecutionResultV1::Success { effect, .. }) => {
            for transform_entry in &effect.transforms {
                if let execution_result_v1::TransformKindV1::WriteTransfer(transfer_v1) =
                    &transform_entry.transform
                {
                    all_transfers.push(Transfer::V1(transfer_v1.clone()));
                }
            }
        }
        ExecutionResult::V2(execution_result_v2) => {
            if execution_result_v2.error_message.is_none() {
                for transfer in &execution_result_v2.transfers {
                    all_transfers.push(transfer.clone());
                }
            }
            // else no-op: we only record transfers from successful executions.
        }
        ExecutionResult::Evm(_) => {
            // No-op: EVM receipt logs are not Casper transfers.
        }
        ExecutionResult::V1(ExecutionResultV1::Failure { .. }) => {
            // No-op: we only record transfers from successful executions.
        }
    }

    all_transfers
}

impl BlockStoreProvider for LmdbBlockStore {
    type Reader<'t> = LmdbBlockStoreTransaction<'t, RoTransaction<'t>>;
    type ReaderWriter<'t> = LmdbBlockStoreTransaction<'t, RwTransaction<'t>>;

    fn checkout_ro(&self) -> Result<Self::Reader<'_>, BlockStoreError> {
        let txn = self
            .env
            .begin_ro_txn()
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        Ok(LmdbBlockStoreTransaction {
            txn,
            block_store: self,
        })
    }

    fn checkout_rw(&mut self) -> Result<Self::ReaderWriter<'_>, BlockStoreError> {
        let txn = self
            .env
            .begin_rw_txn()
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;

        Ok(LmdbBlockStoreTransaction {
            txn,
            block_store: self,
        })
    }
}

pub struct LmdbBlockStoreTransaction<'t, T>
where
    T: LmdbTransaction,
{
    txn: T,
    block_store: &'t LmdbBlockStore,
}

impl<T> BlockStoreTransaction for LmdbBlockStoreTransaction<'_, T>
where
    T: LmdbTransaction,
{
    fn commit(self) -> Result<(), BlockStoreError> {
        self.txn
            .commit()
            .map_err(|e| BlockStoreError::InternalStorage(Box::new(LmdbExtError::from(e))))
    }

    fn rollback(self) {
        self.txn.abort();
    }
}

impl<T> DataReader<BlockHash, Block> for LmdbBlockStoreTransaction<'_, T>
where
    T: LmdbTransaction,
{
    fn read(&self, key: BlockHash) -> Result<Option<Block>, BlockStoreError> {
        self.block_store.get_single_block(&self.txn, &key)
    }

    fn exists(&self, key: BlockHash) -> Result<bool, BlockStoreError> {
        self.block_store.block_exists(&self.txn, &key)
    }
}

impl<T> DataReader<BlockHash, BlockHeader> for LmdbBlockStoreTransaction<'_, T>
where
    T: LmdbTransaction,
{
    fn read(&self, key: BlockHash) -> Result<Option<BlockHeader>, BlockStoreError> {
        self.block_store.get_single_block_header(&self.txn, &key)
    }

    fn exists(&self, key: BlockHash) -> Result<bool, BlockStoreError> {
        self.block_store.block_header_exists(&self.txn, &key)
    }
}

impl<T> DataReader<BlockHash, ApprovalsHashes> for LmdbBlockStoreTransaction<'_, T>
where
    T: LmdbTransaction,
{
    fn read(&self, key: BlockHash) -> Result<Option<ApprovalsHashes>, BlockStoreError> {
        self.block_store.read_approvals_hashes(&self.txn, &key)
    }

    fn exists(&self, key: BlockHash) -> Result<bool, BlockStoreError> {
        self.block_store.block_header_exists(&self.txn, &key)
    }
}

impl<T> DataReader<BlockHash, BlockSignatures> for LmdbBlockStoreTransaction<'_, T>
where
    T: LmdbTransaction,
{
    fn read(&self, key: BlockHash) -> Result<Option<BlockSignatures>, BlockStoreError> {
        self.block_store.get_block_signatures(&self.txn, &key)
    }

    fn exists(&self, key: BlockHash) -> Result<bool, BlockStoreError> {
        self.block_store.block_signatures_exist(&self.txn, &key)
    }
}

impl<T> DataReader<TransactionHash, Transaction> for LmdbBlockStoreTransaction<'_, T>
where
    T: LmdbTransaction,
{
    fn read(&self, key: TransactionHash) -> Result<Option<Transaction>, BlockStoreError> {
        self.block_store
            .transaction_dbs
            .get(&self.txn, &key)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    fn exists(&self, key: TransactionHash) -> Result<bool, BlockStoreError> {
        self.block_store.transaction_exists(&self.txn, &key)
    }
}

impl<T> DataReader<TransactionHash, BTreeSet<Approval>> for LmdbBlockStoreTransaction<'_, T>
where
    T: LmdbTransaction,
{
    fn read(&self, key: TransactionHash) -> Result<Option<BTreeSet<Approval>>, BlockStoreError> {
        self.block_store
            .finalized_transaction_approvals_dbs
            .get(&self.txn, &key)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    fn exists(&self, key: TransactionHash) -> Result<bool, BlockStoreError> {
        self.block_store
            .finalized_transaction_approvals_dbs
            .exists(&self.txn, &key)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }
}

impl<T> DataReader<TransactionHash, ExecutionResult> for LmdbBlockStoreTransaction<'_, T>
where
    T: LmdbTransaction,
{
    fn read(&self, key: TransactionHash) -> Result<Option<ExecutionResult>, BlockStoreError> {
        self.block_store
            .execution_result_dbs
            .get(&self.txn, &key)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    fn exists(&self, key: TransactionHash) -> Result<bool, BlockStoreError> {
        self.block_store
            .execution_result_dbs
            .exists(&self.txn, &key)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }
}

impl<T> DataReader<BlockHash, Vec<Transfer>> for LmdbBlockStoreTransaction<'_, T>
where
    T: LmdbTransaction,
{
    fn read(&self, key: BlockHash) -> Result<Option<Vec<Transfer>>, BlockStoreError> {
        self.block_store.get_transfers(&self.txn, &key)
    }

    fn exists(&self, key: BlockHash) -> Result<bool, BlockStoreError> {
        self.block_store.has_transfers(&self.txn, &key)
    }
}

impl<T, K> DataReader<K, Vec<u8>> for LmdbBlockStoreTransaction<'_, T>
where
    K: AsRef<[u8]>,
    T: LmdbTransaction,
{
    fn read(&self, key: K) -> Result<Option<Vec<u8>>, BlockStoreError> {
        self.block_store.read_state_store(&self.txn, &key)
    }

    fn exists(&self, key: K) -> Result<bool, BlockStoreError> {
        self.block_store.state_store_key_exists(&self.txn, &key)
    }
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

impl<T> LmdbBlockStoreTransaction<'_, T>
where
    T: LmdbTransaction,
{
    fn block_hash_from_index(
        &self,
        index: LmdbBlockStoreIndex,
    ) -> Result<Option<BlockHash>, BlockStoreError> {
        let result = match index {
            LmdbBlockStoreIndex::BlockHeight(position) => match position {
                IndexPosition::Tip => get_last_by_be_u64_key::<_, BlockHash>(
                    &self.txn,
                    self.block_store.block_height_index_db,
                ),
                IndexPosition::Key(height) => get_by_be_u64_key::<_, BlockHash>(
                    &self.txn,
                    self.block_store.block_height_index_db,
                    height,
                ),
            },
            LmdbBlockStoreIndex::SwitchBlockEraId(position) => match position {
                IndexPosition::Tip => get_last_by_be_u64_key::<_, BlockHash>(
                    &self.txn,
                    self.block_store.switch_block_era_id_index_db,
                ),
                IndexPosition::Key(era_id) => get_by_be_u64_key::<_, BlockHash>(
                    &self.txn,
                    self.block_store.switch_block_era_id_index_db,
                    era_id.value(),
                ),
            },
        };
        result.map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    fn read_block_indexed(
        &self,
        index: LmdbBlockStoreIndex,
    ) -> Result<Option<Block>, BlockStoreError> {
        match self.block_hash_from_index(index)? {
            Some(block_hash) => self.block_store.get_single_block(&self.txn, &block_hash),
            None => Ok(None),
        }
    }

    fn read_block_header_indexed(
        &self,
        index: LmdbBlockStoreIndex,
    ) -> Result<Option<BlockHeader>, BlockStoreError> {
        match self.block_hash_from_index(index)? {
            Some(block_hash) => self
                .block_store
                .get_single_block_header(&self.txn, &block_hash),
            None => Ok(None),
        }
    }

    fn read_block_signatures_indexed(
        &self,
        index: LmdbBlockStoreIndex,
    ) -> Result<Option<BlockSignatures>, BlockStoreError> {
        match self.block_hash_from_index(index)? {
            Some(block_hash) => self
                .block_store
                .get_block_signatures(&self.txn, &block_hash),
            None => Ok(None),
        }
    }

    fn read_approvals_hashes_indexed(
        &self,
        index: LmdbBlockStoreIndex,
    ) -> Result<Option<ApprovalsHashes>, BlockStoreError> {
        match self.block_hash_from_index(index)? {
            Some(block_hash) => self
                .block_store
                .read_approvals_hashes(&self.txn, &block_hash),
            None => Ok(None),
        }
    }

    fn contains_data_indexed(
        &self,
        index: LmdbBlockStoreIndex,
        data_type: DataType,
    ) -> Result<bool, BlockStoreError> {
        match self.block_hash_from_index(index)? {
            Some(block_hash) => match data_type {
                DataType::Block => self.block_store.block_exists(&self.txn, &block_hash),
                DataType::BlockHeader => {
                    self.block_store.block_header_exists(&self.txn, &block_hash)
                }
                DataType::ApprovalsHashes => self
                    .block_store
                    .approvals_hashes_exist(&self.txn, &block_hash),
                DataType::BlockSignatures => self
                    .block_store
                    .block_signatures_exist(&self.txn, &block_hash),
            },
            None => Ok(false),
        }
    }

    /// Returns the height of the switch block for the given era, if known.
    pub fn get_switch_block_height(&self, era_id: EraId) -> Result<Option<u64>, BlockStoreError> {
        let index = LmdbBlockStoreIndex::SwitchBlockEraId(IndexPosition::Key(era_id));
        match self.block_hash_from_index(index)? {
            Some(block_hash) => {
                let maybe_header: Option<BlockHeader> = self.read(block_hash)?;
                Ok(maybe_header.map(|header| header.height()))
            }
            None => Ok(None),
        }
    }
}

impl<T> DataReader<BlockHeight, Block> for LmdbBlockStoreTransaction<'_, T>
where
    T: LmdbTransaction,
{
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

impl<T> DataReader<BlockHeight, BlockHeader> for LmdbBlockStoreTransaction<'_, T>
where
    T: LmdbTransaction,
{
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

impl<T> DataReader<BlockHeight, ApprovalsHashes> for LmdbBlockStoreTransaction<'_, T>
where
    T: LmdbTransaction,
{
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

impl<T> DataReader<BlockHeight, BlockSignatures> for LmdbBlockStoreTransaction<'_, T>
where
    T: LmdbTransaction,
{
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
impl<T> DataReader<EraId, Block> for LmdbBlockStoreTransaction<'_, T>
where
    T: LmdbTransaction,
{
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
impl<T> DataReader<EraId, BlockHeader> for LmdbBlockStoreTransaction<'_, T>
where
    T: LmdbTransaction,
{
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

impl<T> DataReader<EraId, ApprovalsHashes> for LmdbBlockStoreTransaction<'_, T>
where
    T: LmdbTransaction,
{
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

impl<T> DataReader<EraId, BlockSignatures> for LmdbBlockStoreTransaction<'_, T>
where
    T: LmdbTransaction,
{
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

impl<T> DataReader<Tip, BlockHeader> for LmdbBlockStoreTransaction<'_, T>
where
    T: LmdbTransaction,
{
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

impl<T> DataReader<Tip, Block> for LmdbBlockStoreTransaction<'_, T>
where
    T: LmdbTransaction,
{
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

impl<T> DataReader<LatestSwitchBlock, BlockHeader> for LmdbBlockStoreTransaction<'_, T>
where
    T: LmdbTransaction,
{
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

impl<T> DataReader<TransactionHash, BlockHashHeightAndEra> for LmdbBlockStoreTransaction<'_, T>
where
    T: LmdbTransaction,
{
    fn read(&self, key: TransactionHash) -> Result<Option<BlockHashHeightAndEra>, BlockStoreError> {
        self.txn
            .get_value_bytesrepr(self.block_store.transaction_hash_index_db, &key)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    fn exists(&self, key: TransactionHash) -> Result<bool, BlockStoreError> {
        self.txn
            .value_exists_bytesrepr(self.block_store.transaction_hash_index_db, &key)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }
}

impl<T> DataReader<StateStoreKey, Vec<u8>> for LmdbBlockStoreTransaction<'_, T>
where
    T: LmdbTransaction,
{
    fn read(&self, StateStoreKey(key): StateStoreKey) -> Result<Option<Vec<u8>>, BlockStoreError> {
        self.block_store.read_state_store(&self.txn, &key)
    }

    fn exists(&self, StateStoreKey(key): StateStoreKey) -> Result<bool, BlockStoreError> {
        self.block_store.state_store_key_exists(&self.txn, &key)
    }
}

impl<T> DataReader<(DbTableId, Vec<u8>), DbRawBytesSpec> for LmdbBlockStoreTransaction<'_, T>
where
    T: LmdbTransaction,
{
    fn read(
        &self,
        (id, key): (DbTableId, Vec<u8>),
    ) -> Result<Option<DbRawBytesSpec>, BlockStoreError> {
        if key.is_empty() {
            return Ok(None);
        }
        let store = self.block_store;
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

impl<'t> LmdbBlockStoreTransaction<'t, RwTransaction<'t>> {
    /// Check if the block height index can be updated.
    fn should_update_block_height_index(
        &self,
        block_height: u64,
        block_hash: &BlockHash,
    ) -> Result<bool, BlockStoreError> {
        match get_by_be_u64_key::<_, BlockHash>(
            &self.txn,
            self.block_store.block_height_index_db,
            block_height,
        )
        .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?
        {
            // There is a block in the index at this height
            Some(first) if first != *block_hash => Err(BlockStoreError::DuplicateBlock {
                height: block_height,
                first,
                second: *block_hash,
            }),
            // Same value already in index, no need to update it.
            Some(_) => Ok(false),
            // Value not in index, update.
            None => Ok(true),
        }
    }

    /// Check if the switch block index can be updated.
    fn should_update_switch_block_index(
        &self,
        block_header: &BlockHeader,
    ) -> Result<bool, BlockStoreError> {
        if !block_header.is_switch_block() {
            return Ok(false);
        }
        let era_id = block_header.era_id();
        match get_by_be_u64_key::<_, BlockHash>(
            &self.txn,
            self.block_store.switch_block_era_id_index_db,
            era_id.value(),
        )
        .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?
        {
            Some(entry) if entry != block_header.block_hash() => {
                Err(BlockStoreError::DuplicateEraId {
                    era_id,
                    first: entry,
                    second: block_header.block_hash(),
                })
            }
            // already in index, no need to update.
            Some(_) => Ok(false),
            // not in the index, update.
            None => Ok(true),
        }
    }

    // Check if the transaction hash index can be updated.
    fn should_update_transaction_hash_index(
        &self,
        transaction_hashes: &[TransactionHash],
        block_hash: &BlockHash,
    ) -> Result<bool, BlockStoreError> {
        for hash in transaction_hashes {
            if let Some(old_details) = self
                .txn
                .get_value_bytesrepr::<_, BlockHashHeightAndEra>(
                    self.block_store.transaction_hash_index_db,
                    hash,
                )
                .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?
            {
                if old_details.block_hash != *block_hash {
                    return Err(BlockStoreError::DuplicateTransaction {
                        transaction_hash: *hash,
                        first: old_details.block_hash,
                        second: *block_hash,
                    });
                }
            }
        }
        Ok(true)
    }
}

impl<'t> DataWriter<BlockHash, Block> for LmdbBlockStoreTransaction<'t, RwTransaction<'t>> {
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
            put_by_be_u64_key(
                &mut self.txn,
                self.block_store.block_height_index_db,
                block_height,
                block_hash,
            )
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        }

        if update_switch_block_index {
            put_by_be_u64_key(
                &mut self.txn,
                self.block_store.switch_block_era_id_index_db,
                era_id.value(),
                block_hash,
            )
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        }

        if update_transaction_hash_index {
            for hash in transaction_hashes {
                self.txn
                    .put_value_bytesrepr(
                        self.block_store.transaction_hash_index_db,
                        &hash,
                        &BlockHashHeightAndEra::new(*block_hash, block_height, era_id),
                        true,
                    )
                    .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
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

            delete_by_be_u64_key(
                &mut self.txn,
                self.block_store.block_height_index_db,
                block.height(),
            )
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;

            if block.is_switch_block() {
                delete_by_be_u64_key(
                    &mut self.txn,
                    self.block_store.switch_block_era_id_index_db,
                    block.era_id().value(),
                )
                .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
            }

            for hash in transaction_hashes {
                delete_value_bytesrepr(
                    &mut self.txn,
                    self.block_store.transaction_hash_index_db,
                    &hash,
                )
                .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
            }

            self.block_store
                .delete_finality_signatures(&mut self.txn, &key)?;
        }
        Ok(())
    }
}

impl<'t> DataWriter<Digest, BlockBody> for LmdbBlockStoreTransaction<'t, RwTransaction<'t>> {
    /// Not supported: a block body is always written together with its header, as part of
    /// writing a whole `Block` (see the `DataWriter<BlockHash, Block>` impl above).
    fn write(&mut self, _data: &BlockBody) -> Result<Digest, BlockStoreError> {
        Err(BlockStoreError::UnsupportedOperation)
    }

    /// Deletes a block body by its hash. Callers are responsible for only doing so once no
    /// retained block header still references this body hash.
    fn delete(&mut self, key: Digest) -> Result<(), BlockStoreError> {
        self.block_store.delete_block_body(&mut self.txn, &key)
    }
}

impl<'t> DataWriter<BlockHash, ApprovalsHashes>
    for LmdbBlockStoreTransaction<'t, RwTransaction<'t>>
{
    fn write(&mut self, data: &ApprovalsHashes) -> Result<BlockHash, BlockStoreError> {
        self.block_store.write_approvals_hashes(&mut self.txn, data)
    }

    fn delete(&mut self, key: BlockHash) -> Result<(), BlockStoreError> {
        self.block_store
            .delete_approvals_hashes(&mut self.txn, &key)
    }
}

impl<'t> DataWriter<BlockHash, BlockSignatures>
    for LmdbBlockStoreTransaction<'t, RwTransaction<'t>>
{
    fn write(&mut self, data: &BlockSignatures) -> Result<BlockHash, BlockStoreError> {
        self.block_store
            .write_finality_signatures(&mut self.txn, data)
    }

    fn delete(&mut self, key: BlockHash) -> Result<(), BlockStoreError> {
        self.block_store
            .delete_finality_signatures(&mut self.txn, &key)
    }
}

impl<'t> DataWriter<BlockHash, BlockHeader> for LmdbBlockStoreTransaction<'t, RwTransaction<'t>> {
    fn write(&mut self, data: &BlockHeader) -> Result<BlockHash, BlockStoreError> {
        let block_hash = data.block_hash();
        let block_height = data.height();
        let era_id = data.era_id();

        let update_height_index =
            self.should_update_block_height_index(block_height, &block_hash)?;
        let update_switch_block_index = self.should_update_switch_block_index(data)?;

        let key = self.block_store.write_block_header(&mut self.txn, data)?;

        if update_height_index {
            put_by_be_u64_key(
                &mut self.txn,
                self.block_store.block_height_index_db,
                block_height,
                &block_hash,
            )
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        }

        if update_switch_block_index {
            put_by_be_u64_key(
                &mut self.txn,
                self.block_store.switch_block_era_id_index_db,
                era_id.value(),
                &block_hash,
            )
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        }

        Ok(key)
    }

    fn delete(&mut self, key: BlockHash) -> Result<(), BlockStoreError> {
        let maybe_block_header = self.block_store.get_single_block_header(&self.txn, &key)?;

        if let Some(block_header) = maybe_block_header {
            self.block_store.delete_block_header(&mut self.txn, &key)?;

            if block_header.is_switch_block() {
                delete_by_be_u64_key(
                    &mut self.txn,
                    self.block_store.switch_block_era_id_index_db,
                    block_header.era_id().value(),
                )
                .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
            }

            delete_by_be_u64_key(
                &mut self.txn,
                self.block_store.block_height_index_db,
                block_header.height(),
            )
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        }
        Ok(())
    }
}

impl<'t> DataWriter<TransactionHash, Transaction>
    for LmdbBlockStoreTransaction<'t, RwTransaction<'t>>
{
    fn write(&mut self, data: &Transaction) -> Result<TransactionHash, BlockStoreError> {
        self.block_store.write_transaction(&mut self.txn, data)
    }

    fn delete(&mut self, key: TransactionHash) -> Result<(), BlockStoreError> {
        self.block_store.delete_transaction(&mut self.txn, &key)
    }
}

impl<'t> DataWriter<BlockHash, BlockTransfers>
    for LmdbBlockStoreTransaction<'t, RwTransaction<'t>>
{
    fn write(&mut self, data: &BlockTransfers) -> Result<BlockHash, BlockStoreError> {
        self.block_store
            .write_transfers(&mut self.txn, &data.block_hash, &data.transfers)
            .map(|_| data.block_hash)
    }

    fn delete(&mut self, key: BlockHash) -> Result<(), BlockStoreError> {
        self.block_store.delete_transfers(&mut self.txn, &key)
    }
}

impl<'t> DataWriter<Cow<'static, [u8]>, StateStore>
    for LmdbBlockStoreTransaction<'t, RwTransaction<'t>>
{
    fn write(&mut self, data: &StateStore) -> Result<Cow<'static, [u8]>, BlockStoreError> {
        self.block_store
            .write_state_store(&mut self.txn, data.key.clone(), &data.value)?;
        Ok(data.key.clone())
    }

    fn delete(&mut self, key: Cow<'static, [u8]>) -> Result<(), BlockStoreError> {
        self.block_store.delete_state_store(&mut self.txn, key)
    }
}

impl<'t> DataWriter<TransactionHash, TransactionFinalizedApprovals>
    for LmdbBlockStoreTransaction<'t, RwTransaction<'t>>
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

impl<'t> DataWriter<BlockHashHeightAndEra, BlockExecutionResults>
    for LmdbBlockStoreTransaction<'t, RwTransaction<'t>>
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
                self.txn
                    .put_value_bytesrepr(
                        self.block_store.transaction_hash_index_db,
                        &hash,
                        &BlockHashHeightAndEra::new(block_hash, block_height, era_id),
                        true,
                    )
                    .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
            }
        }

        Ok(data.block_info)
    }

    fn delete(&mut self, key: BlockHashHeightAndEra) -> Result<(), BlockStoreError> {
        let block_hash = key.block_hash;

        let _ = self
            .block_store
            .delete_execution_results(&mut self.txn, &block_hash)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use casper_types::{
        testing::TestRng, BlockHeaderV2, EraEndV2, ProtocolVersion, PublicKey, SecretKey, Timestamp,
    };
    use once_cell::sync::OnceCell;
    use rand::Rng;
    use tempfile::TempDir;

    /// Number of headers to write: deliberately > 256 so that, were the disk-backed indexes ever
    /// keyed by plain little-endian `bytesrepr` bytes instead of the big-endian encoding, the
    /// `APPEND`-based bulk write in `reindex` would violate LMDB's required key order (since
    /// little-endian byte-lexicographic order diverges from numeric order once values exceed a
    /// single byte) and fail loudly rather than silently produce a wrong index.
    const HEADER_COUNT: u64 = 300;

    fn header_at_height(rng: &mut TestRng, height: u64, proposer: &PublicKey) -> BlockHeader {
        let is_switch_block = height % 10 == 9;
        let era_id = EraId::new(height / 10);
        let era_end = is_switch_block.then(|| EraEndV2::random(rng));
        BlockHeader::V2(BlockHeaderV2::new(
            BlockHash::random(rng),
            Digest::random(rng),
            Digest::hash(height.to_le_bytes()),
            rng.gen(),
            Digest::random(rng),
            era_end,
            Timestamp::now(),
            era_id,
            height,
            ProtocolVersion::V1_0_0,
            proposer.clone(),
            1,
            None,
            OnceCell::new(),
        ))
    }

    #[test]
    fn reindex_rebuilds_disk_backed_indexes_via_append() {
        let rng = &mut TestRng::new();
        let tempdir = TempDir::new().expect("should create tempdir");
        let mut store =
            LmdbBlockStore::new(tempdir.path(), 64 * 1024 * 1024).expect("should create store");

        let secret_key = SecretKey::random(rng);
        let proposer = PublicKey::from(&secret_key);

        let mut headers = Vec::new();
        {
            let mut rw_txn = store.checkout_rw().expect("should checkout rw");
            for height in 0..HEADER_COUNT {
                let header = header_at_height(rng, height, &proposer);
                let _ = DataWriter::<BlockHash, BlockHeader>::write(&mut rw_txn, &header)
                    .expect("should write header");
                headers.push(header);
            }
            rw_txn.commit().expect("should commit");
        }

        store.reindex().expect("reindex should succeed");

        let ro_txn = store.checkout_ro().expect("should checkout ro");

        // Spot-check a handful of heights, including ones that require crossing the
        // little-endian single-byte boundary (e.g. 255 -> 256) to catch ordering bugs.
        for &height in &[0u64, 1, 254, 255, 256, 257, HEADER_COUNT - 1] {
            let expected_hash = headers[height as usize].block_hash();
            let actual: Option<BlockHeader> = ro_txn.read(height).expect("read by height");
            assert_eq!(
                actual.expect("header should exist").block_hash(),
                expected_hash,
                "wrong header at height {height}"
            );
        }

        // Tip should be the highest height.
        let tip: Option<BlockHeader> = ro_txn.read(Tip).expect("read tip");
        assert_eq!(
            tip.expect("tip should exist").height(),
            HEADER_COUNT - 1,
            "tip should be the highest height"
        );

        // Switch blocks (height % 10 == 9) should be resolvable by era ID.
        for &height in &[9u64, 99, 259, HEADER_COUNT - 1] {
            assert_eq!(
                height % 10,
                9,
                "test bug: {height} is not a switch block height"
            );
            let era_id = EraId::new(height / 10);
            let expected_hash = headers[height as usize].block_hash();
            let actual: Option<BlockHeader> = ro_txn.read(era_id).expect("read by era id");
            assert_eq!(
                actual
                    .expect("switch block header should exist")
                    .block_hash(),
                expected_hash,
                "wrong switch block header for era {era_id}"
            );
        }

        // Latest switch block should be the highest-height header with `is_switch_block()` set
        // (derived from `headers` directly, rather than hardcoded, to avoid off-by-one mistakes).
        let latest_switch_block_height = headers
            .iter()
            .filter(|header| header.is_switch_block())
            .map(|header| header.height())
            .max()
            .expect("should have at least one switch block");
        let latest_switch: Option<BlockHeader> =
            DataReader::<LatestSwitchBlock, BlockHeader>::read(&ro_txn, LatestSwitchBlock)
                .expect("read latest switch block");
        assert_eq!(
            latest_switch
                .expect("latest switch block should exist")
                .height(),
            latest_switch_block_height,
            "wrong latest switch block"
        );
    }

    #[test]
    fn init_builds_index_when_headers_exist_but_index_is_empty() {
        let rng = &mut TestRng::new();
        let tempdir = TempDir::new().expect("should create tempdir");
        let mut store =
            LmdbBlockStore::new(tempdir.path(), 64 * 1024 * 1024).expect("should create store");

        let secret_key = SecretKey::random(rng);
        let proposer = PublicKey::from(&secret_key);

        // Write a header directly via a raw transaction, bypassing the index-maintaining
        // `DataWriter` impl -- simulating a migration from a binary version that didn't yet
        // maintain these disk-backed indexes.
        let header = header_at_height(rng, 0, &proposer);
        {
            let mut txn = store.env.begin_rw_txn().expect("should begin rw txn");
            let _ = store
                .write_block_header(&mut txn, &header)
                .expect("should write header");
            txn.commit().expect("should commit");
        }

        // The index hasn't been told about this header yet.
        {
            let ro_txn = store.checkout_ro().expect("should checkout ro");
            let by_height: Option<BlockHeader> = ro_txn.read(0u64).expect("read by height");
            assert!(by_height.is_none(), "index should not exist yet");
        }

        store.init().expect("init should succeed");

        let ro_txn = store.checkout_ro().expect("should checkout ro");
        let by_height: Option<BlockHeader> = ro_txn.read(0u64).expect("read by height");
        assert_eq!(
            by_height
                .expect("header should be indexed after init")
                .block_hash(),
            header.block_hash(),
            "init should have built the height index from the existing headers"
        );
    }

    #[test]
    fn init_does_not_rebuild_an_already_populated_index() {
        let rng = &mut TestRng::new();
        let tempdir = TempDir::new().expect("should create tempdir");
        let mut store =
            LmdbBlockStore::new(tempdir.path(), 64 * 1024 * 1024).expect("should create store");

        let secret_key = SecretKey::random(rng);
        let proposer = PublicKey::from(&secret_key);

        // Write two headers through the normal, index-maintaining path.
        let headers: Vec<BlockHeader> = (0..2)
            .map(|height| header_at_height(rng, height, &proposer))
            .collect();
        {
            let mut rw_txn = store.checkout_rw().expect("should checkout rw");
            for header in &headers {
                let _ = DataWriter::<BlockHash, BlockHeader>::write(&mut rw_txn, header)
                    .expect("should write header");
            }
            rw_txn.commit().expect("should commit");
        }

        // Directly corrupt the height index by deleting the entry for height 0, without touching
        // the header itself -- an inconsistency that only a full rebuild would fix.
        {
            let mut index_txn = store.env.begin_rw_txn().expect("should begin rw txn");
            delete_by_be_u64_key(&mut index_txn, store.block_height_index_db, 0)
                .expect("should delete index entry");
            index_txn.commit().expect("should commit");
        }

        store.init().expect("init should succeed");

        // Since the index wasn't empty (height 1's entry is still present), `init` must have
        // skipped rebuilding it -- so the deleted entry for height 0 stays missing.
        let ro_txn = store.checkout_ro().expect("should checkout ro");
        let by_height_0: Option<BlockHeader> = ro_txn.read(0u64).expect("read by height");
        assert!(
            by_height_0.is_none(),
            "init should not have rebuilt an already-populated index"
        );
        let by_height_1: Option<BlockHeader> = ro_txn.read(1u64).expect("read by height");
        assert_eq!(
            by_height_1
                .expect("height 1 should still be indexed")
                .block_hash(),
            headers[1].block_hash()
        );
    }
}
