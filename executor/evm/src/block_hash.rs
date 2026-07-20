//! Block hash provider abstractions for the EVM `BLOCKHASH` opcode.

use std::sync::Arc;

use casper_storage::block_store::{
    lmdb::LmdbBlockStore, types::BlockHeight, BlockStoreError, BlockStoreProvider, DataReader,
};
use casper_types::{BlockHash, BlockHeader};

/// Result type returned by block hash providers.
pub type BlockHashProviderResult<T> = core::result::Result<T, BlockHashProviderError>;

/// Errors returned while resolving historical block hashes.
#[derive(Debug, thiserror::Error)]
pub enum BlockHashProviderError {
    /// Failed to read from Casper block storage.
    #[error(transparent)]
    BlockStore(#[from] BlockStoreError),
}

/// Resolves Casper block hashes by height for the EVM `BLOCKHASH` opcode.
///
/// The executor applies the EVM availability rules around the provider: current
/// and future block numbers, and block numbers older than 256 blocks, return
/// the zero hash. Providers only need to answer canonical historical heights.
pub trait BlockHashProvider {
    /// Returns the block hash for `block_height`, or `None` when unavailable.
    fn block_hash(&self, block_height: u64) -> BlockHashProviderResult<Option<BlockHash>>;
}

/// Block hash provider that returns no historical hashes.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoBlockHashProvider;

impl BlockHashProvider for NoBlockHashProvider {
    fn block_hash(&self, _block_height: u64) -> BlockHashProviderResult<Option<BlockHash>> {
        Ok(None)
    }
}

/// Block hash provider backed by Casper's indexed LMDB block store.
#[derive(Clone, Debug)]
pub struct IndexedLmdbBlockHashProvider {
    block_store: Arc<LmdbBlockStore>,
}

impl IndexedLmdbBlockHashProvider {
    /// Creates a block hash provider backed by `block_store`.
    pub fn new(block_store: Arc<LmdbBlockStore>) -> Self {
        Self { block_store }
    }
}

impl BlockHashProvider for IndexedLmdbBlockHashProvider {
    fn block_hash(&self, block_height: u64) -> BlockHashProviderResult<Option<BlockHash>> {
        let txn = self.block_store.checkout_ro()?;
        let maybe_header: Option<BlockHeader> =
            DataReader::<BlockHeight, BlockHeader>::read(&txn, block_height)?;
        Ok(maybe_header.map(|header| header.block_hash()))
    }
}
