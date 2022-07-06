use std::convert::TryFrom;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use warp::reject::{self, Rejection};

use crate::{
    error::{Error, ReservedErrorCode},
    rejections::MissingId,
    JSON_RPC_VERSION,
};

/// A JSON-RPC request, prior to validation of conformance to the JSON-RPC specification.
///
/// This overly-permissive type used in the warp filters allows for non-conformance to be detected
/// and a useful error message returned to the client, rather than a somewhat opaque HTTP 404
/// response.
#[derive(Eq, PartialEq, Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct UnvalidatedRequest {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

/// Errors are returned to the client as a JSON-RPC response and HTTP 200 (OK), whereas rejections
/// cause no JSON-RPC response to be sent, but an appropriate HTTP 4xx error will be returned.
#[derive(Debug)]
pub(crate) enum ErrorOrRejection {
    Error { id: Value, error: Error },
    Rejection(Rejection),
}

/// A request which has been validated as conforming to the JSON-RPC specification.
pub(crate) struct Request {
    pub id: Value,
    pub method: String,
    pub params: Option<Value>,
}

/// Returns `Ok` if `id` is a String, Null or a Number with no fractional part.
fn is_valid(id: &Value) -> Result<(), Error> {
    match id {
        Value::String(_) | Value::Null => (),
        Value::Number(number) => {
            if number.is_f64() {
                return Err(Error::new(
                    ReservedErrorCode::InvalidRequest,
                    "'id' must not contain fractional parts if it is a number",
                ));
            }
        }
        _ => {
            return Err(Error::new(
                ReservedErrorCode::InvalidRequest,
                "'id' should be a string or integer",
            ));
        }
    }
    Ok(())
}

impl TryFrom<UnvalidatedRequest> for Request {
    type Error = ErrorOrRejection;

    /// Returns `Ok` if the request is valid as per
    /// [the JSON-RPC specification](https://www.jsonrpc.org/specification#request_object).
    ///
    /// Returns an `Error` in the following cases:
    ///   * "jsonrpc" field is not "2.0"
    ///   * "id" field is not a String, valid Number or Null
    ///   * "id" field is a Number with fractional part
    ///
    /// Returns a `Rejection` if the "id" field is `None`.
    fn try_from(request: UnvalidatedRequest) -> Result<Self, Self::Error> {
        let id = match request.id {
            Some(id) => {
                is_valid(&id).map_err(|error| ErrorOrRejection::Error {
                    id: Value::Null,
                    error,
                })?;
                id
            }
            None => return Err(ErrorOrRejection::Rejection(reject::custom(MissingId))),
        };

        if request.jsonrpc != JSON_RPC_VERSION {
            let error = Error::new(
                ReservedErrorCode::InvalidRequest,
                format!("'jsonrpc' must be '2.0', but was '{}'", request.jsonrpc),
            );
            return Err(ErrorOrRejection::Error { id, error });
        }

        Ok(Request {
            id,
            method: request.method,
            params: request.params,
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn should_validate_using_valid_id() {
        fn run_test(id: Value) {
            let method = "a".to_string();
            let params = Some(Value::Array(vec![Value::Bool(true)]));

            let unvalidated = UnvalidatedRequest {
                jsonrpc: JSON_RPC_VERSION.to_string(),
                id: Some(id.clone()),
                method: method.clone(),
                params: params.clone(),
            };

            let request = Request::try_from(unvalidated).unwrap();
            assert_eq!(request.id, id);
            assert_eq!(request.method, method);
            assert_eq!(request.params, params);
        }

        run_test(Value::String("the id".to_string()));
        run_test(json!(1314));
        run_test(Value::Null);
    }

    #[test]
    fn should_fail_to_validate_id_with_wrong_type() {
        let request = UnvalidatedRequest {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            id: Some(Value::Bool(true)),
            method: "a".to_string(),
            params: None,
        };

        let error = match Request::try_from(request) {
            Err(ErrorOrRejection::Error {
                id: Value::Null,
                error,
            }) => error,
            _ => panic!("should be error"),
        };
        assert_eq!(
            error,
            Error::new(
                ReservedErrorCode::InvalidRequest,
                "'id' should be a string or integer"
            )
        );
    }

    #[test]
    fn should_fail_to_validate_id_with_fractional_part() {
        let request = UnvalidatedRequest {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            id: Some(json!(1.1)),
            method: "a".to_string(),
            params: None,
        };

        let error = match Request::try_from(request) {
            Err(ErrorOrRejection::Error {
                id: Value::Null,
                error,
            }) => error,
            _ => panic!("should be error"),
        };
        assert_eq!(
            error,
            Error::new(
                ReservedErrorCode::InvalidRequest,
                "'id' must not contain fractional parts if it is a number"
            )
        );
    }

    #[test]
    fn should_fail_to_validate_with_invalid_jsonrpc_field() {
        let request = UnvalidatedRequest {
            jsonrpc: "2.1".to_string(),
            id: Some(json!("a")),
            method: "a".to_string(),
            params: None,
        };

        let error = match Request::try_from(request) {
            Err(ErrorOrRejection::Error {
                id: Value::String(id),
                error,
            }) if id == "a" => error,
            _ => panic!("should be error"),
        };
        assert_eq!(
            error,
            Error::new(
                ReservedErrorCode::InvalidRequest,
                "'jsonrpc' must be '2.0', but was '2.1'"
            )
        );
    }
}
