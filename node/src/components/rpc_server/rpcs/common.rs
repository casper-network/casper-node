use std::convert::TryFrom;

use once_cell::sync::Lazy;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use casper_hashing::Digest;
use casper_json_rpc::{ErrorCodeT, ReservedErrorCode};
use casper_types::{bytesrepr::ToBytes, Key};

use super::{
    chain::{self, BlockIdentifier},
    state, Error, ReactorEventT, RpcRequest,
};
use crate::{
    effect::EffectBuilder,
    reactor::QueueKind,
    types::{json_compatibility::StoredValue, AvailableBlockRange, Block},
};

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

/// Runs a global state query and returns a tuple of the JSON-compatible stored value and merkle
/// proof of the value.
///
/// The proof is bytesrepr-encoded, and then hex-encoded.
///
/// On error, a `warp_json_rpc::Error` is returned suitable for sending as a JSON-RPC response.
pub(super) async fn run_query_and_encode<REv: ReactorEventT>(
    effect_builder: EffectBuilder<REv>,
    state_root_hash: Digest,
    base_key: Key,
    path: Vec<String>,
) -> Result<(StoredValue, String), Error> {
    let (value, proofs) = state::run_query(effect_builder, state_root_hash, base_key, path).await?;

    let value_compat = match StoredValue::try_from(value) {
        Ok(value_compat) => value_compat,
        Err(error) => {
            warn!(?error, "failed to encode stored value");
            return Err(Error::new(
                ReservedErrorCode::InternalError,
                format!("failed to encode stored value: {}", error),
            ));
        }
    };

    let encoded_proofs = match proofs.to_bytes() {
        Ok(bytes) => base16::encode_lower(&bytes),
        Err(error) => {
            warn!(?error, ?proofs, "failed to encode proof");
            return Err(Error::new(
                ReservedErrorCode::InternalError,
                format!("failed to encode proof: {}", error),
            ));
        }
    };

    Ok((value_compat, encoded_proofs))
}

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

/// Returns an `Error` which includes the height range of fully available blocks as the additional
/// `data` field.
pub(super) async fn missing_block_or_state_root_error<REv: ReactorEventT, E: ErrorCodeT>(
    effect_builder: EffectBuilder<REv>,
    error_code: E,
    error_message: String,
) -> Error {
    let available_block_range = effect_builder
        .make_request(
            |responder| RpcRequest::GetAvailableBlockRange { responder },
            QueueKind::Api,
        )
        .await;

    debug!(
        %available_block_range,
        "got request for non-existent data, will respond with msg: {}", error_message
    );

    let error_data = ErrorData::MissingBlockOrStateRoot {
        message: error_message,
        available_block_range,
    };

    Error::new(error_code, error_data)
}

pub(super) async fn get_block<REv: ReactorEventT>(
    maybe_id: Option<BlockIdentifier>,
    only_from_available_block_range: bool,
    effect_builder: EffectBuilder<REv>,
) -> Result<Block, Error> {
    chain::get_block_with_metadata(maybe_id, only_from_available_block_range, effect_builder)
        .await
        .map(|block_with_metadata| block_with_metadata.block)
}
