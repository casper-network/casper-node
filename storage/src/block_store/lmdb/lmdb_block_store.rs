use super::lmdb_ext::{LmdbExtError, TransactionExt, WriteTransactionExt};
use crate::block_store::{
    error::BlockStoreError, BlockStoreProvider, BlockStoreReader, BlockStoreTransaction,
    BlockStoreWriter,
};
use datasize::DataSize;
use lmdb::{
    Database, DatabaseFlags, Environment, EnvironmentFlags, RoTransaction, RwTransaction,
    Transaction as LmdbTransaction, WriteFlags,
};
use std::{
    borrow::Cow,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
};

use tracing::{debug, error};

use super::versioned_databases::VersionedDatabases;
use crate::block_store::types::ApprovalsHashes;
use casper_types::{
    execution::{
        execution_result_v1, ExecutionResult, ExecutionResultV1, ExecutionResultV2, TransformKind,
    },
    Block, BlockBody, BlockHash, BlockHeader, BlockSignatures, Digest, EraId, FinalitySignature,
    FinalizedApprovals, StoredValue, Transaction, TransactionHash, TransactionHeader,
    TransactionWithFinalizedApprovals, Transfer,
};

/// Filename for the LMDB database created by the Storage component.
const STORAGE_DB_FILENAME: &str = "storage.lmdb";

/// We can set this very low, as there is only a single reader/writer accessing the component at any
/// one time.
const MAX_TRANSACTIONS: u32 = 5;

/// Maximum number of allowed dbs.
const MAX_DB_COUNT: u32 = 15;

/// OS-specific lmdb flags.
#[cfg(not(target_os = "macos"))]
const OS_FLAGS: EnvironmentFlags = EnvironmentFlags::WRITE_MAP;

/// OS-specific lmdb flags.
///
/// Mac OS X exhibits performance regressions when `WRITE_MAP` is used.
#[cfg(target_os = "macos")]
const OS_FLAGS: EnvironmentFlags = EnvironmentFlags::empty();

#[derive(DataSize, Debug)]
pub struct LmdbBlockStore {
    /// Storage location.
    root: PathBuf,
    /// Environment holding LMDB databases.
    #[data_size(skip)]
    pub(crate) env: Rc<Environment>,
    /// The block header databases.
    pub(crate) block_header_dbs: VersionedDatabases<BlockHash, BlockHeader>,
    /// The block body databases.
    pub(crate) block_body_dbs: VersionedDatabases<Digest, BlockBody>,
    /// The approvals hashes databases.
    approvals_hashes_dbs: VersionedDatabases<BlockHash, ApprovalsHashes>,
    /// The block metadata db.
    #[data_size(skip)]
    pub(crate) block_metadata_db: Database,
    /// The transaction databases.
    pub(crate) transaction_dbs: VersionedDatabases<TransactionHash, Transaction>,
    /// Databases of `ExecutionResult`s indexed by transaction hash for current DB or by deploy
    /// hash for legacy DB.
    pub(crate) execution_result_dbs: VersionedDatabases<TransactionHash, ExecutionResult>,
    /// The transfer database.
    #[data_size(skip)]
    transfer_db: Database,
    /// The state storage database.
    #[data_size(skip)]
    state_store_db: Database,
    /// The finalized transaction approvals databases.
    pub(crate) finalized_transaction_approvals_dbs:
        VersionedDatabases<TransactionHash, FinalizedApprovals>,
}

