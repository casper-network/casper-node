use std::{collections::HashMap, rc::Rc};

use lmdb::{Database, RwTransaction, Transaction};

use casper_types::{Block, BlockBody, BlockBodyV1, BlockV2, DeployHash, Digest, ExecutionResult};
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
    #[cfg_attr(doc, aquamarine::aquamarine)]
    /// ```mermaid
    /// flowchart TD
    ///     style Start fill:#66ccff,stroke:#333,stroke-width:4px
    ///     style End fill:#66ccff,stroke:#333,stroke-width:4px
    ///     style B fill:#00FF00,stroke:#333,stroke-width:4px
    ///     
    ///     Start --> A[Block needs to be stored]
    ///     A --> put_block_to_storage
    ///     put_block_to_storage --> StorageRequest::PutBlock
    ///     StorageRequest::PutBlock --> B["write_block<br>(current version)"]
    ///     B --> write_validated_block
    ///     write_validated_block --> C[convert into BlockBody]
    ///     C --> put_single_versioned_block_body
    ///     put_single_versioned_block_body --> write_block_header
    ///     write_block_header --> D[update indices]
    ///     D --> End
    /// ```
    pub fn write_block(&mut self, block: &BlockV2) -> Result<bool, FatalStorageError> {
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
    /// necessary. This function should only be used by components that deal with historical blocks,
    /// for example: `Fetcher`.
    ///
    /// Returns `Ok(true)` if the block has been successfully written, `Ok(false)` if a part of it
    /// couldn't be written because it already existed, and `Err(_)` if there was an error.
    #[cfg_attr(doc, aquamarine::aquamarine)]
    /// ```mermaid
    /// flowchart TD
    ///     style Start fill:#66ccff,stroke:#333,stroke-width:4px
    ///     style End fill:#66ccff,stroke:#333,stroke-width:4px
    ///     style write_versioned_block fill:#00FF00,stroke:#333,stroke-width:4px
    ///     
    ///     Start --> A[Block fetched]
    ///     A --> put_versioned_block_to_storage
    ///     put_versioned_block_to_storage --> StorageRequest::PutVersionedBlock
    ///     StorageRequest::PutVersionedBlock --> write_versioned_block
    ///     write_versioned_block --> write_validated_versioned_block
    ///     write_validated_versioned_block --> B{"is it a legacy block?<br>(V1)"}
    ///     B -->|Yes| put_single_legacy_block_body
    ///     B -->|No| put_single_versioned_block_body
    ///     put_single_legacy_block_body --> write_block_header
    ///     put_single_versioned_block_body --> write_block_header
    ///     write_block_header --> C[update indices]
    ///     C --> End
    /// ```
    pub fn write_versioned_block(&mut self, block: &Block) -> Result<bool, FatalStorageError> {
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
    #[cfg_attr(doc, aquamarine::aquamarine)]
    /// ```mermaid
    ///     flowchart TD
    ///     style Start fill:#66ccff,stroke:#333,stroke-width:4px
    ///     style End fill:#66ccff,stroke:#333,stroke-width:4px
    ///     style B fill:#00FF00,stroke:#333,stroke-width:4px
    ///     
    ///     Start --> A["Validated block needs to be stored<br>(might be coming from contract runtime)"]
    ///     A --> put_executed_block_to_storage
    ///     put_executed_block_to_storage --> StorageRequest::PutExecutedBlock
    ///     StorageRequest::PutExecutedBlock --> put_executed_block
    ///     put_executed_block --> B["write_validated_block<br>(current version)"]
    ///     B --> C[convert into BlockBody]
    ///     C --> put_single_versioned_block_body
    ///     put_single_versioned_block_body --> write_block_header
    ///     write_block_header --> D[update indices]
    ///     D --> End
    /// ```
    fn write_validated_block(
        &mut self,
        txn: &mut RwTransaction,
        block: &BlockV2,
    ) -> Result<bool, FatalStorageError> {
        {
            let block_body_hash = block.body_hash();
            let block_body = block.body();
            let versioned_block_body: BlockBody = block_body.into();
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
        block: &Block,
    ) -> Result<bool, FatalStorageError> {
        {
            let block_body_hash = block.header().body_hash();
            match block {
                Block::V1(v1) => {
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
                Block::V2(_) => {
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
        versioned_block_body: &BlockBody,
        db: Database,
    ) -> Result<bool, LmdbExtError> {
        debug_assert!(!matches!(versioned_block_body, BlockBody::V1(_)));
        txn.put_value(db, block_body_hash, versioned_block_body, true)
            .map_err(Into::into)
    }

    pub(crate) fn put_executed_block(
        &mut self,
        block: &BlockV2,
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
