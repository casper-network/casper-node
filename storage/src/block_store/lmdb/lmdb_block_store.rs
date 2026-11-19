use std::{
    collections::{btree_map, BTreeMap, BTreeSet, HashMap, HashSet},
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

use datasize::DataSize;
use tempfile::TempDir;
use tracing::{debug, error, info, warn};

use casper_types::{
    execution::{execution_result_v1, ExecutionResult, ExecutionResultV1},
    Approval, AvailableBlockRange, Block, BlockBody, BlockHash, BlockHeader,
    BlockHeaderWithSignatures, BlockSignatures, BlockSignaturesV1, BlockSignaturesV2, BlockV2,
    ChainNameDigest, Digest, EraId, ExecutionInfo, FinalitySignature, ProtocolVersion, TimeDiff,
    Timestamp, Transaction, TransactionHash, TransactionId, Transfer,
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
        ApprovalsHashes, BlockExecutionResults, BlockHashHeightAndEra, BlockTransfers, StateStore,
        StateStoreKey, Transfers,
    },
    DbRawBytesSpec,
};
use lmdb::{
    Database, DatabaseFlags, Environment, EnvironmentFlags, RwCursor, RwTransaction,
    Transaction as LmdbTransaction, WriteFlags,
};

/// Filename for the LMDB database created by the Storage component.
const STORAGE_DB_FILENAME: &str = "storage.lmdb";

/// Maximum number of concurrent LMDB read transactions.
const MAX_READERS: u32 = 512;

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

type TransactionWithApprovals = (Transaction, Option<BTreeSet<Approval>>);
type BlockWithOptionalSignatures = (Block, Option<BlockSignatures>);
type BlockForExecution = (BlockV2, Vec<Transaction>, Option<ApprovalsHashes>);
type TransactionExecutionRecord = (TransactionHash, Transaction, ExecutionResult);

/// Stored data needed to build a sync leap response.
pub enum SyncLeapLookup {
    /// Some of the required blocks are unavailable.
    NotFound,
    /// The trusted block is outside the supported era window.
    NotProvided,
    /// All required headers were found in the same snapshot.
    Found {
        /// The trusted header requested by the peer.
        trusted_block_header: Box<BlockHeader>,
        /// Complete ancestors back to the previous switch block or genesis.
        trusted_ancestor_headers: Vec<BlockHeader>,
        /// Signed switch blocks and the highest complete block, when requested.
        block_headers_with_signatures: Vec<BlockHeaderWithSignatures>,
    },
}

/// The first gap encountered while loading execution records for an era.
pub enum EraExecutionGap {
    /// The block expected at this height is missing.
    MissingBlock(u64),
    /// The block is present, but at least one transaction is missing its results.
    MissingResults(u64),
}

/// Lmdb block store.
#[derive(Clone, DataSize, Debug)]
pub struct LmdbBlockStore {
    /// Storage location.
    root: PathBuf,
    /// Environment holding LMDB databases.
    #[data_size(skip)]
    env: Arc<Environment>,
    /// The block header databases.
    block_header_dbs: VersionedDatabases<BlockHash, BlockHeader>,
    /// The block body databases.
    block_body_dbs: VersionedDatabases<Digest, BlockBody>,
    /// The approvals hashes databases.
    approvals_hashes_dbs: VersionedDatabases<BlockHash, ApprovalsHashes>,
    /// The block metadata db.
    block_metadata_dbs: VersionedDatabases<BlockHash, BlockSignatures>,
    /// The transaction databases.
    transaction_dbs: VersionedDatabases<TransactionHash, Transaction>,
    /// Databases of `ExecutionResult`s indexed by transaction hash for current DB or by deploy
    /// hash for legacy DB.
    execution_result_dbs: VersionedDatabases<TransactionHash, ExecutionResult>,
    /// The transfer databases.
    transfer_dbs: VersionedDatabases<BlockHash, Transfers>,
    /// The state storage database.
    #[data_size(skip)]
    state_store_db: Database,
    /// The finalized transaction approvals databases.
    finalized_transaction_approvals_dbs: VersionedDatabases<TransactionHash, BTreeSet<Approval>>,
    /// Disk-backed index of block height to block hash.
    #[data_size(skip)]
    block_height_index_db: Database,
    /// Disk-backed index of era ID to switch block hash.
    #[data_size(skip)]
    switch_block_era_id_index_db: Database,
    /// Disk-backed index of transaction hash to the hash, height and era of the block containing
    /// it.
    #[data_size(skip)]
    transaction_hash_index_db: Database,
    /// Keeps a temporary storage directory alive for stores created by [`Self::new_temporary`].
    ///
    /// The `Arc` ensures shallow-cloned store handles retain the directory until the last handle
    /// is dropped.
    #[data_size(skip)]
    temporary_directory: Option<Arc<TempDir>>,
}

