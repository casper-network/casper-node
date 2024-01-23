use std::{borrow::Cow, collections::HashMap};

use casper_types::{
    execution::ExecutionResult, Block, BlockHash, BlockHeader, BlockSignatures, EraId,
    FinalitySignature, Transaction, TransactionHash, TransactionHeader, Transfer,
};

use casper_types::{FinalizedApprovals, TransactionWithFinalizedApprovals};

use super::types::ApprovalsHashes;

use super::{error::BlockStoreError, indices::IndexedBy};

/// A block store that supports read/write operations consistently.
pub trait BlockStoreProvider {
    type Reader<'a>: BlockStoreReader + BlockStoreTransaction
    where
        Self: 'a;
    type Writer<'a>: BlockStoreWriter + BlockStoreTransaction
    where
        Self: 'a;
    type ReaderWriter<'a>: BlockStoreReader + BlockStoreWriter + BlockStoreTransaction
    where
        Self: 'a;

    fn checkout_ro(&self) -> Result<Self::Reader<'_>, BlockStoreError>;
    fn checkout_w(&mut self) -> Result<Self::Writer<'_>, BlockStoreError>;
    fn checkout_rw(&mut self) -> Result<Self::ReaderWriter<'_>, BlockStoreError>;
}

/// A block store that supports indexed read operations by the specified key type.
pub trait IndexedBlockStoreProvider<Key, Val> {
    type IndexedBlockStoreReader<'a>: BlockStoreReader + IndexedBy<Key, Val> + BlockStoreTransaction
    where
        Self: 'a;

    fn checkout_ro_indexed(&self) -> Result<Self::IndexedBlockStoreReader<'_>, BlockStoreError>;
}

pub trait BlockStoreTransaction {
    /// Commit changes to the block store.
    fn commit(self) -> Result<(), BlockStoreError>;

    /// Roll back any temporary changes to the block store.
    fn rollback(self);
}

/// A Read transaction for the block store.
pub trait BlockStoreReader: BlockStoreTransaction {
    /// Reads from the state storage database.
    ///
    /// If key is non-empty, returns bytes from under the key. Otherwise returns `Ok(None)`.
    /// May also fail with storage errors.
    fn read_state_store<K: AsRef<[u8]>>(&self, key: &K)
        -> Result<Option<Vec<u8>>, BlockStoreError>;

    /// Reads a single [`Block`] from storage based on the provided [`BlockHash`].
    fn read_block(&mut self, block_hash: &BlockHash) -> Result<Option<Block>, BlockStoreError>;

    /// Reads a single [`BlockHeader`] from storage based on the provided [`BlockHash`].
    fn read_block_header(
        &mut self,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockHeader>, BlockStoreError>;

    /// Retrieves approvals hashes for a block with a given [`BlockHash`].
    fn read_approvals_hashes(
        &mut self,
        block_hash: &BlockHash,
    ) -> Result<Option<ApprovalsHashes>, BlockStoreError>;

    #[allow(clippy::type_complexity)]
    /// Retrieves execution results for a block with a given [`BlockHash`].
    fn read_execution_results(
        &mut self,
        block_hash: &BlockHash,
    ) -> Result<Option<Vec<(TransactionHash, TransactionHeader, ExecutionResult)>>, BlockStoreError>;

    /// Retrieves the block signatures for a block with a given [`BlockHash`].
    fn read_block_signatures(
        &mut self,
        block_hash: &BlockHash,
    ) -> Result<Option<BlockSignatures>, BlockStoreError>;

    /// Retrieves a single [`Transaction`] based on the provided [`TransactionHash`].
    fn read_transaction(
        &mut self,
        transaction_hash: &TransactionHash,
    ) -> Result<Option<Transaction>, BlockStoreError>;

    /// Retrieves successful transfers associated with block with a given [`BlockHash`].
    fn read_transfers(
        &mut self,
        block_hash: &BlockHash,
    ) -> Result<Option<Vec<Transfer>>, BlockStoreError>;

    /// Retrieves a single [`Transaction`] along with its finalized approvals based on the provided
    /// [`TransactionHash`].
    ///
    /// Like [`BlockStoreReader::read_transaction`] but also returns the finalized approvals.
    fn read_transaction_with_finalized_approvals(
        &mut self,
        transaction_hash: &TransactionHash,
    ) -> Result<Option<TransactionWithFinalizedApprovals>, BlockStoreError>;

    /// Retrieves the [`FinalizedApprovals`] for a given transaction.
    fn read_finalized_approvals(
        &mut self,
        transaction_hash: &TransactionHash,
    ) -> Result<Option<FinalizedApprovals>, BlockStoreError>;

    fn read_execution_result(
        &mut self,
        transaction_hash: &TransactionHash,
    ) -> Result<Option<ExecutionResult>, BlockStoreError>;

    /// Checks if a block with a given [`BlockHash`] exists in storage.
    fn block_exists(&mut self, block_hash: &BlockHash) -> Result<bool, BlockStoreError>;

    /// Checks if a transaction with a given [`TransactionHash`] exists in storage.
    fn transaction_exists(
        &mut self,
        transaction_hash: &TransactionHash,
    ) -> Result<bool, BlockStoreError>;
}

/// A ReadWrite transaction
pub trait BlockStoreWriter: BlockStoreTransaction {
    #[allow(clippy::ptr_arg)]
    /// Writes a key to the state storage database.
    fn write_state_store(
        &mut self,
        key: Cow<'static, [u8]>,
        data: &Vec<u8>,
    ) -> Result<(), BlockStoreError>;

    /// Writes a block to storage.
    ///
    /// Returns `Ok(true)` if the block has been successfully written, `Ok(false)` if a part of it
    /// couldn't be written because it already existed, and `Err(_)` if there was an error.
    fn write_block(&mut self, block: &Block) -> Result<bool, BlockStoreError>;

    /// Writes a block header to storage.
    fn write_block_header(&mut self, block_header: BlockHeader) -> Result<bool, BlockStoreError>;

    /// Writes approvals hashes to storage.
    fn write_approvals_hashes(
        &mut self,
        approvals_hashes: &ApprovalsHashes,
    ) -> Result<bool, BlockStoreError>;

    /// Writes execution results to storage.
    fn write_execution_results(
        &mut self,
        block_hash: &BlockHash,
        block_height: u64,
        era_id: EraId,
        execution_results: HashMap<TransactionHash, ExecutionResult>,
    ) -> Result<bool, BlockStoreError>;

    /// Writes multiple finality signatures that are associated with a single block to storage.
    fn write_block_signatures(
        &mut self,
        block_signatures: &BlockSignatures,
    ) -> Result<bool, BlockStoreError>;

    /// Writes a single transaction into storage.
    fn write_transaction(&mut self, transaction: &Transaction) -> Result<bool, BlockStoreError>;

    #[allow(clippy::ptr_arg)]
    /// Writes multiple transfers to storage.
    fn write_transfers(
        &mut self,
        block_hash: &BlockHash,
        transfers: &Vec<Transfer>,
    ) -> Result<bool, BlockStoreError>;

    /// Writes finalized approvals associated with a single transaction to storage.
    fn write_finalized_approvals(
        &mut self,
        transaction_hash: &TransactionHash,
        finalized_approvals: &FinalizedApprovals,
    ) -> Result<bool, BlockStoreError>;

    /// Writes a single finality signature to storage.
    fn write_finality_signature(
        &mut self,
        signature: Box<FinalitySignature>,
    ) -> Result<bool, BlockStoreError>;
}
