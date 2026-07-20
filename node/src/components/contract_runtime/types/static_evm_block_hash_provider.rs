use casper_executor_evm::{
    BlockHashProvider as EvmBlockHashProvider,
    BlockHashProviderResult as EvmBlockHashProviderResult,
};
use casper_types::BlockHash;
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct StaticEvmBlockHashProvider {
    block_hashes: BTreeMap<u64, BlockHash>,
}

impl StaticEvmBlockHashProvider {
    /// Ctor
    pub(crate) fn new(block_hashes: BTreeMap<u64, BlockHash>) -> Self {
        StaticEvmBlockHashProvider { block_hashes }
    }
}

impl EvmBlockHashProvider for StaticEvmBlockHashProvider {
    fn block_hash(&self, block_height: u64) -> EvmBlockHashProviderResult<Option<BlockHash>> {
        Ok(self.block_hashes.get(&block_height).copied())
    }
}
