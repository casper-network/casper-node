use std::collections::BTreeSet;

use lmdb::{Database, DatabaseFlags, Environment, RwTransaction, Transaction};

use casper_hashing::Digest;
use casper_types::{EraId, ProtocolVersion};

use super::{
    lmdb_ext::{LmdbExtError, TransactionExt, WriteTransactionExt},
    BlockHeightIndexValue,
};
use crate::types::{BlockHash, BlockHashAndHeight, DeployHash};

/// The name of the block_height_index_db.
const BLOCK_HEIGHT_INDEX_DB_NAME: &str = "block_height_index";
/// The name of the switch_block_era_id_index_db.
const SWITCH_BLOCK_ERA_ID_INDEX_DB_NAME: &str = "switch_block_era_id_index";
/// The name of the body_hash_index_db.
const BODY_HASH_INDEX_DB_NAME: &str = "body_hash_index";
/// The name of the deploy_hash_index_db.
const DEPLOY_HASH_INDEX_DB_NAME: &str = "deploy_hash_index";

/// The key in the state store DB under which the storage component's lowest available block height
/// is persisted.
const KEY_LOWEST_AVAILABLE_BLOCK_HEIGHT: &str = "lowest_available_block_height";
/// The key in the state store DB under which the storage component's highest available block height
/// is persisted.
const KEY_HIGHEST_AVAILABLE_BLOCK_HEIGHT: &str = "highest_available_block_height";

/// The databases used to persist the various indices of the storage component.
#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub(super) struct IndexDbs {
    /// An index mapping block height (`u64`) to block ID, era ID and protocol version
    /// (`BlockHeightIndexValue`).
    block_height_index_db: Database,
    /// An index mapping era ID (`u64`) to switch block ID (`BlockHash`).
    switch_block_era_id_index_db: Database,
    /// An index mapping block v1 body hash (`Digest`) to set of block IDs and heights containing
    /// it (`BTreeSet<BlockHashAndHeight>`).
    v1_body_hash_index_db: Database,
    /// An index mapping deploy ID (`DeployHash`) to block ID and height containing it
    /// (`BlockHashAndHeight`).
    deploy_hash_index_db: Database,
    /// A general purpose database allowing components to persist arbitrary state.
    state_store_db: Database,
}

impl IndexDbs {
    pub(super) fn new(env: &Environment, state_store_db: Database) -> Result<Self, LmdbExtError> {
        let block_height_index_db =
            env.create_db(Some(BLOCK_HEIGHT_INDEX_DB_NAME), DatabaseFlags::empty())?;
        let switch_block_era_id_index_db = env.create_db(
            Some(SWITCH_BLOCK_ERA_ID_INDEX_DB_NAME),
            DatabaseFlags::empty(),
        )?;
        let v1_body_hash_index_db =
            env.create_db(Some(BODY_HASH_INDEX_DB_NAME), DatabaseFlags::empty())?;
        let deploy_hash_index_db =
            env.create_db(Some(DEPLOY_HASH_INDEX_DB_NAME), DatabaseFlags::empty())?;
        let index_dbs = IndexDbs {
            block_height_index_db,
            switch_block_era_id_index_db,
            v1_body_hash_index_db,
            deploy_hash_index_db,
            state_store_db,
        };
        Ok(index_dbs)
    }

    /// Puts the block hash, era ID and protocol version under the given block height to the
    /// `block_height_index_db`.
    pub(super) fn put_to_block_height_index(
        &self,
        txn: &mut RwTransaction,
        block_height: u64,
        block_hash: BlockHash,
        era_id: EraId,
        protocol_version: ProtocolVersion,
    ) -> Result<(), LmdbExtError> {
        let value = BlockHeightIndexValue {
            block_hash,
            era_id,
            protocol_version,
        };
        txn.put_value_bytesrepr(
            self.block_height_index_db,
            &index_key_from_u64(block_height),
            &value,
            true,
        )?;
        Ok(())
    }

