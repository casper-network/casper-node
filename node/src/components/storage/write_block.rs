use std::{collections::HashMap, rc::Rc};

use lmdb::{Database, RwTransaction, Transaction};

use casper_types::{
    Block, BlockBodyV1, DeployHash, Digest, ExecutionResult, VersionedBlock, VersionedBlockBody,
};
use tracing::error;

use crate::types::ApprovalsHashes;

use super::{
    lmdb_ext::{LmdbExtError, WriteTransactionExt},
    FatalStorageError, Storage,
};

impl Storage {
    /// Verifies a block and writes it to a block to storage, updating indices as necessary.
    ///
    /// Returns `Ok(true)` if the block has been successfully written, `Ok(false)` if a part of it
    /// couldn't be written because it already existed, and `Err(_)` if there was an error.
    pub fn write_block(&mut self, block: &Block) -> Result<bool, FatalStorageError> {
        block.verify()?;
        let env = Rc::clone(&self.env);
        let mut txn = env.begin_rw_txn()?;
        let wrote = self.write_validated_block(&mut txn, block)?;
        if wrote {
            txn.commit()?;
        }
        Ok(wrote)
    }

    /// Verifies a versioned block and writes it to a block to storage, updating indices as
    /// necessary.
    ///
    /// Returns `Ok(true)` if the block has been successfully written, `Ok(false)` if a part of it
    /// couldn't be written because it already existed, and `Err(_)` if there was an error.
    pub fn write_versioned_block(
        &mut self,
        block: &VersionedBlock,
    ) -> Result<bool, FatalStorageError> {
        block.verify()?;
        let env = Rc::clone(&self.env);
        let mut txn = env.begin_rw_txn()?;
        let wrote = self.write_validated_versioned_block(&mut txn, block)?;
        if wrote {
            txn.commit()?;
        }
        Ok(wrote)
    }

    /// Writes a block which has already been verified to storage, updating indices as necessary.
    ///
    /// Returns `Ok(true)` if the block has been successfully written, `Ok(false)` if a part of it
    /// couldn't be written because it already existed, and `Err(_)` if there was an error.
    fn write_validated_block(
        &mut self,
        txn: &mut RwTransaction,
        block: &Block,
    ) -> Result<bool, FatalStorageError> {
        {
            let block_body_hash = block.body_hash();
            let block_body = block.body();
            let versioned_block_body: VersionedBlockBody = block_body.into();
            if !Self::put_single_versioned_block_body(
                txn,
                block_body_hash,
                &versioned_block_body,
                self.block_body_dbs.current,
            )? {
                error!("could not insert body for: {}", block);
                return Ok(false);
            }
        }

        let overwrite = true;

        if !txn.put_value(
            self.block_header_db,
            block.hash(),
            block.header(),
            overwrite,
        )? {
            error!("could not insert block header for block: {}", block);
            return Ok(false);
        }

        {
            Self::insert_to_block_header_indices(
                &mut self.block_height_index,
                &mut self.switch_block_era_id_index,
                block.header(),
            )?;
            Self::insert_block_body_to_deploy_index(
                &mut self.deploy_hash_index,
                *block.hash(),
                block.body(),
                block.height(),
            )?;
        }
        Ok(true)
    }

    // TODO[RC]: Dedup with the above, generic?
    fn write_validated_versioned_block(
        &mut self,
        txn: &mut RwTransaction,
        block: &VersionedBlock,
    ) -> Result<bool, FatalStorageError> {
        {
            let block_body_hash = block.header().body_hash();
            match block {
                VersionedBlock::V1(v1) => {
                    let block_body = v1.body();
                    if !Self::put_single_legacy_block_body(
                        txn,
                        block_body_hash,
                        block_body,
                        self.block_body_dbs.legacy,
                    )? {
                        error!("could not insert body for: {}", block);
                        return Ok(false);
                    }
                }
                VersionedBlock::V2(_) => {
                    let block_body = block.body();
                    if !Self::put_single_versioned_block_body(
                        txn,
                        block_body_hash,
                        &block_body,
                        self.block_body_dbs.current,
                    )? {
                        error!("could not insert body for: {}", block);
                        return Ok(false);
                    }
                }
            }
        }

        let overwrite = true;

        if !txn.put_value(
            self.block_header_db,
            block.hash(),
            block.header(),
            overwrite,
        )? {
            error!("could not insert block header for block: {}", block);
            return Ok(false);
        }

        {
            Self::insert_to_block_header_indices(
                &mut self.block_height_index,
                &mut self.switch_block_era_id_index,
                block.header(),
            )?;
            Self::insert_versioned_block_body_to_deploy_index(
                &mut self.deploy_hash_index,
                *block.hash(),
                &block.body(),
                block.header().height(),
            )?;
        }
        Ok(true)
    }

    /// Writes a single block body in a separate transaction to storage.
    fn put_single_legacy_block_body(
        txn: &mut RwTransaction,
        block_body_hash: &Digest,
        block_body: &BlockBodyV1,
        db: Database,
    ) -> Result<bool, LmdbExtError> {
        txn.put_value(db, block_body_hash, block_body, true)
            .map_err(Into::into)
    }

    /// Writes a single block body in a separate transaction to storage.
    fn put_single_versioned_block_body(
        txn: &mut RwTransaction,
        block_body_hash: &Digest,
        versioned_block_body: &VersionedBlockBody,
        db: Database,
    ) -> Result<bool, LmdbExtError> {
        debug_assert!(!matches!(versioned_block_body, VersionedBlockBody::V1(_)));
        txn.put_value(db, block_body_hash, versioned_block_body, true)
            .map_err(Into::into)
    }

    pub(crate) fn put_executed_block(
        &mut self,
        block: &Block,
        approvals_hashes: &ApprovalsHashes,
        execution_results: HashMap<DeployHash, ExecutionResult>,
    ) -> Result<bool, FatalStorageError> {
        let env = Rc::clone(&self.env);
        let mut txn = env.begin_rw_txn()?;
        let wrote = self.write_validated_block(&mut txn, block)?;
        if !wrote {
            return Err(FatalStorageError::FailedToOverwriteBlock);
        }

        let _ = self.write_approvals_hashes(&mut txn, approvals_hashes)?;
        let _ = self.write_execution_results(&mut txn, block.hash(), execution_results)?;
        txn.commit()?;

        Ok(true)
    }
}
