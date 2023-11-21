use once_cell::sync::Lazy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::rpcs::error::Error;
use casper_types::{
    binary_port::global_state::GlobalStateQueryResult, AvailableBlockRange, Block, BlockHeader, BlockSignatures,
    Digest, ExecutionInfo, FinalizedApprovals, SignedBlock, StoredValue, Transaction,
    TransactionHash,
};

use crate::NodeClient;

use super::{chain::BlockIdentifier, state::GlobalStateIdentifier};

pub(super) static MERKLE_PROOF: Lazy<String> = Lazy::new(|| {
    String::from(
        "01000000006ef2e0949ac76e55812421f755abe129b6244fe7168b77f47a72536147614625016ef2e0949ac76e\
        55812421f755abe129b6244fe7168b77f47a72536147614625000000003529cde5c621f857f75f3810611eb4af3\
        f998caaa9d4a3413cf799f99c67db0307010000006ef2e0949ac76e55812421f755abe129b6244fe7168b77f47a\
        7253614761462501010102000000006e06000000000074769d28aac597a36a03a932d4b43e4f10bf0403ee5c41d\
        d035102553f5773631200b9e173e8f05361b681513c14e25e3138639eb03232581db7557c9e8dbbc83ce9450022\
        6a9a7fe4f2b7b88d5103a4fc7400f02bf89c860c9ccdd56951a2afe9be0e0267006d820fb5676eb2960e15722f7\
        725f3f8f41030078f8b2e44bf0dc03f71b176d6e800dc5ae9805068c5be6da1a90b2528ee85db0609cc0fb4bd60\
        bbd559f497a98b67f500e1e3e846592f4918234647fca39830b7e1e6ad6f5b7a99b39af823d82ba1873d0000030\
        00000010186ff500f287e9b53f823ae1582b1fa429dfede28015125fd233a31ca04d5012002015cc42669a55467\
        a1fdf49750772bfc1aed59b9b085558eb81510e9b015a7c83b0301e3cf4a34b1db6bfa58808b686cb8fe21ebe0c\
        1bcbcee522649d2b135fe510fe3")
});

/// An enum to be used as the `data` field of a JSON-RPC error response.
#[derive(Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields, untagged)]
pub enum ErrorData {
    /// The requested block of state root hash is not available on this node.
    MissingBlockOrStateRoot {
        /// Additional info.
        message: String,
        /// The height range (inclusive) of fully available blocks.
        available_block_range: AvailableBlockRange,
    },
}

pub async fn get_signed_block(
    node_client: &dyn NodeClient,
    identifier: Option<BlockIdentifier>,
) -> Result<SignedBlock, Error> {
    let hash = match identifier {
        Some(BlockIdentifier::Hash(hash)) => hash,
        Some(BlockIdentifier::Height(height)) => node_client
            .read_block_hash_from_height(height)
            .await
            .map_err(|err| Error::NodeRequest("block hash from height", err))?
            .ok_or_else(|| Error::NoBlockAtHeight(height))?,
        None => *node_client
            .read_highest_completed_block_info()
            .await
            .map_err(|err| Error::NodeRequest("highest completed block", err))?
            .ok_or(Error::NoHighestBlock)?
            .block_hash(),
    };

    let should_return_block = node_client
        .does_exist_in_completed_blocks(hash)
        .await
        .map_err(|err| Error::NodeRequest("completed block existence", err))?;

    if !should_return_block {
        return Err(Error::NoBlockWithHash(hash));
    }

    let header = node_client
        .read_block_header(hash)
        .await
        .map_err(|err| Error::NodeRequest("block header", err))?
        .ok_or_else(|| Error::NoBlockWithHash(hash))?;
    let body = node_client
        .read_block_body(*header.body_hash())
        .await
        .map_err(|err| Error::NodeRequest("block body", err))?
        .ok_or_else(|| Error::NoBlockBodyWithHash(*header.body_hash()))?;
    let signatures = node_client
        .read_block_signatures(hash)
        .await
        .map_err(|err| Error::NodeRequest("block signatures", err))?
        .unwrap_or_else(|| BlockSignatures::new(hash, header.era_id()));
    let block = Block::new_from_header_and_body(header, body).unwrap();

    if signatures.is_verified().is_err() {
        return Err(Error::CouldNotVerifyBlock(hash));
    };

    Ok(SignedBlock::new(block, signatures))
}

pub async fn resolve_state_root_hash(
    node_client: &dyn NodeClient,
    identifier: Option<GlobalStateIdentifier>,
) -> Result<(Digest, Option<BlockHeader>), Error> {
    let hash = match identifier {
        None => *node_client
            .read_highest_completed_block_info()
            .await
            .map_err(|err| Error::NodeRequest("highest completed block", err))?
            .ok_or(Error::NoHighestBlock)?
            .block_hash(),
        Some(GlobalStateIdentifier::BlockHash(hash)) => hash,
        Some(GlobalStateIdentifier::BlockHeight(height)) => node_client
            .read_block_hash_from_height(height)
            .await
            .map_err(|err| Error::NodeRequest("block hash from height", err))?
            .ok_or_else(|| Error::NoBlockAtHeight(height))?,
        Some(GlobalStateIdentifier::StateRootHash(hash)) => return Ok((hash, None)),
    };
    let header = node_client
        .read_block_header(hash)
        .await
        .map_err(|err| Error::NodeRequest("block header", err))?
        .ok_or_else(|| Error::NoBlockWithHash(hash))?;

    Ok((*header.state_root_hash(), Some(header)))
}

pub async fn get_transaction_with_approvals(
    node_client: &dyn NodeClient,
    hash: TransactionHash,
) -> Result<(Transaction, Option<FinalizedApprovals>), Error> {
    let txn = node_client
        .read_transaction(hash)
        .await
        .map_err(|err| Error::NodeRequest("transaction", err))?
        .ok_or_else(|| Error::NoTransactionWithHash(hash))?;
    let approvals = node_client
        .read_finalized_approvals(hash)
        .await
        .map_err(|err| Error::NodeRequest("finalized approvals", err))?;
    Ok((txn, approvals))
}

pub async fn get_transaction_execution_info(
    node_client: &dyn NodeClient,
    hash: TransactionHash,
) -> Result<Option<ExecutionInfo>, Error> {
    let Some(block_hash_and_height) = node_client
        .read_transaction_block_info(hash)
        .await
        .map_err(|err| Error::NodeRequest("block of a transaction", err))?
        else { return Ok(None) };
    let execution_result = node_client
        .read_execution_result(hash)
        .await
        .map_err(|err| Error::NodeRequest("block execution result", err))?;

    Ok(Some(ExecutionInfo {
        block_hash: *block_hash_and_height.block_hash(),
        block_height: block_hash_and_height.block_height(),
        execution_result,
    }))
}

pub(super) fn handle_query_result(
    query_result: GlobalStateQueryResult,
) -> Result<SuccessfulQueryResult, Error> {
    match query_result {
        GlobalStateQueryResult::Success {
            value,
            merkle_proof,
        } => Ok(SuccessfulQueryResult {
            value,
            merkle_proof,
        }),
        GlobalStateQueryResult::ValueNotFound => Err(Error::GlobalStateEntryNotFound),
        GlobalStateQueryResult::RootNotFound => Err(Error::GlobalStateRootHashNotFound),
        GlobalStateQueryResult::Error(error) => {
            info!(?error, "query failed");
            Err(Error::GlobalStateQueryFailed(error))
        }
    }
}

#[derive(Debug)]
pub(super) struct SuccessfulQueryResult {
    pub value: StoredValue,
    pub merkle_proof: String,
}