    /// Gets the block hash, era ID and protocol version under the given block height from the
    /// `block_height_index_db`.
    pub(super) fn get_from_block_height_index<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        block_height: u64,
    ) -> Result<Option<BlockHeightIndexValue>, LmdbExtError> {
        txn.get_value_bytesrepr(
            self.block_height_index_db,
            &index_key_from_u64(block_height),
        )
    }

    /// Deletes the entry under the given block height from the `block_height_index_db`.
    pub(super) fn delete_from_block_height_index(
        &self,
        txn: &mut RwTransaction,
        block_height: u64,
    ) -> Result<(), LmdbExtError> {
        txn.delete(
            self.block_height_index_db,
            &index_key_from_u64(block_height),
        )
    }

    /// Returns `true` if the `block_height_index_db` has an entry under the given block height.
    pub(super) fn exists_in_block_height_index<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        block_height: u64,
    ) -> Result<bool, LmdbExtError> {
        match txn.get(
            self.block_height_index_db,
            &index_key_from_u64(block_height),
        ) {
            Ok(_raw) => Ok(true),
            Err(lmdb::Error::NotFound) => Ok(false),
            Err(err) => Err(err.into()),
        }
    }

    /// Put the block hash under the given era ID to the `switch_block_era_id_index_db`.
    pub(super) fn put_to_switch_block_era_id_index(
        &self,
        txn: &mut RwTransaction,
        era_id: EraId,
        block_hash: &BlockHash,
    ) -> Result<(), LmdbExtError> {
        txn.put_value_bytesrepr(
            self.switch_block_era_id_index_db,
            &index_key_from_u64(era_id.value()),
            block_hash,
            true,
        )?;
        Ok(())
    }

    /// Get the block hash under the given era ID from the `switch_block_era_id_index_db`.
    pub(super) fn get_from_switch_block_era_id_index<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        era_id: EraId,
    ) -> Result<Option<BlockHash>, LmdbExtError> {
        txn.get_value_bytesrepr(
            self.switch_block_era_id_index_db,
            &index_key_from_u64(era_id.value()),
        )
    }

    /// Deletes the entry under the given era ID from the `switch_block_era_id_index_db`.
    pub(super) fn delete_from_switch_block_era_id_index(
        &self,
        txn: &mut RwTransaction,
        era_id: EraId,
    ) -> Result<(), LmdbExtError> {
        txn.delete(
            self.switch_block_era_id_index_db,
            &index_key_from_u64(era_id.value()),
        )
    }

    /// Put the block hash and height under the given v1 block body hash to the
    /// `v1_body_hash_index_db`.
    pub(super) fn put_to_v1_body_hash_index(
        &self,
        txn: &mut RwTransaction,
        body_hash: &Digest,
        block_hash: BlockHash,
        block_height: u64,
    ) -> Result<(), LmdbExtError> {
        let mut all_block_hash_and_heights =
            match self.get_from_v1_body_hash_index(txn, body_hash)? {
                Some(existing) => existing,
                None => BTreeSet::new(),
            };
        all_block_hash_and_heights.insert(BlockHashAndHeight::new(block_hash, block_height));
        txn.put_value_bytesrepr(
            self.v1_body_hash_index_db,
            body_hash,
            &all_block_hash_and_heights,
            true,
        )?;
        Ok(())
    }

    /// Get the block hashes and heights under the given v1 block body hash from the
    /// `v1_body_hash_index_db`.
    pub(super) fn get_from_v1_body_hash_index<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        body_hash: &Digest,
    ) -> Result<Option<BTreeSet<BlockHashAndHeight>>, LmdbExtError> {
        txn.get_value_bytesrepr(self.v1_body_hash_index_db, body_hash)
    }

    /// Deletes the block hash and height under the given v1 block body hash of the
    /// `v1_body_hash_index_db`.
    ///
    /// If the set becomes empty, removes the entry entirely and returns `true`.  If the set is not
    /// empty, it is stored and `false` is returned.
    pub(super) fn delete_from_v1_body_hash_index(
        &self,
        txn: &mut RwTransaction,
        body_hash: &Digest,
        block_hash: BlockHash,
        block_height: u64,
    ) -> Result<bool, LmdbExtError> {
        let mut all_block_hash_and_heights =
            match self.get_from_v1_body_hash_index(txn, body_hash)? {
                Some(existing) => existing,
                None => BTreeSet::new(),
            };
        all_block_hash_and_heights.remove(&BlockHashAndHeight::new(block_hash, block_height));
        if all_block_hash_and_heights.is_empty() {
            txn.delete(self.v1_body_hash_index_db, body_hash)?;
            Ok(true)
        } else {
            txn.put_value_bytesrepr(
                self.v1_body_hash_index_db,
                body_hash,
                &all_block_hash_and_heights,
                true,
            )?;
            Ok(false)
        }
    }

    /// Put the block hash and height under the given deploy hash to the `deploy_hash_index_db`.
    pub(super) fn put_to_deploy_hash_index(
        &self,
        txn: &mut RwTransaction,
        deploy_hash: &DeployHash,
        block_hash: BlockHash,
        block_height: u64,
    ) -> Result<(), LmdbExtError> {
        txn.put_value_bytesrepr(
            self.deploy_hash_index_db,
            deploy_hash,
            &BlockHashAndHeight::new(block_hash, block_height),
            true,
        )?;
        Ok(())
    }

    /// Get the `BlockHashAndHeight` under the given deploy hash from the `deploy_hash_index_db`.
    pub(super) fn get_from_deploy_hash_index<Tx: Transaction>(
        &self,
        txn: &mut Tx,
        deploy_hash: &DeployHash,
    ) -> Result<Option<BlockHashAndHeight>, LmdbExtError> {
        txn.get_value_bytesrepr(self.deploy_hash_index_db, deploy_hash)
    }

    /// Delete the entry under the given deploy hash from the `deploy_hash_index_db`.
    pub(super) fn delete_from_deploy_hash_index(
        &self,
        txn: &mut RwTransaction,
        deploy_hash: &DeployHash,
    ) -> Result<(), LmdbExtError> {
        txn.delete(self.deploy_hash_index_db, deploy_hash)
    }

    /// Put the block height under `KEY_LOWEST_AVAILABLE_BLOCK_HEIGHT` to the `state_store_db`.
    pub(super) fn put_lowest_available_block_height(
        &self,
        txn: &mut RwTransaction,
        block_height: u64,
    ) -> Result<(), LmdbExtError> {
        txn.put_value_bytesrepr(
            self.state_store_db,
            &KEY_LOWEST_AVAILABLE_BLOCK_HEIGHT,
            &block_height,
            true,
        )?;
        Ok(())
    }

    /// Get the block height under `KEY_LOWEST_AVAILABLE_BLOCK_HEIGHT` from the `state_store_db`.
    pub(super) fn get_lowest_available_block_height<Tx: Transaction>(
        &self,
        txn: &mut Tx,
    ) -> Result<Option<u64>, LmdbExtError> {
        txn.get_value_bytesrepr(self.state_store_db, &KEY_LOWEST_AVAILABLE_BLOCK_HEIGHT)
    }

    /// Put the block height under `KEY_HIGHEST_AVAILABLE_BLOCK_HEIGHT` to the `state_store_db`.
    pub(super) fn put_highest_available_block_height(
        &self,
        txn: &mut RwTransaction,
        block_height: u64,
    ) -> Result<(), LmdbExtError> {
        txn.put_value_bytesrepr(
            self.state_store_db,
            &KEY_HIGHEST_AVAILABLE_BLOCK_HEIGHT,
            &block_height,
            true,
        )?;
        Ok(())
    }

    /// Get the block height under `KEY_HIGHEST_AVAILABLE_BLOCK_HEIGHT` from the `state_store_db`.
    pub(super) fn get_highest_available_block_height<Tx: Transaction>(
        &self,
        txn: &mut Tx,
    ) -> Result<Option<u64>, LmdbExtError> {
        txn.get_value_bytesrepr(self.state_store_db, &KEY_HIGHEST_AVAILABLE_BLOCK_HEIGHT)
    }

    /// Get the block height under `KEY_HIGHEST_AVAILABLE_BLOCK_HEIGHT` from the `state_store_db`.
    pub(super) fn get_highest_available_block_info<Tx: Transaction>(
        &self,
        txn: &mut Tx,
    ) -> Result<Option<BlockHeightIndexValue>, LmdbExtError> {
        match self.get_highest_available_block_height(txn)? {
            Some(height) => self.get_from_block_height_index(txn, height),
            None => Ok(None),
        }
    }

    /// Clears all index databases, and resets the highest and lowest block to 0.
    pub(super) fn clear_all(&self, txn: &mut RwTransaction) -> Result<(), LmdbExtError> {
        txn.clear_db(self.block_height_index_db)?;
        txn.clear_db(self.switch_block_era_id_index_db)?;
        txn.clear_db(self.v1_body_hash_index_db)?;
        txn.clear_db(self.deploy_hash_index_db)?;
        txn.delete(self.state_store_db, &KEY_LOWEST_AVAILABLE_BLOCK_HEIGHT)?;
        txn.delete(self.state_store_db, &KEY_HIGHEST_AVAILABLE_BLOCK_HEIGHT)
    }
}

