use std::collections::{btree_map::Entry, BTreeMap};

use casper_types::{BlockHash, BlockHashAndHeight, BlockHeader, EraId, TransactionHash};

use super::{FatalStorageError, Storage};

impl Storage {
    /// Inserts the relevant entries to the two indices.
    ///
    /// If a duplicate entry is encountered, neither index is updated and an error is returned.
    pub(crate) fn insert_to_block_header_indices(
        block_height_index: &mut BTreeMap<u64, BlockHash>,
        switch_block_era_id_index: &mut BTreeMap<EraId, BlockHash>,
        block_header: &BlockHeader,
    ) -> Result<(), FatalStorageError> {
        let block_hash = block_header.block_hash();
        if let Some(first) = block_height_index.get(&block_header.height()) {
            if *first != block_hash {
                return Err(FatalStorageError::DuplicateBlockIndex {
                    height: block_header.height(),
                    first: *first,
                    second: block_hash,
                });
            }
        }

        if block_header.is_switch_block() {
            match switch_block_era_id_index.entry(block_header.era_id()) {
                Entry::Vacant(entry) => {
                    let _ = entry.insert(block_hash);
                }
                Entry::Occupied(entry) => {
                    if *entry.get() != block_hash {
                        return Err(FatalStorageError::DuplicateEraIdIndex {
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

    /// Inserts the relevant entries to the index.
    ///
    /// If a duplicate entry is encountered, index is not updated and an error is returned.
    pub(crate) fn insert_to_transaction_index(
        transaction_hash_index: &mut BTreeMap<TransactionHash, BlockHashAndHeight>,
        block_hash: BlockHash,
        block_height: u64,
        transaction_hashes: Vec<TransactionHash>,
    ) -> Result<(), FatalStorageError> {
        if let Some(hash) = transaction_hashes.iter().find(|hash| {
            transaction_hash_index
                .get(hash)
                .map_or(false, |old_block_hash_and_height| {
                    *old_block_hash_and_height.block_hash() != block_hash
                })
        }) {
            return Err(FatalStorageError::DuplicateTransactionIndex {
                transaction_hash: *hash,
                first: transaction_hash_index[hash],
                second: BlockHashAndHeight::new(block_hash, block_height),
            });
        }

        for hash in transaction_hashes {
            transaction_hash_index.insert(hash, BlockHashAndHeight::new(block_hash, block_height));
        }

        Ok(())
    }
}
