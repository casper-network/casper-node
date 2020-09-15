use std::str;

use futures::{future::BoxFuture, FutureExt};
use http::Response;
use hyper::Body;
use semver::Version;
use serde::{Deserialize, Serialize};
use tracing::info;
use warp_json_rpc::Builder;

use casper_execution_engine::{
    core::engine_state::{self, QueryResult},
    shared::stored_value::StoredValue,
};
use casper_types::{bytesrepr::ToBytes, CLValue, Key};

use super::{ApiRequest, Error, ErrorCode, ReactorEventT, RpcWithParams};
use crate::{
    components::api_server::CLIENT_API_VERSION,
    crypto::hash::Digest,
    effect::EffectBuilder,
    reactor::QueueKind,
    types::{json_compatibility::Account, BlockHash},
};

#[derive(Deserialize)]
pub(in crate::components::api_server) struct GetItemParams {
    /// The block to query, or use the latest block if `None`
    block_hash: Option<String>,
    /// Hex-encoded `casper_types::Key`.
    key: String,
    /// The path components starting from the key as base.
    path: Vec<String>,
}

impl GetItemParams {
    fn get_block_hash(&self) -> Result<Option<BlockHash>, String> {
        match self.block_hash.as_ref() {
            Some(hex_hash) => Digest::from_hex(hex_hash)
                .map_err(|error| format!("failed to parse block hash: {}", error))
                .map(|digest| Some(BlockHash::new(digest))),
            None => Ok(None),
        }
    }
}

pub(in crate::components::api_server) struct GetItem {}

enum CLValueOrAccount {
    CLValue(CLValue),
    Account(Account),
}

impl GetItem {
    /// Returns the `CLValue` or `Account` contained in the result, serialized and hex-encoded, or
    /// else returns an error with the error code and message which should be returned to the
    /// client.
    fn cl_value_or_account_from_result(
        result: Option<Result<QueryResult, engine_state::Error>>,
    ) -> Result<CLValueOrAccount, (i64, String)> {
        match result {
            Some(Ok(QueryResult::Success(StoredValue::CLValue(cl_value)))) => {
                Ok(CLValueOrAccount::CLValue(cl_value))
            }
            Some(Ok(QueryResult::Success(StoredValue::Account(ee_account)))) => {
                Ok(CLValueOrAccount::Account(Account::from(&ee_account)))
            }
            Some(Ok(QueryResult::Success(StoredValue::Contract(_)))) => Err((
                ErrorCode::QueryYieldedContract as i64,
                String::from("state query yielded contract, not cl_value"),
            )),
            Some(Ok(QueryResult::Success(StoredValue::ContractPackage(_)))) => Err((
                ErrorCode::QueryYieldedContractPackage as i64,
                String::from("state query yielded contract package, not cl_value"),
            )),
            Some(Ok(QueryResult::Success(StoredValue::ContractWasm(_)))) => Err((
                ErrorCode::QueryYieldedContractWasm as i64,
                String::from("state query yielded contract wasm, not cl_value"),
            )),
            Some(Ok(query_result)) => Err((
                ErrorCode::QueryFailed as i64,
                format!("state query failed: {:?}", query_result),
            )),
            Some(Err(error)) => Err((
                ErrorCode::QueryFailedToExecute as i64,
                format!("state query failed to execute: {}", error),
            )),
            None => Err((
                ErrorCode::NoSuchBlock as i64,
                String::from("block not known for state query"),
            )),
        }
    }
}

impl RpcWithParams for GetItem {
    const METHOD: &'static str = "state_get_item";

    type RequestParams = GetItemParams;

    fn handle_request<REv: ReactorEventT>(
        effect_builder: EffectBuilder<REv>,
        response_builder: Builder,
        params: Self::RequestParams,
    ) -> BoxFuture<'static, Result<Response<Body>, Error>> {
        /// The JSON-RPC response's "result" if the queried value is a `CLValue`.
        #[derive(Serialize)]
        struct CLValueResponseResult {
            api_version: Version,
            /// Hex-encoded, serialized CLValue.
            cl_value: String,
        }

        /// The JSON-RPC response's "result" if the queried value is an `Account`.
        #[derive(Serialize)]
        struct AccountResponseResult {
            api_version: Version,
            account: Account,
        }

        async move {
            // Try to parse a block hash from the params.
            let maybe_hash = match params.get_block_hash() {
                Ok(maybe_hash) => maybe_hash,
                Err(error_msg) => {
                    info!("{}", error_msg);
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::ParseBlockHash as i64,
                        error_msg,
                    ))?);
                }
            };

            // Try to parse a `casper_types::Key` from the params.
            let base_key = match Key::from_formatted_str(&params.key)
                .map_err(|error| format!("failed to parse key: {:?}", error))
            {
                Ok(key) => key,
                Err(error_msg) => {
                    info!("{}", error_msg);
                    return Ok(response_builder.error(warp_json_rpc::Error::custom(
                        ErrorCode::ParseQueryKey as i64,
                        error_msg,
                    ))?);
                }
            };

            // Run the query.
            let maybe_query_result = effect_builder
                .make_request(
                    |responder| ApiRequest::QueryGlobalState {
                        maybe_hash,
                        base_key,
                        path: params.path,
                        responder,
                    },
                    QueueKind::Api,
                )
                .await;

            // Extract the `CLValue` or `Account` from the result.
            let cl_value_or_account =
                match Self::cl_value_or_account_from_result(maybe_query_result) {
                    Ok(cl_value_or_account) => cl_value_or_account,
                    Err((code, error_msg)) => {
                        info!("{}", error_msg);
                        return Ok(response_builder
                            .error(warp_json_rpc::Error::custom(code, error_msg))?);
                    }
                };

            // Return the result.
            match cl_value_or_account {
                CLValueOrAccount::CLValue(cl_value) => match cl_value.to_bytes() {
                    Ok(serialized_cl_value) => {
                        let result = CLValueResponseResult {
                            api_version: CLIENT_API_VERSION.clone(),
                            cl_value: hex::encode(&serialized_cl_value),
                        };
                        Ok(response_builder.success(result)?)
                    }
                    Err(error) => {
                        info!("failed to serialize cl_value: {}", error);
                        return Ok(response_builder.error(warp_json_rpc::Error::INTERNAL_ERROR)?);
                    }
                },
                CLValueOrAccount::Account(account) => {
                    let result = AccountResponseResult {
                        api_version: CLIENT_API_VERSION.clone(),
                        account,
                    };
                    Ok(response_builder.success(result)?)
                }
            }
        }
        .boxed()
    }
}