/// We use big-endian encoding of the index keys so they result in a sorted collection as LMDB
/// orders its data as sorted byte strings.
///
/// We don't want to use the `DatabaseFlags::INTEGER_KEY` for these indices as LMDB encodes the keys
/// in native-endian format.  This doesn't lend itself to being able to use a copy of the DB file
/// across different machines on different architectures.
fn index_key_from_u64(key: u64) -> [u8; 8] {
    key.to_be_bytes()
}

#[cfg(test)]
pub(super) mod test_utils {
    use std::{collections::BTreeMap, convert::TryFrom};

    use lmdb::Cursor;

    use casper_types::bytesrepr;

    use super::*;

    fn u64_from_index_key(raw_key: &[u8]) -> u64 {
        assert_eq!(raw_key.len(), 8);
        let mut buffer = [0; 8];
        buffer.copy_from_slice(raw_key);
        u64::from_be_bytes(buffer)
    }

    #[derive(Default, PartialEq, Eq, Debug)]
    pub(in crate::components::storage) struct InMemIndices {
        block_height_index: BTreeMap<u64, BlockHeightIndexValue>,
        switch_block_era_id_index: BTreeMap<u64, BlockHash>,
        v1_body_hash_index: BTreeMap<Digest, BTreeSet<BlockHashAndHeight>>,
        deploy_hash_index: BTreeMap<DeployHash, BlockHashAndHeight>,
        lowest_available_block_height: u64,
        highest_available_block_height: u64,
    }