impl LmdbBlockStore {
    pub fn new(root_path: &Path, total_size: usize) -> Result<Self, BlockStoreError> {
        // Create the environment and databases.
        let env = new_environment(total_size, root_path)?;

        let block_header_dbs = VersionedDatabases::new(&env, "block_header", "block_header_v2")
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        let block_body_dbs =
            VersionedDatabases::<_, BlockBody>::new(&env, "block_body", "block_body_v2")
                .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        let block_metadata_db = env
            .create_db(Some("block_metadata"), DatabaseFlags::empty())
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        let transaction_dbs = VersionedDatabases::new(&env, "deploys", "transactions")
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        let execution_result_dbs =
            VersionedDatabases::new(&env, "deploy_metadata", "execution_results")
                .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        let transfer_db = env
            .create_db(Some("transfer"), DatabaseFlags::empty())
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
            env: Rc::new(env),
            block_header_dbs,
            block_body_dbs,
            approvals_hashes_dbs,
            block_metadata_db,
            transaction_dbs,
            execution_result_dbs,
            transfer_db,
            state_store_db,
            finalized_transaction_approvals_dbs,
        })
    }

    pub fn write_finality_signatures(
        &self,
        txn: &mut RwTransaction,
        signatures: &BlockSignatures,
    ) -> Result<bool, BlockStoreError> {
        let block_hash = signatures.block_hash();
        txn.put_value(self.block_metadata_db, block_hash, signatures, true)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    pub(crate) fn transaction_exists<Tx: lmdb::Transaction>(
        &self,
        txn: &mut Tx,
        transaction_hash: &TransactionHash,
    ) -> Result<bool, BlockStoreError> {
        self.transaction_dbs
            .exists(txn, transaction_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    /// Returns `true` if the given block's header and body are stored.
    pub(crate) fn block_exists<Tx: lmdb::Transaction>(
        &self,
        txn: &mut Tx,
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

    pub(crate) fn get_transfers<Tx: lmdb::Transaction>(
        &self,
        txn: &mut Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<Vec<Transfer>>, BlockStoreError> {
        txn.get_value::<_, Vec<Transfer>>(self.transfer_db, block_hash)
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
        txn: &mut Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<ApprovalsHashes>, BlockStoreError> {
        self.approvals_hashes_dbs
            .get(txn, block_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    /// Put a single transaction into storage.
    pub(crate) fn write_transaction(
        &self,
        txn: &mut RwTransaction,
        transaction: &Transaction,
    ) -> Result<bool, BlockStoreError> {
        let transaction_hash = transaction.hash();
        let outcome = self
            .transaction_dbs
            .put(txn, &transaction_hash, transaction, false)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        if outcome {
            debug!(%transaction_hash, "Storage: new transaction stored");
        } else {
            debug!(%transaction_hash, "Storage: attempt to store existing transaction");
        }
        Ok(outcome)
    }

    pub(crate) fn write_transfers(
        &self,
        txn: &mut RwTransaction,
        block_hash: &BlockHash,
        transfers: &Vec<Transfer>,
    ) -> Result<bool, BlockStoreError> {
        txn.put_value(self.transfer_db, block_hash, transfers, true)
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

    pub(crate) fn write_finality_signature(
        &self,
        txn: &mut RwTransaction,
        signature: Box<FinalitySignature>,
    ) -> Result<bool, BlockStoreError> {
        let mut block_signatures = txn
            .get_value(self.block_metadata_db, signature.block_hash())
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?
            .unwrap_or_else(|| BlockSignatures::new(*signature.block_hash(), signature.era_id()));
        block_signatures.insert_signature(*signature);
        let outcome = txn
            .put_value(
                self.block_metadata_db,
                block_signatures.block_hash(),
                &block_signatures,
                true,
            )
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        Ok(outcome)
    }

    /// Retrieves a single block header in a given transaction from storage.
    pub(crate) fn get_single_block_header<Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
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
        txn: &mut Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockSignatures>, BlockStoreError> {
        txn.get_value(self.block_metadata_db, block_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))
    }

    fn get_execution_results<Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<Vec<(TransactionHash, ExecutionResult)>>, BlockStoreError> {
        let block_header = match self.get_single_block_header(txn, block_hash)? {
            Some(block_header) => block_header,
            None => return Ok(None),
        };
        let maybe_block_body = self
            .block_body_dbs
            .get(txn, block_header.body_hash())
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)));

        let Some(block_body) = maybe_block_body? else {
            debug!(
                %block_hash,
                "retrieved block header but block body is absent"
            );
            return Ok(None);
        };

        let transaction_hashes: Vec<TransactionHash> = match block_body {
            BlockBody::V1(v1) => v1
                .deploy_and_transfer_hashes()
                .map(TransactionHash::from)
                .collect(),
            BlockBody::V2(v2) => v2.all_transactions().copied().collect(),
        };
        let mut execution_results = vec![];
        for transaction_hash in transaction_hashes {
            match self
                .execution_result_dbs
                .get(txn, &transaction_hash)
                .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?
            {
                None => {
                    debug!(
                        %block_hash,
                        %transaction_hash,
                        "retrieved block but execution result for given transaction is absent"
                    );
                    return Ok(None);
                }
                Some(execution_result) => {
                    execution_results.push((transaction_hash, execution_result));
                }
            }
        }
        Ok(Some(execution_results))
    }

    /// Retrieves a single block from storage.
    pub(crate) fn get_single_block<Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
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
    ) -> Result<bool, BlockStoreError> {
        if !self
            .block_body_dbs
            .put(txn, block.body_hash(), &block.clone_body(), true)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?
        {
            error!(%block, "could not insert block body");
            return Ok(false);
        }

        let block_header = block.clone_header();
        if !self
            .block_header_dbs
            .put(txn, block.hash(), &block_header, true)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?
        {
            error!(%block, "could not insert block header");
            return Ok(false);
        }

        Ok(true)
    }

    pub(crate) fn write_block_header(
        &self,
        txn: &mut RwTransaction,
        block_header: &BlockHeader,
    ) -> Result<bool, BlockStoreError> {
        if !self
            .block_header_dbs
            .put(txn, &block_header.block_hash(), block_header, true)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?
        {
            error!(%block_header, "could not insert block header");
            return Ok(false);
        }

        Ok(true)
    }

    /// Retrieves a single transaction along with its finalized approvals.
    pub(crate) fn get_transaction_with_finalized_approvals<Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
        transaction_hash: &TransactionHash,
    ) -> Result<Option<TransactionWithFinalizedApprovals>, BlockStoreError> {
        let transaction = match self
            .transaction_dbs
            .get(txn, transaction_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?
        {
            Some(transaction) => transaction,
            None => return Ok(None),
        };
        let finalized_approvals = self
            .finalized_transaction_approvals_dbs
            .get(txn, transaction_hash)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;

        let ret = match (transaction, finalized_approvals) {
            (
                Transaction::Deploy(deploy),
                Some(FinalizedApprovals::Deploy(finalized_approvals)),
            ) => TransactionWithFinalizedApprovals::new_deploy(deploy, Some(finalized_approvals)),
            (Transaction::Deploy(deploy), None) => {
                TransactionWithFinalizedApprovals::new_deploy(deploy, None)
            }
            (Transaction::V1(transaction), Some(FinalizedApprovals::V1(finalized_approvals))) => {
                TransactionWithFinalizedApprovals::new_v1(transaction, Some(finalized_approvals))
            }
            (Transaction::V1(transaction), None) => {
                TransactionWithFinalizedApprovals::new_v1(transaction, None)
            }
            mismatch => {
                let mismatch = BlockStoreError::VariantMismatch(Box::new(mismatch));
                error!(%mismatch, "failed getting transaction with finalized approvals");
                return Err(mismatch);
            }
        };

        Ok(Some(ret))
    }

    /// Writes approvals hashes to storage.
    pub(crate) fn write_approvals_hashes(
        &self,
        txn: &mut RwTransaction,
        approvals_hashes: &ApprovalsHashes,
    ) -> Result<bool, BlockStoreError> {
        let overwrite = true;
        if !self
            .approvals_hashes_dbs
            .put(
                txn,
                approvals_hashes.block_hash(),
                approvals_hashes,
                overwrite,
            )
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?
        {
            error!("could not insert approvals' hashes: {}", approvals_hashes);
            return Ok(false);
        }
        Ok(true)
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

        let was_written = txn
            .put_value(self.transfer_db, block_hash, &transfers, true)
            .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?;
        if !was_written {
            error!(?block_hash, "failed to write transfers");
            debug_assert!(was_written);
        }
        Ok(was_written)
    }

    #[allow(clippy::type_complexity)]
    pub(crate) fn read_execution_results<Tx: LmdbTransaction>(
        &self,
        txn: &mut Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<Vec<(TransactionHash, TransactionHeader, ExecutionResult)>>, BlockStoreError>
    {
        let execution_results = match self.get_execution_results(txn, block_hash)? {
            Some(execution_results) => execution_results,
            None => return Ok(None),
        };

        let mut ret = Vec::with_capacity(execution_results.len());
        for (transaction_hash, execution_result) in execution_results {
            match self
                .transaction_dbs
                .get(txn, &transaction_hash)
                .map_err(|err| BlockStoreError::InternalStorage(Box::new(err)))?
            {
                None => {
                    error!(
                        %block_hash,
                        %transaction_hash,
                        "missing transaction"
                    );
                    return Ok(None);
                }
                Some(Transaction::Deploy(deploy)) => ret.push((
                    transaction_hash,
                    deploy.take_header().into(),
                    execution_result,
                )),
                Some(Transaction::V1(transaction_v1)) => ret.push((
                    transaction_hash,
                    transaction_v1.take_header().into(),
                    execution_result,
                )),
            };
        }
        Ok(Some(ret))
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
    let mut transfers: Vec<Transfer> = vec![];
    match execution_result {
        ExecutionResult::V1(ExecutionResultV1::Success { effect, .. }) => {
            for transform_entry in &effect.transforms {
                if let execution_result_v1::Transform::WriteTransfer(transfer) =
                    &transform_entry.transform
                {
                    transfers.push(*transfer);
                }
            }
        }
        ExecutionResult::V2(ExecutionResultV2::Success { effects, .. }) => {
            for transform in effects.transforms() {
                if let TransformKind::Write(StoredValue::Transfer(transfer)) = transform.kind() {
                    transfers.push(*transfer);
                }
            }
        }
        ExecutionResult::V1(ExecutionResultV1::Failure { .. })
        | ExecutionResult::V2(ExecutionResultV2::Failure { .. }) => {
            // No-op: we only record transfers from successful executions.
        }
    }
    transfers
}

impl BlockStoreProvider for LmdbBlockStore {
    type Reader<'a> = LmdbBlockStoreTransaction<'a, RoTransaction<'a>>;
    type Writer<'a> = LmdbBlockStoreTransaction<'a, RwTransaction<'a>>;
    type ReaderWriter<'a> = LmdbBlockStoreTransaction<'a, RwTransaction<'a>>;

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

    fn checkout_w(&mut self) -> Result<Self::Writer<'_>, BlockStoreError> {
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

pub struct LmdbBlockStoreTransaction<'a, T>
where
    T: LmdbTransaction,
{
    txn: T,
    block_store: &'a LmdbBlockStore,
}

impl<'a, T> BlockStoreTransaction for LmdbBlockStoreTransaction<'a, T>
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

impl<'a, T> BlockStoreReader for LmdbBlockStoreTransaction<'a, T>
where
    T: LmdbTransaction,
{
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

impl<'a> BlockStoreWriter for LmdbBlockStoreTransaction<'a, RwTransaction<'a>> {
    fn write_block(&mut self, block: &Block) -> Result<bool, BlockStoreError> {
        self.block_store.write_block(&mut self.txn, block)
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
        _block_height: u64,
        _era_id: EraId,
        execution_results: HashMap<TransactionHash, ExecutionResult>,
    ) -> Result<bool, BlockStoreError> {
        if !self.block_store.write_execution_results(
            &mut self.txn,
            block_hash,
            execution_results,
        )? {
            return Ok(false);
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
        self.block_store
            .write_block_header(&mut self.txn, &block_header)
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
