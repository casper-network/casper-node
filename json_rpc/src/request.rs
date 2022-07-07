mod params;

use itertools::Itertools;
use serde_json::{Map, Value};
use warp::reject::{self, Rejection};

use crate::{
    error::{Error, ReservedErrorCode},
    rejections::MissingId,
    JSON_RPC_VERSION,
};
pub use params::Params;

const JSONRPC_FIELD_NAME: &str = "jsonrpc";
const METHOD_FIELD_NAME: &str = "method";
const PARAMS_FIELD_NAME: &str = "params";
const ID_FIELD_NAME: &str = "id";

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
    pub params: Option<Params>,
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

impl Request {
    /// Returns `Ok` if the request is valid as per
    /// [the JSON-RPC specification](https://www.jsonrpc.org/specification#request_object).
    ///
    /// Returns an `Error` in any of the following cases:
    ///   * "jsonrpc" field is not "2.0"
    ///   * "method" field is not a String
    ///   * "params" field is present, but is not an Array or Object
    ///   * "id" field is not a String, valid Number or Null
    ///   * "id" field is a Number with fractional part
    ///   * `allow_unknown_fields` is `false` and extra fields exist
    ///
    /// Returns a `Rejection` if the "id" field is `None`.
    pub(super) fn new(
        mut request: Map<String, Value>,
        allow_unknown_fields: bool,
    ) -> Result<Self, ErrorOrRejection> {
        // Just copy "id" field for now to return verbatim in any errors before we get to actually
        // validating the "id" field itself.
        let id = request.get(ID_FIELD_NAME).cloned().unwrap_or_default();

        match request.remove(JSONRPC_FIELD_NAME) {
            Some(Value::String(jsonrpc)) => {
                if jsonrpc != JSON_RPC_VERSION {
                    let error = Error::new(
                        ReservedErrorCode::InvalidRequest,
                        format!("Expected 'jsonrpc' to be '2.0', but got '{}'", jsonrpc),
                    );
                    return Err(ErrorOrRejection::Error { id, error });
                }
            }
            Some(Value::Number(jsonrpc)) => {
                let error = Error::new(
                    ReservedErrorCode::InvalidRequest,
                    format!(
                        "Expected 'jsonrpc' to be a String with value '2.0', but got a Number '{}'",
                        jsonrpc
                    ),
                );
                return Err(ErrorOrRejection::Error { id, error });
            }
            Some(jsonrpc) => {
                let error = Error::new(
                    ReservedErrorCode::InvalidRequest,
                    format!(
                        "Expected 'jsonrpc' to be a String with value '2.0', but got '{}'",
                        jsonrpc
                    ),
                );
                return Err(ErrorOrRejection::Error { id, error });
            }
            None => {
                let error = Error::new(
                    ReservedErrorCode::InvalidRequest,
                    format!("Missing '{}' field", JSONRPC_FIELD_NAME),
                );
                return Err(ErrorOrRejection::Error { id, error });
            }
        }

        let method = match request.remove(METHOD_FIELD_NAME) {
            Some(Value::String(method)) => method,
            Some(_) => {
                let error = Error::new(
                    ReservedErrorCode::InvalidRequest,
                    format!("Expected '{}' to be a String", METHOD_FIELD_NAME),
                );
                return Err(ErrorOrRejection::Error { id, error });
            }
            None => {
                let error = Error::new(
                    ReservedErrorCode::InvalidRequest,
                    format!("Missing '{}' field", METHOD_FIELD_NAME),
                );
                return Err(ErrorOrRejection::Error { id, error });
            }
        };

        let params = match request.remove(PARAMS_FIELD_NAME) {
            Some(unvalidated_params) => Some(Params::try_from(&id, unvalidated_params)?),
            None => None,
        };

        let id = match request.remove(ID_FIELD_NAME) {
            Some(id) => {
                is_valid(&id).map_err(|error| ErrorOrRejection::Error {
                    id: Value::Null,
                    error,
                })?;
                id
            }
            None => return Err(ErrorOrRejection::Rejection(reject::custom(MissingId))),
        };

        if !allow_unknown_fields && !request.is_empty() {
            let error = Error::new(
                ReservedErrorCode::InvalidRequest,
                format!(
                    "Unexpected field{}: {}",
                    if request.len() > 1 { "s" } else { "" },
                    request.keys().map(|f| format!("'{}'", f)).join(", ")
                ),
            );
            return Err(ErrorOrRejection::Error { id, error });
        }

        Ok(Request { id, method, params })
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
            let params_inner = vec![Value::Bool(true)];

            let unvalidated = json!({
                JSONRPC_FIELD_NAME: JSON_RPC_VERSION,
                ID_FIELD_NAME: id,
                METHOD_FIELD_NAME: method,
                PARAMS_FIELD_NAME: params_inner,
            })
            .as_object()
            .cloned()
            .unwrap();

            let request = Request::new(unvalidated, false).unwrap();
            assert_eq!(request.id, id);
            assert_eq!(request.method, method);
            assert_eq!(request.params.unwrap(), Params::Array(params_inner));
        }

