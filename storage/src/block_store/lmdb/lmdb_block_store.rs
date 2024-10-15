use std::{
    borrow::Cow,
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
};

use datasize::DataSize;
use tracing::{debug, error};

use casper_types::{
    execution::{execution_result_v1, ExecutionResult, ExecutionResultV1, ExecutionResultV2},
    Approval, Block, BlockBody, BlockHash, BlockHeader, BlockSignatures, Digest, Transaction,
    TransactionHash, Transfer,
};

use super::{
    lmdb_ext::{LmdbExtError, TransactionExt},
    versioned_databases::VersionedDatabases,
};
use crate::block_store::{
    error::BlockStoreError,
    types::{
        ApprovalsHashes, BlockExecutionResults, BlockHashHeightAndEra, BlockTransfers, StateStore,
        TransactionFinalizedApprovals, Transfers,
    },
    BlockStoreProvider, BlockStoreTransaction, DataReader, DataWriter,
};
use lmdb::{
    Database, DatabaseFlags, Environment, EnvironmentFlags, RoTransaction, RwTransaction,
    Transaction as LmdbTransaction, WriteFlags,
};

/// Filename for the LMDB database created by the Storage component.
const STORAGE_DB_FILENAME: &str = "storage.lmdb";

/// We can set this very low, as there is only a single reader/writer accessing the component at any
/// one time.
const MAX_TRANSACTIONS: u32 = 5;

/// Maximum number of allowed dbs.
const MAX_DB_COUNT: u32 = 17;

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
        })
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
        ExecutionResult::V2(ExecutionResultV2 {
            transfers,
            error_message,
            ..
        }) => {
            if error_message.is_none() {
                for transfer in transfers {
                    all_transfers.push(transfer.clone());
                }
            }
            // else no-op: we only record transfers from successful executions.
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

impl<'t, T> BlockStoreTransaction for LmdbBlockStoreTransaction<'t, T>
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

impl<'t, T> DataReader<BlockHash, Block> for LmdbBlockStoreTransaction<'t, T>
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

impl<'t, T> DataReader<BlockHash, BlockHeader> for LmdbBlockStoreTransaction<'t, T>
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

impl<'t, T> DataReader<BlockHash, ApprovalsHashes> for LmdbBlockStoreTransaction<'t, T>
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

impl<'t, T> DataReader<BlockHash, BlockSignatures> for LmdbBlockStoreTransaction<'t, T>
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

impl<'t, T> DataReader<TransactionHash, Transaction> for LmdbBlockStoreTransaction<'t, T>
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

impl<'t, T> DataReader<TransactionHash, BTreeSet<Approval>> for LmdbBlockStoreTransaction<'t, T>
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

impl<'t, T> DataReader<TransactionHash, ExecutionResult> for LmdbBlockStoreTransaction<'t, T>
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

impl<'t, T> DataReader<BlockHash, Vec<Transfer>> for LmdbBlockStoreTransaction<'t, T>
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

impl<'t, T, K> DataReader<K, Vec<u8>> for LmdbBlockStoreTransaction<'t, T>
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

impl<'t> DataWriter<BlockHash, Block> for LmdbBlockStoreTransaction<'t, RwTransaction<'t>> {
    /// Writes a block to storage.
    fn write(&mut self, data: &Block) -> Result<BlockHash, BlockStoreError> {
        self.block_store.write_block(&mut self.txn, data)
    }

    fn delete(&mut self, key: BlockHash) -> Result<(), BlockStoreError> {
        let maybe_block = self.block_store.get_single_block_header(&self.txn, &key)?;

        if let Some(block_header) = maybe_block {
            self.block_store.delete_block_header(&mut self.txn, &key)?;
            self.block_store
                .delete_block_body(&mut self.txn, block_header.body_hash())?;
        }
        Ok(())
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
        self.block_store.write_block_header(&mut self.txn, data)
    }

    fn delete(&mut self, key: BlockHash) -> Result<(), BlockStoreError> {
        self.block_store.delete_block_header(&mut self.txn, &key)
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
        let block_hash = data.block_info.block_hash;

        let _ = self.block_store.write_execution_results(
            &mut self.txn,
            &block_hash,
            data.exec_results.clone(),
        )?;

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
