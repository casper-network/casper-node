use once_cell::sync::Lazy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::rpcs::error::Error;
use casper_types::{
    account::AccountHash, AddressableEntity, AvailableBlockRange, Block, BlockHeader,
    BlockIdentifier, BlockSignatures, Digest, ExecutionInfo, FinalizedApprovals, Key, SignedBlock,
    StoredValue, Transaction, TransactionHash, URef, U512,
};

use crate::NodeClient;

use super::state::{GlobalStateIdentifier, PurseIdentifier};

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
    let available_block_range = node_client
        .read_available_block_range()
        .await
        .map_err(|err| Error::NodeRequest("available block range", err))?;
    let hash = match identifier {
        Some(BlockIdentifier::Hash(hash)) => hash,
        Some(BlockIdentifier::Height(height)) => node_client
            .read_block_hash_from_height(height)
            .await
            .map_err(|err| Error::NodeRequest("block hash from height", err))?
            .ok_or(Error::NoBlockAtHeight(height, available_block_range))?,
        None => *node_client
            .read_highest_completed_block_info()
            .await
            .map_err(|err| Error::NodeRequest("highest completed block", err))?
            .ok_or(Error::NoHighestBlock(available_block_range))?
            .block_hash(),
    };

    let should_return_block = node_client
        .does_exist_in_completed_blocks(hash)
        .await
        .map_err(|err| Error::NodeRequest("completed block existence", err))?;

    if !should_return_block {
        return Err(Error::NoBlockWithHash(hash, available_block_range));
    }

    let header = node_client
        .read_block_header(hash)
        .await
        .map_err(|err| Error::NodeRequest("block header", err))?
        .ok_or(Error::NoBlockWithHash(hash, available_block_range))?;

    let body = node_client
        .read_block_body(*header.body_hash())
        .await
        .map_err(|err| Error::NodeRequest("block body", err))?
        .ok_or_else(|| Error::NoBlockBodyWithHash(*header.body_hash(), available_block_range))?;
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
    let available_block_range = node_client
        .read_available_block_range()
        .await
        .map_err(|err| Error::NodeRequest("available block range", err))?;
    let hash = match identifier {
        None => *node_client
            .read_highest_completed_block_info()
            .await
            .map_err(|err| Error::NodeRequest("highest completed block", err))?
            .ok_or(Error::NoHighestBlock(available_block_range))?
            .block_hash(),
        Some(GlobalStateIdentifier::BlockHash(hash)) => hash,
        Some(GlobalStateIdentifier::BlockHeight(height)) => node_client
            .read_block_hash_from_height(height)
            .await
            .map_err(|err| Error::NodeRequest("block hash from height", err))?
            .ok_or(Error::NoBlockAtHeight(height, available_block_range))?,
        Some(GlobalStateIdentifier::StateRootHash(hash)) => return Ok((hash, None)),
    };
    let header = node_client
        .read_block_header(hash)
        .await
        .map_err(|err| Error::NodeRequest("block header", err))?
        .ok_or(Error::NoBlockWithHash(hash, available_block_range))?;

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
        .ok_or(Error::NoTransactionWithHash(hash))?;
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
    else {
        return Ok(None);
    };
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

pub async fn get_account(
    node_client: &dyn NodeClient,
    account_hash: AccountHash,
    state_root_hash: Digest,
) -> Result<AddressableEntity, Error> {
    let account_key = Key::Account(account_hash);
    let (value, _) = node_client
        .query_global_state(state_root_hash, account_key, vec![])
        .await
        .map_err(|err| Error::NodeRequest("account stored value", err))?
        .ok_or(Error::GlobalStateEntryNotFound)?
        .into_inner();

    match value {
        StoredValue::Account(account) => Ok(account.into()),
        StoredValue::CLValue(entity_key_as_clvalue) => {
            let key: Key = entity_key_as_clvalue
                .into_t()
                .map_err(|_| Error::InvalidAccountInfo)?;
            let (value, _) = node_client
                .query_global_state(state_root_hash, key, vec![])
                .await
                .map_err(|err| Error::NodeRequest("account owning a purse", err))?
                .ok_or(Error::GlobalStateEntryNotFound)?
                .into_inner();
            value
                .into_addressable_entity()
                .ok_or(Error::InvalidAccountInfo)
        }
        _ => Err(Error::InvalidAccountInfo),
    }
}

pub async fn get_main_purse(
    node_client: &dyn NodeClient,
    identifier: PurseIdentifier,
    state_root_hash: Digest,
) -> Result<URef, Error> {
    let account_hash = match identifier {
        PurseIdentifier::MainPurseUnderPublicKey(account_public_key) => {
            account_public_key.to_account_hash()
        }
        PurseIdentifier::MainPurseUnderAccountHash(account_hash) => account_hash,
        PurseIdentifier::PurseUref(purse_uref) => return Ok(purse_uref),
    };
    let account = get_account(node_client, account_hash, state_root_hash)
        .await
        .map_err(|_| Error::InvalidMainPurse)?;
    Ok(account.main_purse())
}

pub async fn get_balance(
    node_client: &dyn NodeClient,
    uref: URef,
    state_root_hash: Digest,
) -> Result<SuccessfulQueryResult<U512>, Error> {
    let key = Key::Balance(uref.addr());
    let (value, merkle_proof) = node_client
        .query_global_state(state_root_hash, key, vec![])
        .await
        .map_err(|err| Error::NodeRequest("balance by uref", err))?
        .ok_or(Error::GlobalStateEntryNotFound)?
        .into_inner();
    let value = value
        .into_cl_value()
        .ok_or(Error::InvalidPurseBalance)?
        .into_t()
        .map_err(|_| Error::InvalidPurseBalance)?;
    Ok(SuccessfulQueryResult {
        value,
        merkle_proof,
    })
}

#[derive(Debug)]
pub struct SuccessfulQueryResult<A> {
    pub value: A,
    pub merkle_proof: String,
}