        run_test(Value::String("the id".to_string()));
        run_test(json!(1314));
        run_test(Value::Null);
    }

    #[test]
    fn should_fail_to_validate_id_with_wrong_type() {
        let request = json!({
            JSONRPC_FIELD_NAME: JSON_RPC_VERSION,
            ID_FIELD_NAME: true,
            METHOD_FIELD_NAME: "a",
        })
        .as_object()
        .cloned()
        .unwrap();

        let error = match Request::new(request, false) {
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
        let request = json!({
            JSONRPC_FIELD_NAME: JSON_RPC_VERSION,
            ID_FIELD_NAME: 1.1,
            METHOD_FIELD_NAME: "a",
        })
        .as_object()
        .cloned()
        .unwrap();

        let error = match Request::new(request, false) {
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
    fn should_reject_with_missing_id() {
        let request = json!({
            JSONRPC_FIELD_NAME: JSON_RPC_VERSION,
            METHOD_FIELD_NAME: "a",
        })
        .as_object()
        .cloned()
        .unwrap();

        match Request::new(request, false) {
            Err(ErrorOrRejection::Rejection(_)) => (),
            _ => panic!("should be rejection"),
        };
    }

    #[test]
    fn should_fail_to_validate_with_invalid_jsonrpc_field_value() {
        let request = json!({
            JSONRPC_FIELD_NAME: "2.1",
            ID_FIELD_NAME: "a",
            METHOD_FIELD_NAME: "a",
        })
        .as_object()
        .cloned()
        .unwrap();

        let error = match Request::new(request, false) {
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
                "Expected 'jsonrpc' to be '2.0', but got '2.1'"
            )
        );
    }

    #[test]
    fn should_fail_to_validate_with_invalid_jsonrpc_field_type() {
        let request = json!({
            JSONRPC_FIELD_NAME: true,
            ID_FIELD_NAME: "a",
            METHOD_FIELD_NAME: "a",
        })
        .as_object()
        .cloned()
        .unwrap();

        let error = match Request::new(request, false) {
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
                "Expected 'jsonrpc' to be a String with value '2.0', but got 'true'"
            )
        );
    }

    #[test]
    fn should_fail_to_validate_with_missing_jsonrpc_field() {
        let request = json!({
            ID_FIELD_NAME: "a",
            METHOD_FIELD_NAME: "a",
        })
        .as_object()
        .cloned()
        .unwrap();

        let error = match Request::new(request, false) {
            Err(ErrorOrRejection::Error {
                id: Value::String(id),
                error,
            }) if id == "a" => error,
            _ => panic!("should be error"),
        };
        assert_eq!(
            error,
            Error::new(ReservedErrorCode::InvalidRequest, "Missing 'jsonrpc' field")
        );
    }

    #[test]
    fn should_fail_to_validate_with_invalid_method_field_type() {
        let request = json!({
            JSONRPC_FIELD_NAME: JSON_RPC_VERSION,
            ID_FIELD_NAME: "a",
            METHOD_FIELD_NAME: 1,
        })
        .as_object()
        .cloned()
        .unwrap();

        let error = match Request::new(request, false) {
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
                "Expected 'method' to be a String"
            )
        );
    }

    #[test]
    fn should_fail_to_validate_with_missing_method_field() {
        let request = json!({
            JSONRPC_FIELD_NAME: JSON_RPC_VERSION,
            ID_FIELD_NAME: "a",
        })
        .as_object()
        .cloned()
        .unwrap();

        let error = match Request::new(request, false) {
            Err(ErrorOrRejection::Error {
                id: Value::String(id),
                error,
            }) if id == "a" => error,
            _ => panic!("should be error"),
        };
        assert_eq!(
            error,
            Error::new(ReservedErrorCode::InvalidRequest, "Missing 'method' field")
        );
    }

    #[test]
    fn should_fail_to_validate_with_invalid_params_type() {
        let request = json!({
            JSONRPC_FIELD_NAME: JSON_RPC_VERSION,
            ID_FIELD_NAME: "a",
            METHOD_FIELD_NAME: "a",
            PARAMS_FIELD_NAME: "a",
        })
        .as_object()
        .cloned()
        .unwrap();

        let error = match Request::new(request, false) {
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
                "If present, 'params' must be an Array or Object, but was a String"
            )
        );
    }

    fn request_with_extra_fields() -> Map<String, Value> {
        json!({
            JSONRPC_FIELD_NAME: JSON_RPC_VERSION,
            ID_FIELD_NAME: "a",
            METHOD_FIELD_NAME: "a",
            "extra": 1,
            "another": true,
        })
        .as_object()
        .cloned()
        .unwrap()
    }

    #[test]
    fn should_validate_with_extra_fields_if_allowed() {
        let request = request_with_extra_fields();
        assert!(Request::new(request, true).is_ok());
    }

    #[test]
    fn should_fail_to_validate_with_extra_fields_if_disallowed() {
        let request = request_with_extra_fields();
        let error = match Request::new(request, false) {
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
                "Unexpected fields: 'another', 'extra'"
            )
        );
    }
}
