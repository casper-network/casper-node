use std::convert::TryFrom;

use jsonrpc_lite::JsonRpc;
use thiserror::Error;

use casper_execution_engine::{
    core, core::ValidationError, shared::stored_value::StoredValue,
    storage::trie::merkle_proof::TrieMerkleProof,
};
use casper_node::{crypto::hash::Digest, types::json_compatibility};
use casper_types::{bytesrepr, Key};

use crate::error::{Error, Result};

const GET_ITEM_RESULT_STORED_VALUE: &str = "stored_value";
const GET_ITEM_RESULT_MERKLE_PROOF: &str = "merkle_proof";

/// Error that can be returned by validate_response.
#[derive(Error, Debug)]
pub enum ValidateResponseError {
    /// Failed to marshall value
    #[error("Failed to marshall value {0}")]
    BytesRepr(bytesrepr::Error),

    /// [`validate_response`] failed to parse JSON
    #[error("validate_response failed to parse")]
    ValidateResponseFailedToParse,

    /// [`validate_response`] failed to validate
    #[error(transparent)]
    ValidationError(#[from] ValidationError),

    /// Serialized value not contained in proof
    #[error("serialized value not contained in proof")]
    SerializedValueNotContainedInProof,
}

impl From<bytesrepr::Error> for ValidateResponseError {
    fn from(e: bytesrepr::Error) -> Self {
        ValidateResponseError::BytesRepr(e)
    }
}

pub(crate) fn validate_response(
    response: &JsonRpc,
    state_root_hash: &Digest,
    key: &Key,
    path: &[String],
) -> Result<()> {
    let value = response
        .get_result()
        .ok_or_else(|| Error::InvalidRpcResponse(response.clone()))?;

    let object = value
        .as_object()
        .ok_or(ValidateResponseError::ValidateResponseFailedToParse)?;

    let proofs: Vec<TrieMerkleProof<Key, StoredValue>> = {
        let proof = object
            .get(GET_ITEM_RESULT_MERKLE_PROOF)
            .ok_or(ValidateResponseError::ValidateResponseFailedToParse)?;
        let proof_str = proof
            .as_str()
            .ok_or(ValidateResponseError::ValidateResponseFailedToParse)?;
        let proof_bytes = hex::decode(proof_str)
            .map_err(|_| ValidateResponseError::ValidateResponseFailedToParse)?;
        bytesrepr::deserialize(proof_bytes)?
    };

    let proof_value: &StoredValue = {
        let last_proof = proofs
            .last()
            .ok_or(ValidateResponseError::ValidateResponseFailedToParse)?;
        last_proof.value()
    };

    // Here we need to validate that JSON `stored_value` is contained in the proof.
    //
    // Possible to deserialize that field into a `StoredValue` and pass below to
    // `validate_query_proof` instead of using this approach?
    {
        let value: json_compatibility::StoredValue = {
            let value = object
                .get(GET_ITEM_RESULT_STORED_VALUE)
                .ok_or(ValidateResponseError::ValidateResponseFailedToParse)?;
            serde_json::from_value(value.to_owned())?
        };
        match json_compatibility::StoredValue::try_from(proof_value) {
            Ok(json_proof_value) if json_proof_value == value => (),
            _ => return Err(ValidateResponseError::SerializedValueNotContainedInProof.into()),
        }
    }

    Ok(core::validate_query_proof(
        &state_root_hash.to_owned().into(),
        &proofs,
        key,
        path,
        proof_value,
    )
    .map_err(ValidateResponseError::ValidationError)?)
}