impl LmdbBlockStore {
    /// Ctor.
    pub fn new(root_path: &Path, total_size: usize) -> Result<Self, BlockStoreError> {
        // Create the environment and databases.
        let env = new_environment(total_size, root_path)?;

        let block_header_dbs = VersionedDatabases::new(&env, "block_header", "block_header_v2")
            .map_err(internal_storage)?;
        let block_body_dbs =
            VersionedDatabases::<_, BlockBody>::new(&env, "block_body", "block_body_v2")
                .map_err(internal_storage)?;
        let block_metadata_dbs =
            VersionedDatabases::new(&env, "block_metadata", "block_metadata_v2")
                .map_err(internal_storage)?;
        let transaction_dbs =
            VersionedDatabases::new(&env, "deploys", "transactions").map_err(internal_storage)?;
        let execution_result_dbs =
            VersionedDatabases::new(&env, "deploy_metadata", "execution_results")
                .map_err(internal_storage)?;
        let transfer_dbs = VersionedDatabases::new(&env, "transfer", "versioned_transfers")
            .map_err(internal_storage)?;
        let state_store_db = env
            .create_db(Some("state_store"), DatabaseFlags::empty())
            .map_err(internal_storage)?;

        let finalized_transaction_approvals_dbs =
            VersionedDatabases::new(&env, "finalized_approvals", "versioned_finalized_approvals")
                .map_err(internal_storage)?;
        let approvals_hashes_dbs =
            VersionedDatabases::new(&env, "approvals_hashes", "versioned_approvals_hashes")
                .map_err(internal_storage)?;

        let block_height_index_db = env
            .create_db(Some("block_height_index"), DatabaseFlags::empty())
            .map_err(internal_storage)?;
        let switch_block_era_id_index_db = env
            .create_db(Some("switch_block_era_id_index"), DatabaseFlags::empty())
            .map_err(internal_storage)?;
        let transaction_hash_index_db = env
            .create_db(Some("transaction_hash_index"), DatabaseFlags::empty())
            .map_err(internal_storage)?;

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
            temporary_directory: None,
        })
    }

    /// Creates a block store which owns its temporary directory.
    pub fn new_temporary(total_size: usize) -> Result<Self, BlockStoreError> {
        let temp_dir = tempfile::tempdir().map_err(internal_storage)?;
        let mut block_store = Self::new(temp_dir.path(), total_size)?;
        block_store.temporary_directory = Some(Arc::new(temp_dir));
        Ok(block_store)
    }

    /// Stores a block.
    pub fn put_block(&mut self, block: &Block) -> Result<BlockHash, BlockStoreError> {
        let mut txn = self.env.begin_rw_txn().map_err(internal_storage)?;
        let hash = self.write_block(&mut txn, block)?;
        txn.commit().map_err(commit_error)?;
        Ok(hash)
    }

    /// Stores a header.
    pub fn put_block_header(&mut self, header: &BlockHeader) -> Result<BlockHash, BlockStoreError> {
        let mut txn = self.env.begin_rw_txn().map_err(internal_storage)?;
        let hash = self.write_block_header(&mut txn, header)?;
        txn.commit().map_err(commit_error)?;
        Ok(hash)
    }

    /// Stores a batch of headers atomically.
    pub fn put_block_headers(&mut self, headers: &[BlockHeader]) -> Result<(), BlockStoreError> {
        let mut txn = self.env.begin_rw_txn().map_err(internal_storage)?;
        for header in headers {
            self.write_block_header(&mut txn, header)?;
        }
        txn.commit().map_err(commit_error)
    }

    /// Stores the block, approvals and execution results in one transaction.
    pub fn put_executed_block(
        &mut self,
        block: &Block,
        approvals_hashes: &ApprovalsHashes,
        execution_results: HashMap<TransactionHash, ExecutionResult>,
    ) -> Result<BlockHash, BlockStoreError> {
        let mut txn = self.env.begin_rw_txn().map_err(internal_storage)?;
        let block_hash = self.write_block(&mut txn, block)?;
        self.write_approvals_hashes(&mut txn, approvals_hashes)?;
        self.write_execution_results(
            &mut txn,
            BlockHashHeightAndEra::new(block_hash, block.height(), block.era_id()),
            &execution_results,
        )?;
        txn.commit().map_err(commit_error)?;
        Ok(block_hash)
    }

    /// Checks the persisted index for the first missing header in the range.
    pub fn first_missing_block_header_height(
        &self,
        range: Range<u64>,
    ) -> Result<Option<u64>, BlockStoreError> {
        let txn = self.env.begin_ro_txn().map_err(internal_storage)?;
        for height in range {
            if self.block_hash_at_height(&txn, height)?.is_none() {
                return Ok(Some(height));
            }
        }
        Ok(None)
    }

    /// Prunes blocks from earlier protocol versions at or after the hard-reset era.
    pub fn prune_for_hard_reset(
        &mut self,
        hard_reset_to_start_of_era: Option<EraId>,
        protocol_version: ProtocolVersion,
    ) -> Result<(), BlockStoreError> {
        let Some(invalid_era) = hard_reset_to_start_of_era else {
            return Ok(());
        };

        info!("pruning block store");
        let mut txn = self.env.begin_rw_txn().map_err(internal_storage)?;
        let Some(tip) = self.tip_header(&txn)? else {
            info!("block store is empty, nothing to prune");
            return Ok(());
        };
        let total_headers = tip.height().saturating_add(1);
        let progress_step = (total_headers / 20).max(1);

        // A body can be shared by multiple headers, including headers we retain.
        let mut blocks_to_delete = Vec::new();
        let mut body_hash_retained = HashMap::<Digest, bool>::new();
        for height in 0..=tip.height() {
            if height % progress_step == 0 {
                info!(
                    percent_complete = (height.saturating_mul(100) / total_headers),
                    height, total_headers, "pruning block store: scanning"
                );
            }
            let Some(header) = self.header_at_height(&txn, height)? else {
                continue;
            };
            let should_retain =
                header.era_id() < invalid_era || header.protocol_version() == protocol_version;
            *body_hash_retained.entry(*header.body_hash()).or_default() |= should_retain;
            if !should_retain {
                blocks_to_delete.push(header.block_hash());
            }
        }

        let blocks_to_delete_count = blocks_to_delete.len();
        if blocks_to_delete_count == 0 {
            info!("block store pruning complete: nothing to prune");
            return Ok(());
        }

        for hash in blocks_to_delete {
            if let Some(block) = self.get_single_block(&txn, &hash)? {
                self.delete_transaction_execution_records(
                    &mut txn,
                    block.all_transaction_hashes(),
                )?;
                self.delete_block_header_and_signatures(&mut txn, &hash)?;
                self.block_body_dbs
                    .delete(&mut txn, block.body_hash())
                    .map_err(internal_storage)?;
            }
        }
        for (body_hash, retained) in body_hash_retained {
            if !retained {
                self.block_body_dbs
                    .delete(&mut txn, &body_hash)
                    .map_err(internal_storage)?;
            }
        }

        txn.commit().map_err(commit_error)?;
        info!(blocks_to_delete_count, "block store pruning complete");
        Ok(())
    }

    /// Initializes the disk-backed indexes. This operation can be time
    /// consuming because it needs to go through all entries in block
    /// headers db. If the index has data it assumes that no reindexing
    /// is needed.
    pub fn init(&mut self) -> Result<(), BlockStoreError> {
        let ro_txn = self.env.begin_ro_txn().map_err(internal_storage)?;
        let index_is_empty = ro_txn
            .stat(self.block_height_index_db)
            .map_err(internal_storage)?
            .entries()
            == 0;
        let headers_exist = header_count(&ro_txn, self)? > 0;
        ro_txn.commit().map_err(internal_storage)?;

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

    /// Reads a block by its hash.
    pub fn read_block_by_hash(
        &self,
        block_hash: BlockHash,
    ) -> Result<Option<Block>, BlockStoreError> {
        self.get_single_block(
            &self.env.begin_ro_txn().map_err(internal_storage)?,
            &block_hash,
        )
    }

    /// Returns whether a block exists at the given hash.
    pub fn block_exists_by_hash(&self, block_hash: BlockHash) -> Result<bool, BlockStoreError> {
        self.block_exists(
            &self.env.begin_ro_txn().map_err(internal_storage)?,
            &block_hash,
        )
    }

    /// Reads a block header by its hash.
    pub fn read_block_header_by_hash(
        &self,
        block_hash: BlockHash,
    ) -> Result<Option<BlockHeader>, BlockStoreError> {
        self.get_single_block_header(
            &self.env.begin_ro_txn().map_err(internal_storage)?,
            &block_hash,
        )
    }

    /// Reads approvals hashes by block hash.
    pub fn read_approvals_hashes_by_hash(
        &self,
        block_hash: BlockHash,
    ) -> Result<Option<ApprovalsHashes>, BlockStoreError> {
        self.read_approvals_hashes(
            &self.env.begin_ro_txn().map_err(internal_storage)?,
            &block_hash,
        )
    }

    /// Reads block signatures by block hash.
    pub fn read_block_signatures_by_hash(
        &self,
        block_hash: BlockHash,
    ) -> Result<Option<BlockSignatures>, BlockStoreError> {
        self.get_block_signatures(
            &self.env.begin_ro_txn().map_err(internal_storage)?,
            &block_hash,
        )
    }

    /// Reads a transaction by its hash.
    pub fn read_transaction(
        &self,
        transaction_hash: TransactionHash,
    ) -> Result<Option<Transaction>, BlockStoreError> {
        self.get_transaction(
            &self.env.begin_ro_txn().map_err(internal_storage)?,
            transaction_hash,
        )
    }

    /// Returns whether a transaction exists at the given hash.
    pub fn transaction_exists(
        &self,
        transaction_hash: TransactionHash,
    ) -> Result<bool, BlockStoreError> {
        self.has_transaction(
            &self.env.begin_ro_txn().map_err(internal_storage)?,
            &transaction_hash,
        )
    }

    /// Reads an execution result by transaction hash.
    pub fn read_execution_result(
        &self,
        transaction_hash: TransactionHash,
    ) -> Result<Option<ExecutionResult>, BlockStoreError> {
        self.get_execution_result(
            &self.env.begin_ro_txn().map_err(internal_storage)?,
            transaction_hash,
        )
    }

    /// Reads transfers by block hash.
    pub fn read_transfers(
        &self,
        block_hash: BlockHash,
    ) -> Result<Option<Vec<Transfer>>, BlockStoreError> {
        self.get_transfers(
            &self.env.begin_ro_txn().map_err(internal_storage)?,
            &block_hash,
        )
    }

    /// Reads a state-store value.
    pub fn read_state_store(&self, key: StateStoreKey) -> Result<Option<Vec<u8>>, BlockStoreError> {
        self.load_state_store(&self.env.begin_ro_txn().map_err(internal_storage)?, &key.0)
    }

    /// Reads a block by height.
    pub fn read_block_at_height(&self, height: u64) -> Result<Option<Block>, BlockStoreError> {
        self.block_at_height(&self.env.begin_ro_txn().map_err(internal_storage)?, height)
    }

    /// Reads a block header by height.
    pub fn read_block_header_at_height(
        &self,
        height: u64,
    ) -> Result<Option<BlockHeader>, BlockStoreError> {
        self.header_at_height(&self.env.begin_ro_txn().map_err(internal_storage)?, height)
    }

    /// Reads a switch block by era ID.
    pub fn read_switch_block_by_era(
        &self,
        era_id: EraId,
    ) -> Result<Option<Block>, BlockStoreError> {
        let txn = self.env.begin_ro_txn().map_err(internal_storage)?;
        self.indexed_block(
            &txn,
            self.switch_block_era_id_index_db,
            Some(era_id.value()),
        )
    }

    /// Reads a switch block header by era ID.
    pub fn read_block_header_by_era(
        &self,
        era_id: EraId,
    ) -> Result<Option<BlockHeader>, BlockStoreError> {
        self.header_by_era(&self.env.begin_ro_txn().map_err(internal_storage)?, era_id)
    }

    /// Reads the highest block header.
    pub fn read_tip_block_header(&self) -> Result<Option<BlockHeader>, BlockStoreError> {
        self.tip_header(&self.env.begin_ro_txn().map_err(internal_storage)?)
    }

    /// Reads the highest block.
    pub fn read_tip_block(&self) -> Result<Option<Block>, BlockStoreError> {
        self.tip_block(&self.env.begin_ro_txn().map_err(internal_storage)?)
    }

    /// Reads the highest switch block header.
    pub fn read_latest_switch_block_header(&self) -> Result<Option<BlockHeader>, BlockStoreError> {
        self.latest_switch_header(&self.env.begin_ro_txn().map_err(internal_storage)?)
    }

    /// Reads raw bytes from a database table.
    pub fn read_raw(
        &self,
        table_id: DbTableId,
        key: Vec<u8>,
    ) -> Result<Option<DbRawBytesSpec>, BlockStoreError> {
        let txn = self.env.begin_ro_txn().map_err(internal_storage)?;
        if key.is_empty() {
            return Ok(None);
        }
        let result = match table_id {
            DbTableId::BlockHeader => self.block_header_dbs.get_raw(&txn, &key),
            DbTableId::BlockBody => self.block_body_dbs.get_raw(&txn, &key),
            DbTableId::ApprovalsHashes => self.approvals_hashes_dbs.get_raw(&txn, &key),
            DbTableId::BlockMetadata => self.block_metadata_dbs.get_raw(&txn, &key),
            DbTableId::Transaction => self.transaction_dbs.get_raw(&txn, &key),
            DbTableId::ExecutionResult => self.execution_result_dbs.get_raw(&txn, &key),
            DbTableId::Transfer => self.transfer_dbs.get_raw(&txn, &key),
            DbTableId::FinalizedTransactionApprovals => {
                self.finalized_transaction_approvals_dbs.get_raw(&txn, &key)
            }
        };
        result.map_err(internal_storage)
    }

    /// Stores approvals hashes.
    pub fn put_approvals_hashes(
        &mut self,
        approvals: &ApprovalsHashes,
    ) -> Result<(), BlockStoreError> {
        let mut txn = self.env.begin_rw_txn().map_err(internal_storage)?;
        self.write_approvals_hashes(&mut txn, approvals)?;
        txn.commit().map_err(commit_error)
    }

    /// Inserts a transaction if its hash is absent.
    pub fn put_transaction(&mut self, transaction: &Transaction) -> Result<bool, BlockStoreError> {
        let mut txn = self.env.begin_rw_txn().map_err(internal_storage)?;
        let hash = transaction.hash();
        if self.has_transaction(&txn, &hash)? {
            return Ok(false);
        }
        self.transaction_dbs
            .put(&mut txn, &hash, transaction, false)
            .map_err(internal_storage)?;
        txn.commit().map_err(commit_error)?;
        Ok(true)
    }

    /// Writes execution results and returns the block, if present in the same transaction.
    pub fn put_execution_results(
        &mut self,
        results: &BlockExecutionResults,
    ) -> Result<Option<Block>, BlockStoreError> {
        let mut txn = self.env.begin_rw_txn().map_err(internal_storage)?;
        let block = self.get_single_block(&txn, &results.block_info.block_hash)?;
        self.write_execution_results(&mut txn, results.block_info, &results.exec_results)?;
        txn.commit().map_err(commit_error)?;
        Ok(block)
    }

    /// Merges a collection of signatures. Returns false for an empty or incompatible collection.
    pub fn put_block_signatures(
        &mut self,
        signatures: BlockSignatures,
    ) -> Result<bool, BlockStoreError> {
        if signatures.is_empty() {
            return Ok(false);
        }
        let mut txn = self.env.begin_rw_txn().map_err(internal_storage)?;
        let signatures = match self.get_block_signatures(&txn, signatures.block_hash())? {
            None => signatures,
            Some(mut stored) => {
                if let Err(error) = stored.merge(signatures) {
                    error!(%error, "failed to put block signatures");
                    return Ok(false);
                }
                stored
            }
        };
        self.write_finality_signatures(&mut txn, &signatures)?;
        txn.commit().map_err(commit_error)?;
        Ok(true)
    }

    /// Stores one finality signature, returning false if its version conflicts with the record.
    pub fn put_finality_signature(
        &mut self,
        signature: &FinalitySignature,
    ) -> Result<bool, BlockStoreError> {
        let mut txn = self.env.begin_rw_txn().map_err(internal_storage)?;
        let mut signatures = match self.get_block_signatures(&txn, signature.block_hash())? {
            Some(signatures) => signatures,
            None => match signature {
                FinalitySignature::V1(signature) => {
                    BlockSignaturesV1::new(*signature.block_hash(), signature.era_id()).into()
                }
                FinalitySignature::V2(signature) => BlockSignaturesV2::new(
                    *signature.block_hash(),
                    signature.block_height(),
                    signature.era_id(),
                    signature.chain_name_hash(),
                )
                .into(),
            },
        };
        match (&mut signatures, signature) {
            (BlockSignatures::V1(signatures), FinalitySignature::V1(signature)) => {
                signatures.insert_signature(signature.public_key().clone(), *signature.signature());
            }
            (BlockSignatures::V2(signatures), FinalitySignature::V2(signature)) => {
                signatures.insert_signature(signature.public_key().clone(), *signature.signature());
            }
            _ => return Ok(false),
        }
        self.write_finality_signatures(&mut txn, &signatures)?;
        txn.commit().map_err(commit_error)?;
        Ok(true)
    }

    /// Deletes the signature records for the supplied blocks in one transaction.
    pub fn delete_block_signatures(&mut self, hashes: &[BlockHash]) -> Result<(), BlockStoreError> {
        let mut txn = self.env.begin_rw_txn().map_err(internal_storage)?;
        for &hash in hashes {
            self.delete_finality_signatures(&mut txn, &hash)?;
        }
        txn.commit().map_err(commit_error)
    }

    /// Stores a state-store value.
    pub fn put_state_store(&mut self, state: &StateStore) -> Result<(), BlockStoreError> {
        let mut txn = self.env.begin_rw_txn().map_err(internal_storage)?;
        txn.put(
            self.state_store_db,
            &state.key,
            &state.value,
            WriteFlags::default(),
        )
        .map_err(internal_storage)?;
        txn.commit().map_err(commit_error)
    }

    /// Stores changed finalized approvals. Returns None when the transaction is missing.
    pub fn put_finalized_approvals(
        &mut self,
        hash: TransactionHash,
        approvals: &BTreeSet<Approval>,
    ) -> Result<Option<bool>, BlockStoreError> {
        let mut txn = self.env.begin_rw_txn().map_err(internal_storage)?;
        let Some(original) = self.get_transaction(&txn, hash)? else {
            return Ok(None);
        };
        if self.get_finalized_approvals(&txn, hash)?.as_ref() == Some(approvals)
            || original.approvals() == *approvals
        {
            return Ok(Some(false));
        }
        self.finalized_transaction_approvals_dbs
            .put(&mut txn, &hash, approvals, true)
            .map_err(internal_storage)?;
        txn.commit().map_err(commit_error)?;
        Ok(Some(true))
    }

    /// Stores transfers for a block.
    pub fn put_transfers(
        &mut self,
        transfers: &BlockTransfers,
    ) -> Result<BlockHash, BlockStoreError> {
        let mut txn = self.env.begin_rw_txn().map_err(internal_storage)?;
        self.write_transfers(&mut txn, &transfers.block_hash, &transfers.transfers)?;
        txn.commit().map_err(commit_error)?;
        Ok(transfers.block_hash)
    }

    /// Reads block transfers and repairs a missing or empty record from execution results.
    ///
    /// Older sync paths could leave the record missing or empty. See
    /// <https://github.com/casper-network/casper-node/issues/4255> and
    /// <https://github.com/casper-network/casper-node/issues/4268>.
    pub fn get_or_rebuild_block_transfers(
        &mut self,
        hash: BlockHash,
    ) -> Result<Option<Vec<Transfer>>, BlockStoreError> {
        let mut txn = self.env.begin_rw_txn().map_err(internal_storage)?;
        if let Some(transfers) = self.get_transfers(&txn, &hash)? {
            if !transfers.is_empty() {
                return Ok(Some(transfers));
            }
        }
        let Some(block) = self.get_single_block(&txn, &hash)? else {
            return Ok(None);
        };
        let mut transfers = Vec::new();
        for tx_hash in block
            .all_transaction_hashes()
            .filter(|hash| matches!(hash, TransactionHash::Deploy(_)))
        {
            match self.get_execution_result(&txn, tx_hash)? {
                Some(result) => transfers.extend(successful_transfers(&result)),
                None => error!(%tx_hash, "should have exec result"),
            }
        }
        self.write_transfers(&mut txn, &hash, &transfers)?;
        txn.commit().map_err(commit_error)?;
        Ok(Some(transfers))
    }

    /// Reads the completion state and its maximum possible height in one snapshot.
    /// Without persisted completion state, only pre-upgrade blocks are assumed complete.
    pub fn read_completion_state(
        &self,
        key: StateStoreKey,
        protocol_version: ProtocolVersion,
    ) -> Result<(Option<Vec<u8>>, Option<u64>), BlockStoreError> {
        let txn = self.env.begin_ro_txn().map_err(internal_storage)?;
        let value = self.load_state_store(&txn, &key.0)?;
        let Some(tip) = self.tip_header(&txn)? else {
            return Ok((value, None));
        };
        if value.is_some() {
            return Ok((value, Some(tip.height())));
        }
        for height in (0..=tip.height()).rev() {
            if self
                .header_at_height(&txn, height)?
                .is_some_and(|header| header.protocol_version() < protocol_version)
            {
                return Ok((None, Some(height)));
            }
        }
        Ok((None, None))
    }

    /// Reads the eras associated with the stored transactions.
    pub fn read_transaction_era_ids(
        &self,
        hashes: impl IntoIterator<Item = TransactionHash>,
    ) -> Result<HashSet<EraId>, BlockStoreError> {
        let txn = self.env.begin_ro_txn().map_err(internal_storage)?;
        let mut eras = HashSet::new();
        for hash in hashes {
            if let Some(info) = self.block_info_by_transaction(&txn, hash)? {
                eras.insert(info.era_id);
            }
        }
        Ok(eras)
    }

    /// Reads a transaction and its finalized approvals.
    pub fn read_transaction_with_finalized_approvals(
        &self,
        hash: TransactionHash,
    ) -> Result<Option<TransactionWithApprovals>, BlockStoreError> {
        self.transaction_with_finalized_approvals(
            &self.env.begin_ro_txn().map_err(internal_storage)?,
            hash,
        )
    }

    /// Reads transactions with their finalized approvals from one snapshot.
    pub fn read_transactions_with_finalized_approvals(
        &self,
        hashes: impl IntoIterator<Item = TransactionHash>,
    ) -> Result<Vec<Option<TransactionWithApprovals>>, BlockStoreError> {
        let txn = self.env.begin_ro_txn().map_err(internal_storage)?;
        hashes
            .into_iter()
            .map(|hash| self.transaction_with_finalized_approvals(&txn, hash))
            .collect()
    }

    /// Reads transactions using the finalized approvals when available, and checks each ID.
    pub fn read_transactions_matching_ids(
        &self,
        ids: impl IntoIterator<Item = TransactionId>,
    ) -> Result<Vec<Option<Transaction>>, BlockStoreError> {
        let txn = self.env.begin_ro_txn().map_err(internal_storage)?;
        ids.into_iter()
            .map(|id| self.transaction_matching_finalized_id(&txn, id))
            .collect()
    }

    /// Reads a transaction using the finalized approvals when available, and checks its ID.
    pub fn read_transaction_matching_id(
        &self,
        id: TransactionId,
    ) -> Result<Option<Transaction>, BlockStoreError> {
        self.transaction_matching_finalized_id(
            &self.env.begin_ro_txn().map_err(internal_storage)?,
            id,
        )
    }

    /// Reads a transaction for a fetch. An exact ID has no approval override.
    /// Other IDs are returned with the finalized approvals for the caller to check.
    pub fn read_transaction_for_fetch(
        &self,
        id: TransactionId,
    ) -> Result<Option<TransactionWithApprovals>, BlockStoreError> {
        let txn = self.env.begin_ro_txn().map_err(internal_storage)?;
        let Some(transaction) = self.get_transaction(&txn, id.transaction_hash())? else {
            return Ok(None);
        };
        if transaction.compute_id() == id {
            return Ok(Some((transaction, None)));
        }
        Ok(self
            .get_finalized_approvals(&txn, id.transaction_hash())?
            .map(|approvals| (transaction, Some(approvals))))
    }

    /// Reads a transaction and any execution information in the same snapshot.
    pub fn read_transaction_and_execution_info(
        &self,
        hash: TransactionHash,
        with_finalized_approvals: bool,
    ) -> Result<Option<(Transaction, Option<ExecutionInfo>)>, BlockStoreError> {
        let txn = self.env.begin_ro_txn().map_err(internal_storage)?;
        let transaction = if with_finalized_approvals {
            self.transaction_with_finalized_approvals(&txn, hash)?
                .map(apply_finalized_approvals)
        } else {
            self.get_transaction(&txn, hash)?
        };
        let Some(transaction) = transaction else {
            return Ok(None);
        };
        Ok(Some((transaction, self.execution_info(&txn, hash)?)))
    }

    /// Reads execution information for a transaction.
    pub fn read_execution_info(
        &self,
        hash: TransactionHash,
    ) -> Result<Option<ExecutionInfo>, BlockStoreError> {
        self.execution_info(&self.env.begin_ro_txn().map_err(internal_storage)?, hash)
    }

    /// Reads a block and any stored signatures in the same snapshot.
    pub fn read_block_and_signatures_by_hash(
        &self,
        hash: BlockHash,
    ) -> Result<Option<BlockWithOptionalSignatures>, BlockStoreError> {
        let txn = self.env.begin_ro_txn().map_err(internal_storage)?;
        self.block_with_signatures(&txn, self.get_single_block(&txn, &hash)?)
    }

    /// Reads a block and its signatures at a height, or at the tip if a height is absent.
    pub fn read_block_and_signatures_at_height(
        &self,
        height: Option<u64>,
    ) -> Result<Option<BlockWithOptionalSignatures>, BlockStoreError> {
        let txn = self.env.begin_ro_txn().map_err(internal_storage)?;
        let block = match height {
            Some(height) => self.block_at_height(&txn, height)?,
            None => self.tip_block(&txn)?,
        };
        self.block_with_signatures(&txn, block)
    }

    /// Reads a V2 block, its transactions in block order, and its approvals hashes in one snapshot.
    /// Returns `None` if the block or any of its transactions is missing.
    pub fn read_block_for_execution(
        &self,
        hash: BlockHash,
    ) -> Result<Option<BlockForExecution>, BlockStoreError> {
        let txn = self.env.begin_ro_txn().map_err(internal_storage)?;
        let Some(Block::V2(block)) = self.get_single_block(&txn, &hash)? else {
            return Ok(None);
        };
        let mut transactions = Vec::new();
        for &transaction_hash in block.all_transactions() {
            let Some(transaction) = self.get_transaction(&txn, transaction_hash)? else {
                debug!(block_hash = %hash, %transaction_hash, "missing transaction for execution");
                return Ok(None);
            };
            transactions.push(transaction);
        }
        let approvals_hashes = self.read_approvals_hashes(&txn, &hash)?;
        Ok(Some((block, transactions, approvals_hashes)))
    }

    /// Reads the highest switch block headers, returning them oldest first.
    pub fn read_highest_switch_block_headers(
        &self,
        count: u64,
    ) -> Result<Vec<BlockHeader>, BlockStoreError> {
        let txn = self.env.begin_ro_txn().map_err(internal_storage)?;
        let Some(latest) = self.latest_switch_header(&txn)? else {
            return Ok(vec![]);
        };
        let last_era = latest.era_id().value();
        let mut headers = vec![latest];
        for era in (0..last_era).rev().take(count as usize).map(EraId::new) {
            let Some(header) = self.header_by_era(&txn, era)? else {
                break;
            };
            headers.push(header);
        }
        headers.reverse();
        Ok(headers)
    }

    /// Reads blocks at the supplied heights until one predates the replay window.
    pub fn read_blocks_for_replay_protection(
        &self,
        heights: impl IntoIterator<Item = u64>,
        max_ttl: TimeDiff,
    ) -> Result<Vec<Block>, BlockStoreError> {
        let txn = self.env.begin_ro_txn().map_err(internal_storage)?;
        let timestamp = match self.latest_switch_header(&txn)? {
            Some(header) => header.timestamp().saturating_sub(max_ttl),
            None => Timestamp::now(),
        };
        let mut blocks = Vec::new();
        for height in heights {
            if let Some(block) = self.block_at_height(&txn, height)? {
                let in_window = block.timestamp() >= timestamp;
                blocks.push(block);
                if !in_window {
                    break;
                }
            }
        }
        Ok(blocks)
    }

    /// Reads the execution results in the block's transaction order.
    pub fn read_block_execution_results(
        &self,
        hash: BlockHash,
    ) -> Result<Option<Vec<(TransactionHash, ExecutionResult)>>, BlockStoreError> {
        let txn = self.env.begin_ro_txn().map_err(internal_storage)?;
        let Some(block) = self.get_single_block(&txn, &hash)? else {
            return Ok(None);
        };
        self.block_execution_results(&txn, &block)
    }

    /// Reads the transactions and execution results in the block's transaction order.
    pub fn read_block_transactions_and_results(
        &self,
        hash: BlockHash,
    ) -> Result<Option<Vec<TransactionExecutionRecord>>, BlockStoreError> {
        let txn = self.env.begin_ro_txn().map_err(internal_storage)?;
        let Some(block) = self.get_single_block(&txn, &hash)? else {
            return Ok(None);
        };
        let Some(results) = self.block_execution_results(&txn, &block)? else {
            return Ok(None);
        };
        let mut records = Vec::with_capacity(results.len());
        for (hash, result) in results {
            let Some(transaction) = self.get_transaction(&txn, hash)? else {
                error!(%hash, "missing transaction");
                return Ok(None);
            };
            records.push((hash, transaction, result));
        }
        Ok(Some(records))
    }

    /// Visits each completed block and its results within the newest era in the range.
    /// Returns the first missing block or results, if any.
    pub fn visit_latest_era_execution_records(
        &self,
        heights: std::ops::RangeInclusive<u64>,
        mut visit: impl FnMut(Block, Vec<(TransactionHash, ExecutionResult)>),
    ) -> Result<Option<EraExecutionGap>, BlockStoreError> {
        let txn = self.env.begin_ro_txn().map_err(internal_storage)?;
        let mut era = None;
        for height in heights.rev() {
            let Some(block) = self.block_at_height(&txn, height)? else {
                return Ok(Some(EraExecutionGap::MissingBlock(height)));
            };
            if *era.get_or_insert(block.era_id()) != block.era_id() {
                break;
            }
            let Some(results) = self.block_execution_results(&txn, &block)? else {
                return Ok(Some(EraExecutionGap::MissingResults(height)));
            };
            visit(block, results);
        }
        Ok(None)
    }

    /// Reads the height of the switch block for the given era.
    pub fn get_switch_block_height(&self, era: EraId) -> Result<Option<u64>, BlockStoreError> {
        Ok(self
            .read_block_header_by_era(era)?
            .map(|header| header.height()))
    }

    /// Reads all headers and signatures needed for a sync leap from one LMDB snapshot.
    pub fn read_sync_leap(
        &self,
        trusted_hash: BlockHash,
        trusted_ancestor_only: bool,
        available_blocks: AvailableBlockRange,
        highest_complete_height: Option<u64>,
        recent_era_count: u64,
        chain_name_hash: ChainNameDigest,
    ) -> Result<SyncLeapLookup, BlockStoreError> {
        let txn = self.env.begin_ro_txn().map_err(internal_storage)?;
        let Some(trusted) = self
            .get_single_block_header(&txn, &trusted_hash)?
            .filter(|header| available_blocks.contains(header.height()))
        else {
            return Ok(SyncLeapLookup::NotFound);
        };
        let Some(mut ancestors) = self.trusted_ancestors(&txn, &trusted, available_blocks)? else {
            return Ok(SyncLeapLookup::NotFound);
        };
        let mut signed_headers = Vec::new();
        if !trusted_ancestor_only {
            let Some(height) = highest_complete_height else {
                return Ok(SyncLeapLookup::NotFound);
            };
            let Some(highest) = self.header_at_height(&txn, height)? else {
                return Ok(SyncLeapLookup::NotFound);
            };
            let highest = self.header_with_signatures(&txn, highest, chain_name_hash)?;
            if highest
                .block_header()
                .era_id()
                .saturating_sub(trusted.era_id().into())
                > recent_era_count.into()
            {
                return Ok(SyncLeapLookup::NotProvided);
            }
            if height == 0 {
                ancestors.clear();
            } else if trusted.block_hash() != highest.block_header().block_hash() {
                let start = trusted.next_block_era_id().value();
                let end = highest.block_header().era_id().value();
                for era in start..end {
                    let Some(header) = self.header_by_era(&txn, EraId::new(era))? else {
                        return Ok(SyncLeapLookup::NotFound);
                    };
                    signed_headers.push(self.header_with_signatures(
                        &txn,
                        header,
                        chain_name_hash,
                    )?);
                }
                signed_headers.push(highest);
            }
        }
        Ok(SyncLeapLookup::Found {
            trusted_block_header: Box::new(trusted),
            trusted_ancestor_headers: ancestors,
            block_headers_with_signatures: signed_headers,
        })
    }

    fn rebuild_indexes(&mut self) -> Result<(), BlockStoreError> {
        info!("reindexing block store");

        let mut block_height_index = BTreeMap::new();
        let mut switch_block_era_id_index = BTreeMap::new();
        let mut transaction_hash_index = BTreeMap::new();

        let mut txn = self.env.begin_rw_txn().map_err(internal_storage)?;

        let total_headers = header_count(&txn, self)?;
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
            .for_each_value_in_current(&mut txn, &mut collect_fn)?;
        self.block_header_dbs
            .for_each_value_in_legacy(&mut txn, &mut collect_fn)?;

        // Read the bodies after both header cursors have closed.
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
                .get(&txn, block_header.body_hash())
                .map_err(internal_storage)?;
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

        txn.clear_db(self.block_height_index_db)
            .map_err(internal_storage)?;
        txn.clear_db(self.switch_block_era_id_index_db)
            .map_err(internal_storage)?;
        txn.clear_db(self.transaction_hash_index_db)
            .map_err(internal_storage)?;

        // `block_height_index`/`switch_block_era_id_index`/`transaction_hash_index` are
        // `BTreeMap`s, so iterating them yields ascending key order; combined with the `clear_db`
        // calls above, this lets us use LMDB's `APPEND` flag to skip the usual B-tree
        // search/rebalance per insert (a significant speedup for a full rebuild). This is safe
        // because: the two `u64`/`EraId`-keyed indexes use `append_by_be_u64_key`, whose
        // big-endian key encoding is specifically chosen so ascending numeric order is ascending
        // byte order; and `TransactionHash`'s derived `Ord` (variant tag, then digest bytes)
        // matches its `bytesrepr` encoding (tag byte, then raw digest bytes) byte-for-byte.
        for (height, block_hash) in block_height_index {
            append_by_be_u64_key(&mut txn, self.block_height_index_db, height, &block_hash)
                .map_err(internal_storage)?;
        }
        for (era_id, block_hash) in switch_block_era_id_index {
            append_by_be_u64_key(
                &mut txn,
                self.switch_block_era_id_index_db,
                era_id.value(),
                &block_hash,
            )
            .map_err(internal_storage)?;
        }
        for (transaction_hash, block_info) in transaction_hash_index {
            append_value_bytesrepr(
                &mut txn,
                self.transaction_hash_index_db,
                &transaction_hash,
                &block_info,
            )
            .map_err(internal_storage)?;
        }

        txn.commit().map_err(internal_storage)?;

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
    fn write_finality_signatures(
        &self,
        txn: &mut RwTransaction,
        signatures: &BlockSignatures,
    ) -> Result<(), BlockStoreError> {
        self.block_metadata_dbs
            .put(txn, signatures.block_hash(), signatures, true)
            .map(|_| ())
            .map_err(internal_storage)
    }

    fn delete_finality_signatures(
        &self,
        txn: &mut RwTransaction,
        block_hash: &BlockHash,
    ) -> Result<(), BlockStoreError> {
        self.block_metadata_dbs
            .delete(txn, block_hash)
            .map_err(internal_storage)
    }

    fn has_transaction<Tx: lmdb::Transaction>(
        &self,
        txn: &Tx,
        transaction_hash: &TransactionHash,
    ) -> Result<bool, BlockStoreError> {
        self.transaction_dbs
            .exists(txn, transaction_hash)
            .map_err(internal_storage)
    }

    /// Returns `true` if the given block's header and body are stored.
    fn block_exists<Tx: lmdb::Transaction>(
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
            .map_err(internal_storage)
    }

    /// Returns `true` if the given block's header is stored.
    fn block_header_exists<Tx: lmdb::Transaction>(
        &self,
        txn: &Tx,
        block_hash: &BlockHash,
    ) -> Result<bool, BlockStoreError> {
        self.block_header_dbs
            .exists(txn, block_hash)
            .map_err(internal_storage)
    }

    fn get_transfers<Tx: lmdb::Transaction>(
        &self,
        txn: &Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<Vec<Transfer>>, BlockStoreError> {
        Ok(self
            .transfer_dbs
            .get(txn, block_hash)
            .map_err(internal_storage)?
            .map(Transfers::into_owned))
    }

    fn load_state_store<K: AsRef<[u8]>, Tx: lmdb::Transaction>(
        &self,
        txn: &Tx,
        key: &K,
    ) -> Result<Option<Vec<u8>>, BlockStoreError> {
        match txn.get(self.state_store_db, key) {
            Ok(slice) => Ok(Some(slice.to_owned())),
            Err(lmdb::Error::NotFound) => Ok(None),
            Err(err) => Err(internal_storage(err)),
        }
    }

    /// Retrieves approvals hashes by block hash.
    fn read_approvals_hashes<Tx: lmdb::Transaction>(
        &self,
        txn: &Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<ApprovalsHashes>, BlockStoreError> {
        self.approvals_hashes_dbs
            .get(txn, block_hash)
            .map_err(internal_storage)
    }

    fn write_transfers(
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
            .map_err(internal_storage)
    }

    /// Retrieves a single block header in a given transaction from storage.
    fn get_single_block_header<Tx: LmdbTransaction>(
        &self,
        txn: &Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockHeader>, BlockStoreError> {
        let block_header = match self
            .block_header_dbs
            .get(txn, block_hash)
            .map_err(internal_storage)?
        {
            Some(block_header) => block_header,
            None => return Ok(None),
        };
        block_header.set_block_hash(*block_hash);
        Ok(Some(block_header))
    }

    /// Retrieves block signatures for a block with a given block hash.
    fn get_block_signatures<Tx: LmdbTransaction>(
        &self,
        txn: &Tx,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockSignatures>, BlockStoreError> {
        self.block_metadata_dbs
            .get(txn, block_hash)
            .map_err(internal_storage)
    }

    /// Retrieves a single block from storage.
    fn get_single_block<Tx: LmdbTransaction>(
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
            .map_err(internal_storage);
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
        let block =
            Block::new_from_header_and_body(block_header, block_body).map_err(internal_storage)?;
        Ok(Some(block))
    }

    /// Writes approvals hashes to storage.
    fn write_approvals_hashes(
        &self,
        txn: &mut RwTransaction,
        approvals_hashes: &ApprovalsHashes,
    ) -> Result<(), BlockStoreError> {
        self.approvals_hashes_dbs
            .put(txn, approvals_hashes.block_hash(), approvals_hashes, true)
            .map(|_| ())
            .map_err(internal_storage)
    }

    fn write_execution_results(
        &self,
        txn: &mut RwTransaction,
        block_info: BlockHashHeightAndEra,
        execution_results: &HashMap<TransactionHash, ExecutionResult>,
    ) -> Result<(), BlockStoreError> {
        let block_hash = &block_info.block_hash;
        self.write_transaction_index(txn, &block_info, execution_results.keys().copied())?;
        let mut transfers = Vec::new();
        for (&transaction_hash, execution_result) in execution_results {
            transfers.extend(successful_transfers(execution_result));

            let maybe_stored_execution_result = self.get_execution_result(txn, transaction_hash)?;

            // If we have a previous execution result, we can continue if it is the same.
            match maybe_stored_execution_result {
                Some(stored_execution_result) if &stored_execution_result == execution_result => {
                    continue
                }
                Some(_) | None => (),
            }

            let was_written = self
                .execution_result_dbs
                .put(txn, &transaction_hash, execution_result, true)
                .map_err(internal_storage)?;

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
            .map_err(internal_storage)?;
        if !was_written {
            error!(?block_hash, "failed to write transfers");
            debug_assert!(was_written);
        }
        Ok(())
    }

    fn get_transaction(
        &self,
        txn: &impl LmdbTransaction,
        hash: TransactionHash,
    ) -> Result<Option<Transaction>, BlockStoreError> {
        self.transaction_dbs
            .get(txn, &hash)
            .map_err(internal_storage)
    }

    fn get_finalized_approvals(
        &self,
        txn: &impl LmdbTransaction,
        hash: TransactionHash,
    ) -> Result<Option<BTreeSet<Approval>>, BlockStoreError> {
        self.finalized_transaction_approvals_dbs
            .get(txn, &hash)
            .map_err(internal_storage)
    }

    fn get_execution_result(
        &self,
        txn: &impl LmdbTransaction,
        hash: TransactionHash,
    ) -> Result<Option<ExecutionResult>, BlockStoreError> {
        self.execution_result_dbs
            .get(txn, &hash)
            .map_err(internal_storage)
    }

    fn indexed_hash(
        &self,
        txn: &impl LmdbTransaction,
        index: Database,
        key: Option<u64>,
    ) -> Result<Option<BlockHash>, BlockStoreError> {
        match key {
            Some(key) => get_by_be_u64_key(txn, index, key),
            None => get_last_by_be_u64_key(txn, index),
        }
        .map_err(internal_storage)
    }

    fn block_hash_at_height(
        &self,
        txn: &impl LmdbTransaction,
        height: u64,
    ) -> Result<Option<BlockHash>, BlockStoreError> {
        let Some(hash) = self.indexed_hash(txn, self.block_height_index_db, Some(height))? else {
            return Ok(None);
        };
        Ok(self.block_header_exists(txn, &hash)?.then_some(hash))
    }

    fn indexed_block(
        &self,
        txn: &impl LmdbTransaction,
        index: Database,
        key: Option<u64>,
    ) -> Result<Option<Block>, BlockStoreError> {
        match self.indexed_hash(txn, index, key)? {
            Some(hash) => self.get_single_block(txn, &hash),
            None => Ok(None),
        }
    }

    fn indexed_header(
        &self,
        txn: &impl LmdbTransaction,
        index: Database,
        key: Option<u64>,
    ) -> Result<Option<BlockHeader>, BlockStoreError> {
        match self.indexed_hash(txn, index, key)? {
            Some(hash) => self.get_single_block_header(txn, &hash),
            None => Ok(None),
        }
    }

    fn block_at_height(
        &self,
        txn: &impl LmdbTransaction,
        height: u64,
    ) -> Result<Option<Block>, BlockStoreError> {
        self.indexed_block(txn, self.block_height_index_db, Some(height))
    }

    fn header_at_height(
        &self,
        txn: &impl LmdbTransaction,
        height: u64,
    ) -> Result<Option<BlockHeader>, BlockStoreError> {
        self.indexed_header(txn, self.block_height_index_db, Some(height))
    }

    fn header_by_era(
        &self,
        txn: &impl LmdbTransaction,
        era_id: EraId,
    ) -> Result<Option<BlockHeader>, BlockStoreError> {
        self.indexed_header(txn, self.switch_block_era_id_index_db, Some(era_id.value()))
    }

    fn tip_block(&self, txn: &impl LmdbTransaction) -> Result<Option<Block>, BlockStoreError> {
        self.indexed_block(txn, self.block_height_index_db, None)
    }

    fn tip_header(
        &self,
        txn: &impl LmdbTransaction,
    ) -> Result<Option<BlockHeader>, BlockStoreError> {
        self.indexed_header(txn, self.block_height_index_db, None)
    }

    fn latest_switch_header(
        &self,
        txn: &impl LmdbTransaction,
    ) -> Result<Option<BlockHeader>, BlockStoreError> {
        self.indexed_header(txn, self.switch_block_era_id_index_db, None)
    }

    fn block_info_by_transaction(
        &self,
        txn: &impl LmdbTransaction,
        hash: TransactionHash,
    ) -> Result<Option<BlockHashHeightAndEra>, BlockStoreError> {
        txn.get_value_bytesrepr(self.transaction_hash_index_db, &hash)
            .map_err(internal_storage)
    }

    fn write_block(
        &self,
        txn: &mut RwTransaction<'_>,
        block: &Block,
    ) -> Result<BlockHash, BlockStoreError> {
        let hash = self.write_block_header(txn, &block.clone_header())?;
        let info = BlockHashHeightAndEra::new(hash, block.height(), block.era_id());
        self.write_transaction_index(txn, &info, block.all_transaction_hashes())?;
        self.block_body_dbs
            .put(txn, block.body_hash(), &block.clone_body(), true)
            .map_err(internal_storage)?;
        Ok(hash)
    }

    fn write_block_header(
        &self,
        txn: &mut RwTransaction<'_>,
        header: &BlockHeader,
    ) -> Result<BlockHash, BlockStoreError> {
        let hash = header.block_hash();
        let height = header.height();
        match self.indexed_hash(txn, self.block_height_index_db, Some(height))? {
            Some(first) if first != hash => {
                return Err(BlockStoreError::DuplicateBlock {
                    height,
                    first,
                    second: hash,
                });
            }
            Some(_) => {}
            None => {
                put_by_be_u64_key(txn, self.block_height_index_db, height, &hash)
                    .map_err(internal_storage)?;
            }
        }
        if header.is_switch_block() {
            let era_id = header.era_id();
            match self.indexed_hash(txn, self.switch_block_era_id_index_db, Some(era_id.value()))? {
                Some(first) if first != hash => {
                    return Err(BlockStoreError::DuplicateEraId {
                        era_id,
                        first,
                        second: hash,
                    });
                }
                Some(_) => {}
                None => {
                    put_by_be_u64_key(
                        txn,
                        self.switch_block_era_id_index_db,
                        era_id.value(),
                        &hash,
                    )
                    .map_err(internal_storage)?;
                }
            }
        }
        self.block_header_dbs
            .put(txn, &hash, header, true)
            .map_err(internal_storage)?;
        Ok(hash)
    }

    fn write_transaction_index(
        &self,
        txn: &mut RwTransaction<'_>,
        block_info: &BlockHashHeightAndEra,
        hashes: impl IntoIterator<Item = TransactionHash>,
    ) -> Result<(), BlockStoreError> {
        for hash in hashes {
            match self.block_info_by_transaction(txn, hash)? {
                Some(existing) if existing.block_hash != block_info.block_hash => {
                    return Err(BlockStoreError::DuplicateTransaction {
                        transaction_hash: hash,
                        first: existing.block_hash,
                        second: block_info.block_hash,
                    });
                }
                _ => {}
            }
            txn.put_value_bytesrepr(self.transaction_hash_index_db, &hash, block_info, true)
                .map_err(internal_storage)?;
        }
        Ok(())
    }

    fn delete_transaction_execution_records(
        &self,
        txn: &mut RwTransaction<'_>,
        hashes: impl IntoIterator<Item = TransactionHash>,
    ) -> Result<(), BlockStoreError> {
        for hash in hashes {
            self.execution_result_dbs
                .delete(txn, &hash)
                .map_err(internal_storage)?;
            delete_value_bytesrepr(txn, self.transaction_hash_index_db, &hash)
                .map_err(internal_storage)?;
        }
        Ok(())
    }

    fn delete_block_header_and_signatures(
        &self,
        txn: &mut RwTransaction<'_>,
        hash: &BlockHash,
    ) -> Result<(), BlockStoreError> {
        if let Some(header) = self.get_single_block_header(txn, hash)? {
            self.block_header_dbs
                .delete(txn, hash)
                .map_err(internal_storage)?;
            if header.is_switch_block() {
                delete_by_be_u64_key(
                    txn,
                    self.switch_block_era_id_index_db,
                    header.era_id().value(),
                )
                .map_err(internal_storage)?;
            }
            delete_by_be_u64_key(txn, self.block_height_index_db, header.height())
                .map_err(internal_storage)?;
        }
        self.delete_finality_signatures(txn, hash)
    }

    fn transaction_with_finalized_approvals(
        &self,
        txn: &impl LmdbTransaction,
        hash: TransactionHash,
    ) -> Result<Option<TransactionWithApprovals>, BlockStoreError> {
        let Some(transaction) = self.get_transaction(txn, hash)? else {
            return Ok(None);
        };
        Ok(Some((
            transaction,
            self.get_finalized_approvals(txn, hash)?,
        )))
    }

    fn transaction_matching_finalized_id(
        &self,
        txn: &impl LmdbTransaction,
        id: TransactionId,
    ) -> Result<Option<Transaction>, BlockStoreError> {
        Ok(self
            .transaction_with_finalized_approvals(txn, id.transaction_hash())?
            .map(apply_finalized_approvals)
            .filter(|transaction| transaction.compute_id() == id))
    }

    fn execution_info(
        &self,
        txn: &impl LmdbTransaction,
        hash: TransactionHash,
    ) -> Result<Option<ExecutionInfo>, BlockStoreError> {
        let Some(info) = self.block_info_by_transaction(txn, hash)? else {
            return Ok(None);
        };
        Ok(Some(ExecutionInfo {
            block_hash: info.block_hash,
            block_height: info.block_height,
            execution_result: self.get_execution_result(txn, hash)?,
        }))
    }

    fn block_with_signatures(
        &self,
        txn: &impl LmdbTransaction,
        block: Option<Block>,
    ) -> Result<Option<BlockWithOptionalSignatures>, BlockStoreError> {
        let Some(block) = block else {
            return Ok(None);
        };
        let signatures = self.get_block_signatures(txn, block.hash())?;
        Ok(Some((block, signatures)))
    }

    fn block_execution_results(
        &self,
        txn: &impl LmdbTransaction,
        block: &Block,
    ) -> Result<Option<Vec<(TransactionHash, ExecutionResult)>>, BlockStoreError> {
        let mut results = Vec::new();
        for hash in block.all_transaction_hashes() {
            let Some(result) = self.get_execution_result(txn, hash)? else {
                debug!(block_hash = %block.hash(), transaction_hash = %hash, "missing execution result");
                return Ok(None);
            };
            results.push((hash, result));
        }
        Ok(Some(results))
    }

    fn trusted_ancestors(
        &self,
        txn: &impl LmdbTransaction,
        trusted: &BlockHeader,
        available_blocks: AvailableBlockRange,
    ) -> Result<Option<Vec<BlockHeader>>, BlockStoreError> {
        let mut ancestors = Vec::new();
        if trusted.is_genesis() {
            return Ok(Some(ancestors));
        }
        let mut parent_hash = *trusted.parent_hash();
        loop {
            let Some(header) = self.get_single_block_header(txn, &parent_hash)? else {
                warn!(%parent_hash, "block header not found");
                return Ok(None);
            };
            if !available_blocks.contains(header.height()) {
                debug!(%parent_hash, "block header not complete");
                return Ok(None);
            }
            let reached_end = header.is_switch_block() || header.is_genesis();
            parent_hash = *header.parent_hash();
            ancestors.push(header);
            if reached_end {
                return Ok(Some(ancestors));
            }
        }
    }

    fn header_with_signatures(
        &self,
        txn: &impl LmdbTransaction,
        header: BlockHeader,
        chain_name_hash: ChainNameDigest,
    ) -> Result<BlockHeaderWithSignatures, BlockStoreError> {
        let hash = header.block_hash();
        let signatures = self
            .get_block_signatures(txn, &hash)?
            .unwrap_or_else(|| match &header {
                BlockHeader::V1(header) => BlockSignaturesV1::new(hash, header.era_id()).into(),
                BlockHeader::V2(header) => {
                    BlockSignaturesV2::new(hash, header.height(), header.era_id(), chain_name_hash)
                        .into()
                }
            });
        Ok(BlockHeaderWithSignatures::new(header, signatures))
    }
}

fn header_count<Tx: LmdbTransaction>(
    txn: &Tx,
    block_store: &LmdbBlockStore,
) -> Result<usize, BlockStoreError> {
    let current = txn
        .stat(block_store.block_header_dbs.current)
        .map_err(internal_storage)?
        .entries();
    let legacy = txn
        .stat(block_store.block_header_dbs.legacy)
        .map_err(internal_storage)?
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
        .set_max_readers(MAX_READERS)
        .set_max_dbs(MAX_DB_COUNT)
        .set_map_size(total_size)
        .open(&root.join(STORAGE_DB_FILENAME))
        .map_err(internal_storage)
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

fn internal_storage(error: impl std::error::Error + Send + Sync + 'static) -> BlockStoreError {
    BlockStoreError::InternalStorage(Box::new(error))
}

fn commit_error(error: lmdb::Error) -> BlockStoreError {
    internal_storage(LmdbExtError::from(error))
}

fn apply_finalized_approvals((transaction, approvals): TransactionWithApprovals) -> Transaction {
    match approvals {
        Some(approvals) => transaction.with_approvals(approvals),
        None => transaction,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use casper_types::{
        global_state::TrieMerkleProof, testing::TestRng, BlockHeaderV2, CLValue, EraEndV2, Key,
        ProtocolVersion, PublicKey, SecretKey, StoredValue, TestBlockBuilder, Timestamp,
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
    fn put_executed_block_commits_all_records_or_rolls_back() {
        fn approvals(block: &Block) -> ApprovalsHashes {
            let proof = TrieMerkleProof::new(
                Key::ChecksumRegistry,
                StoredValue::CLValue(CLValue::from_t(BTreeMap::<String, Digest>::new()).unwrap()),
                Default::default(),
            );
            ApprovalsHashes::new(*block.hash(), vec![], proof)
        }

        let mut store = LmdbBlockStore::new_temporary(64 * 1024 * 1024).unwrap();
        let mut rng = TestRng::new();
        let first: Block = TestBlockBuilder::new()
            .height(1)
            .switch_block(false)
            .build(&mut rng)
            .into();
        let second: Block = TestBlockBuilder::new()
            .height(2)
            .switch_block(false)
            .build(&mut rng)
            .into();
        let transaction_hash = TransactionHash::random(&mut rng);
        let result = ExecutionResult::random(&mut rng);
        let first_approvals = approvals(&first);
        let results = HashMap::from([(transaction_hash, result.clone())]);
        store
            .put_executed_block(&first, &first_approvals, results.clone())
            .unwrap();

        // The duplicate is detected while writing execution results, after
        // the second block and its approvals have been written to the transaction.
        assert!(matches!(
            store.put_executed_block(&second, &approvals(&second), results),
            Err(BlockStoreError::DuplicateTransaction { .. })
        ));

        let txn = store.env.begin_ro_txn().unwrap();
        assert_eq!(
            store.get_single_block(&txn, &*first.hash()).unwrap(),
            Some(first.clone())
        );
        assert_eq!(
            store.read_approvals_hashes(&txn, &*first.hash()).unwrap(),
            Some(first_approvals)
        );
        assert_eq!(
            store.get_execution_result(&txn, transaction_hash).unwrap(),
            Some(result)
        );
        assert!(store
            .get_single_block(&txn, &*second.hash())
            .unwrap()
            .is_none());
        assert!(store
            .read_approvals_hashes(&txn, &*second.hash())
            .unwrap()
            .is_none());
        assert_eq!(
            store.block_hash_at_height(&txn, second.height()).unwrap(),
            None
        );
    }

    #[test]
    fn supports_more_than_five_simultaneous_read_transactions() {
        const CONCURRENT_READERS: usize = 6;

        let tempdir = TempDir::new().expect("should create tempdir");
        let store =
            LmdbBlockStore::new(tempdir.path(), 64 * 1024 * 1024).expect("should create store");

        let read_transactions = (0..CONCURRENT_READERS)
            .map(|_| store.env.begin_ro_txn().expect("should checkout ro"))
            .collect::<Vec<_>>();

        assert_eq!(read_transactions.len(), CONCURRENT_READERS);
    }

    #[test]
    fn cloned_temporary_store_keeps_its_directory_alive() {
        let store = LmdbBlockStore::new_temporary(64 * 1024 * 1024).expect("should create store");
        let storage_path = store.root.clone();
        let cloned_store = store.clone();

        drop(store);

        assert!(storage_path.exists());
        cloned_store
            .env
            .begin_ro_txn()
            .expect("clone should retain the temporary directory");
    }

    #[test]
    fn approvals_hashes_existence_does_not_follow_header_existence() {
        let rng = &mut TestRng::new();
        let mut store =
            LmdbBlockStore::new_temporary(64 * 1024 * 1024).expect("should create store");

        let secret_key = SecretKey::random(rng);
        let proposer = PublicKey::from(&secret_key);
        let header = header_at_height(rng, 0, &proposer);
        let hash = store
            .put_block_header(&header)
            .expect("should write header");
        assert_eq!(store.read_block_header_by_hash(hash).unwrap(), Some(header));
        assert!(store.read_approvals_hashes_by_hash(hash).unwrap().is_none());
    }

    #[test]
    fn finds_first_missing_block_header_height() {
        let rng = &mut TestRng::new();
        let mut store =
            LmdbBlockStore::new_temporary(64 * 1024 * 1024).expect("should create store");
        let proposer = PublicKey::from(&SecretKey::random(rng));
        let headers = [10, 11, 13, 14].map(|height| header_at_height(rng, height, &proposer));
        store.put_block_headers(&headers).unwrap();

        for (range, expected) in [
            (10..12, None),
            (9..12, Some(9)),
            (10..14, Some(12)),
            (13..16, Some(15)),
            (10..10, None),
        ] {
            assert_eq!(
                store
                    .first_missing_block_header_height(range.clone())
                    .unwrap(),
                expected,
                "range {range:?}"
            );
        }
    }

    #[test]
    fn reindex_rebuilds_disk_backed_indexes_via_append() {
        let rng = &mut TestRng::new();
        let tempdir = TempDir::new().expect("should create tempdir");
        let mut store =
            LmdbBlockStore::new(tempdir.path(), 64 * 1024 * 1024).expect("should create store");

        let secret_key = SecretKey::random(rng);
        let proposer = PublicKey::from(&secret_key);

        let headers: Vec<_> = (0..HEADER_COUNT)
            .map(|height| header_at_height(rng, height, &proposer))
            .collect();
        store.put_block_headers(&headers).unwrap();

        store.reindex().expect("reindex should succeed");

        let ro_txn = store.env.begin_ro_txn().expect("should checkout ro");

        // Spot-check a handful of heights, including ones that require crossing the
        // little-endian single-byte boundary (e.g. 255 -> 256) to catch ordering bugs.
        for &height in &[0u64, 1, 254, 255, 256, 257, HEADER_COUNT - 1] {
            let expected_hash = headers[height as usize].block_hash();
            let actual = store
                .header_at_height(&ro_txn, height)
                .expect("read by height");
            assert_eq!(
                actual.expect("header should exist").block_hash(),
                expected_hash,
                "wrong header at height {height}"
            );
        }

        // Tip should be the highest height.
        let tip = store.tip_header(&ro_txn).expect("read tip");
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
            let actual = store
                .header_by_era(&ro_txn, era_id)
                .expect("read by era id");
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
        let latest_switch = store
            .latest_switch_header(&ro_txn)
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
        // transaction method. This simulates a migration from a binary version that didn't yet
        // maintain these disk-backed indexes.
        let header = header_at_height(rng, 0, &proposer);
        {
            let mut txn = store.env.begin_rw_txn().expect("should begin rw txn");
            store
                .block_header_dbs
                .put(&mut txn, &header.block_hash(), &header, true)
                .expect("should write header");
            txn.commit().expect("should commit");
        }

        // The index hasn't been told about this header yet.
        {
            let ro_txn = store.env.begin_ro_txn().expect("should checkout ro");
            let by_height = store.header_at_height(&ro_txn, 0).expect("read by height");
            assert!(by_height.is_none(), "index should not exist yet");
        }

        store.init().expect("init should succeed");

        let ro_txn = store.env.begin_ro_txn().expect("should checkout ro");
        let by_height = store.header_at_height(&ro_txn, 0).expect("read by height");
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
        store.put_block_headers(&headers).unwrap();

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
        let ro_txn = store.env.begin_ro_txn().expect("should checkout ro");
        let by_height_0 = store.header_at_height(&ro_txn, 0).expect("read by height");
        assert!(
            by_height_0.is_none(),
            "init should not have rebuilt an already-populated index"
        );
        let by_height_1 = store.header_at_height(&ro_txn, 1).expect("read by height");
        assert_eq!(
            by_height_1
                .expect("height 1 should still be indexed")
                .block_hash(),
            headers[1].block_hash()
        );
    }

    #[test]
    fn hard_reset_prunes_execution_records_without_a_block_body() {
        let mut rng = TestRng::new();
        let mut store = LmdbBlockStore::new_temporary(64 * 1024 * 1024).unwrap();
        let era = EraId::new(1);
        let height = 5;
        let transaction = Transaction::random(&mut rng);
        let transaction_hash = transaction.hash();
        let old_block = TestBlockBuilder::new()
            .era(era)
            .height(height)
            .switch_block(false)
            .protocol_version(ProtocolVersion::V1_0_0)
            .transactions([&transaction])
            .build_versioned(&mut rng);
        let old_hash = *old_block.hash();

        store.put_block_header(&old_block.clone_header()).unwrap();
        let result = ExecutionResult::random(&mut rng);
        let stored_block = store
            .put_execution_results(&BlockExecutionResults {
                block_info: BlockHashHeightAndEra::new(old_hash, height, era),
                exec_results: HashMap::from([(transaction_hash, result.clone())]),
            })
            .unwrap();
        assert!(stored_block.is_none(), "the block body must be missing");
        assert_eq!(
            store.read_execution_info(transaction_hash).unwrap(),
            Some(ExecutionInfo {
                block_hash: old_hash,
                block_height: height,
                execution_result: Some(result),
            })
        );

        store
            .prune_for_hard_reset(Some(era), ProtocolVersion::V2_0_0)
            .unwrap();
        assert!(
            store
                .read_execution_info(transaction_hash)
                .unwrap()
                .is_none(),
            "the pruned block's transaction index entry must be removed"
        );
        assert_eq!(store.read_execution_result(transaction_hash).unwrap(), None);
        assert_eq!(store.read_block_header_by_hash(old_hash).unwrap(), None);

        let replacement = TestBlockBuilder::new()
            .era(era)
            .height(height)
            .switch_block(false)
            .protocol_version(ProtocolVersion::V2_0_0)
            .transactions([&transaction])
            .build_versioned(&mut rng);
        assert_eq!(store.put_block(&replacement).unwrap(), *replacement.hash());
    }
}