    #[cfg(test)]
    impl From<(IndexDbs, &Environment)> for InMemIndices {
        fn from((dbs, env): (IndexDbs, &Environment)) -> Self {
            let mut indices = InMemIndices::default();

            let mut ro_txn = env.begin_ro_txn().unwrap();
            {
                let mut block_height_cursor =
                    ro_txn.open_ro_cursor(dbs.block_height_index_db).unwrap();
                for (raw_key, raw_val) in block_height_cursor.iter() {
                    assert!(indices
                        .block_height_index
                        .insert(
                            u64_from_index_key(raw_key),
                            bytesrepr::deserialize_from_slice(raw_val).unwrap(),
                        )
                        .is_none());
                }

                let mut switch_block_era_id_cursor = ro_txn
                    .open_ro_cursor(dbs.switch_block_era_id_index_db)
                    .unwrap();
                for (raw_key, raw_val) in switch_block_era_id_cursor.iter() {
                    assert!(indices
                        .switch_block_era_id_index
                        .insert(
                            u64_from_index_key(raw_key),
                            bytesrepr::deserialize_from_slice(raw_val).unwrap(),
                        )
                        .is_none());
                }

                let mut v1_body_hash_cursor =
                    ro_txn.open_ro_cursor(dbs.v1_body_hash_index_db).unwrap();
                for (raw_key, raw_val) in v1_body_hash_cursor.iter() {
                    assert!(indices
                        .v1_body_hash_index
                        .insert(
                            Digest::try_from(raw_key).unwrap(),
                            bytesrepr::deserialize_from_slice(raw_val).unwrap(),
                        )
                        .is_none());
                }

                let mut deploy_hash_cursor =
                    ro_txn.open_ro_cursor(dbs.deploy_hash_index_db).unwrap();
                for (raw_key, raw_val) in deploy_hash_cursor.iter() {
                    assert!(indices
                        .deploy_hash_index
                        .insert(
                            DeployHash::new(Digest::try_from(raw_key).unwrap()),
                            bytesrepr::deserialize_from_slice(raw_val).unwrap(),
                        )
                        .is_none());
                }
            }

            indices.lowest_available_block_height = dbs
                .get_lowest_available_block_height(&mut ro_txn)
                .expect("should be ok")
                .expect("should be some");
            indices.highest_available_block_height = dbs
                .get_highest_available_block_height(&mut ro_txn)
                .expect("should be ok")
                .expect("should be some");

            assert_eq!(
                *indices.block_height_index.keys().last().unwrap(),
                indices.highest_available_block_height
            );

            indices
        }
    }
}
