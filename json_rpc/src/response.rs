use std::borrow::Cow;

use serde::{
    de::{DeserializeOwned, Deserializer},
    Deserialize, Serialize,
};
use serde_json::Value;
use tracing::error;

use super::{Error, JSON_RPC_VERSION};

/// A JSON-RPC response.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields, untagged)]
pub enum Response {
    /// A successful RPC execution.
    Success {
        /// The JSON-RPC version field.
        #[serde(deserialize_with = "set_jsonrpc_field")]
        jsonrpc: Cow<'static, str>,
        /// The same ID as was passed in the corresponding request.
        id: Value,
        /// The successful result of executing the RPC.
        result: Value,
    },
    /// An RPC execution which failed.
    Failure {
        /// The JSON-RPC version field.
        #[serde(deserialize_with = "set_jsonrpc_field")]
        jsonrpc: Cow<'static, str>,
        /// The same ID as was passed in the corresponding request.
        id: Value,
        /// The error encountered while executing the RPC.
        error: Error,
    },
}

impl Response {
    /// Returns a new `Response::Success`.
    pub fn new_success(id: Value, result: Value) -> Self {
        Response::Success {
            jsonrpc: Cow::Borrowed(JSON_RPC_VERSION),
            id,
            result,
        }
    }

    /// Returns a new `Response::Failure`.
    pub fn new_failure(id: Value, error: Error) -> Self {
        Response::Failure {
            jsonrpc: Cow::Borrowed(JSON_RPC_VERSION),
            id,
            error,
        }
    }

    /// Returns `true` is this is a `Response::Success`.
    pub fn is_success(&self) -> bool {
        matches!(self, Response::Success { .. })
    }

    /// Returns `true` is this is a `Response::Failure`.
    pub fn is_failure(&self) -> bool {
        matches!(self, Response::Failure { .. })
    }

    /// Returns the "result" field, or `None` if this is a `Response::Failure`.
    pub fn raw_result(&self) -> Option<&Value> {
        match &self {
            Response::Success { result, .. } => Some(result),
            Response::Failure { .. } => None,
        }
    }

    /// Returns the "result" field parsed as `T`, or `None` if this is a `Response::Failure` or if
    /// parsing fails.
    pub fn result<T: DeserializeOwned>(&self) -> Option<T> {
        match &self {
            Response::Success { result, .. } => serde_json::from_value(result.clone())
                .map_err(|error| {
                    error!("failed to parse: {}", error);
                })
                .ok(),
            Response::Failure { .. } => None,
        }
    }

    /// Returns the "error" field or `None` if this is a `Response::Success`.
    pub fn error(&self) -> Option<&Error> {
        match &self {
            Response::Success { .. } => None,
            Response::Failure { error, .. } => Some(error),
        }
    }

    /// Returns the "id" field.
    pub fn id(&self) -> &Value {
        match &self {
            Response::Success { id, .. } | Response::Failure { id, .. } => id,
        }
    }
}

fn set_jsonrpc_field<'de, D: Deserializer<'de>>(
    _deserializer: D,
) -> Result<Cow<'static, str>, D::Error> {
    Ok(Cow::Borrowed(JSON_RPC_VERSION))
}
