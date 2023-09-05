use std::collections::{btree_map::Entry, BTreeMap};

use casper_types::{BlockBodyV2, BlockHash, BlockHashAndHeight, BlockHeader, DeployHash, EraId};

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
    /// Inserts the relevant entries to the index.
    ///
    /// If a duplicate entry is encountered, index is not updated and an error is returned.
    pub(crate) fn insert_block_body_to_deploy_index<'a>(
        deploy_hash_index: &mut BTreeMap<DeployHash, BlockHashAndHeight>,
        block_hash: BlockHash,
        block_height: u64,
        deploy_hash_iter: impl Iterator<Item = &'a DeployHash> + Clone,
    ) -> Result<(), FatalStorageError> {
        if let Some(hash) = deploy_hash_iter.clone().find(|hash| {
            deploy_hash_index
                .get(hash)
                .map_or(false, |old_block_hash_and_height| {
                    *old_block_hash_and_height.block_hash() != block_hash
                })
        }) {
            return Err(FatalStorageError::DuplicateDeployIndex {
                deploy_hash: *hash,
                first: deploy_hash_index[hash],
                second: BlockHashAndHeight::new(block_hash, block_height),
            });
        }

        for hash in deploy_hash_iter {
            deploy_hash_index.insert(*hash, BlockHashAndHeight::new(block_hash, block_height));
        }

        Ok(())
    }

    /// Inserts the relevant entries to the index.
    ///
    /// If a duplicate entry is encountered, index is not updated and an error is returned.
    pub(crate) fn insert_block_body_v2_to_deploy_index(
        deploy_hash_index: &mut BTreeMap<DeployHash, BlockHashAndHeight>,
        block_hash: BlockHash,
        block_body: &BlockBodyV2,
        block_height: u64,
    ) -> Result<(), FatalStorageError> {
        if let Some(hash) = block_body.deploy_and_transfer_hashes().find(|hash| {
            deploy_hash_index
                .get(hash)
                .map_or(false, |old_block_hash_and_height| {
                    *old_block_hash_and_height.block_hash() != block_hash
                })
        }) {
            return Err(FatalStorageError::DuplicateDeployIndex {
                deploy_hash: *hash,
                first: deploy_hash_index[hash],
                second: BlockHashAndHeight::new(block_hash, block_height),
            });
        }

        for hash in block_body.deploy_and_transfer_hashes() {
            deploy_hash_index.insert(*hash, BlockHashAndHeight::new(block_hash, block_height));
        }

        Ok(())
    }
}
