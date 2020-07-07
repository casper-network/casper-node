#![allow(missing_docs)]

// modules
pub mod error;
pub mod global_state;
pub mod protocol_data;
pub mod protocol_data_store;
pub mod store;
pub mod transaction_source;
pub mod trie;
pub mod trie_store;

#[cfg(test)]
use lazy_static::lazy_static;

pub(crate) const GAUGE_METRIC_KEY: &str = "gauge";
const MAX_DBS: u32 = 2;

#[cfg(test)]
lazy_static! {
    // 50 MiB = 52428800 bytes
    // page size on x86_64 linux = 4096 bytes
    // 52428800 / 4096 = 12800
    static ref TEST_MAP_SIZE: usize = {
        let page_size = *crate::components::contract_runtime::shared::page_size::PAGE_SIZE;
        page_size * 12800
    };
}
