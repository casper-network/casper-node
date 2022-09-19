use std::fmt::{self, Display, Formatter};

use serde_json::{Map, Value};

use super::ErrorOrRejection;
use crate::error::{Error, ReservedErrorCode};

/// The "params" field of a JSON-RPC request.
///
/// As per [the JSON-RPC specification](https://www.jsonrpc.org/specification#parameter_structures),
/// if present these must be a JSON Array or Object.
///
/// **NOTE:** Currently we treat '"params": null' as '"params": []', but this deviation from the
/// standard will be removed in an upcoming release, and `null` will become an invalid value.
///
/// `Params` is effectively a restricted [`serde_json::Value`], and can be converted to a `Value`
/// using `Value::from()` if required.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum Params {
    /// Represents a JSON Array.
    Array(Vec<Value>),
    /// Represents a JSON Object.
    Object(Map<String, Value>),
}

impl Params {
    pub(super) fn try_from(request_id: &Value, params: Value) -> Result<Self, ErrorOrRejection> {
        let err_invalid_request = |additional_info: &str| {
            let error = Error::new(ReservedErrorCode::InvalidRequest, additional_info);
            Err(ErrorOrRejection::Error {
                id: request_id.clone(),
                error,
            })
        };

        match params {
            Value::Null => Ok(Params::Array(vec![])),
            Value::Bool(false) => err_invalid_request(
                "If present, 'params' must be an Array or Object, but was 'false'",
            ),
            Value::Bool(true) => err_invalid_request(
                "If present, 'params' must be an Array or Object, but was 'true'",
            ),
            Value::Number(_) => err_invalid_request(
                "If present, 'params' must be an Array or Object, but was a Number",
            ),
            Value::String(_) => err_invalid_request(
                "If present, 'params' must be an Array or Object, but was a String",
            ),
            Value::Array(array) => Ok(Params::Array(array)),
            Value::Object(map) => Ok(Params::Object(map)),
        }
    }

    /// Returns `true` if `self` is an Array, otherwise returns `false`.
    pub fn is_array(&self) -> bool {
        self.as_array().is_some()
    }

    /// Returns a reference to the inner `Vec` if `self` is an Array, otherwise returns `None`.
    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Params::Array(array) => Some(array),
            _ => None,
        }
    }

    /// Returns a mutable reference to the inner `Vec` if `self` is an Array, otherwise returns
    /// `None`.
    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Params::Array(array) => Some(array),
            _ => None,
        }
    }

    /// Returns `true` if `self` is an Object, otherwise returns `false`.
    pub fn is_object(&self) -> bool {
        self.as_object().is_some()
    }

    /// Returns a reference to the inner `Map` if `self` is an Object, otherwise returns `None`.
    pub fn as_object(&self) -> Option<&Map<String, Value>> {
        match self {
            Params::Object(map) => Some(map),
            _ => None,
        }
    }

    /// Returns a mutable reference to the inner `Map` if `self` is an Object, otherwise returns
    /// `None`.
    pub fn as_object_mut(&mut self) -> Option<&mut Map<String, Value>> {
        match self {
            Params::Object(map) => Some(map),
            _ => None,
        }
    }

    /// Returns `true` if `self` is an empty Array or an empty Object, otherwise returns `false`.
    pub fn is_empty(&self) -> bool {
        match self {
            Params::Array(array) => array.is_empty(),
            Params::Object(map) => map.is_empty(),
        }
    }
}

impl Display for Params {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        Display::fmt(&Value::from(self.clone()), formatter)
    }
}

/// The default value for `Params` is an empty Array.
impl Default for Params {
    fn default() -> Self {
        Params::Array(vec![])
    }
}

impl From<Params> for Value {
    fn from(params: Params) -> Self {
        match params {
            Params::Array(array) => Value::Array(array),
            Params::Object(map) => Value::Object(map),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn should_fail_to_convert_invalid_params(bad_params: Value, expected_invalid_type_msg: &str) {
        let original_id = Value::from(1_i8);
        match Params::try_from(&original_id, bad_params).unwrap_err() {
            ErrorOrRejection::Error { id, error } => {
                assert_eq!(id, original_id);
                let expected_error = format!(
                    r#"{{"code":-32600,"message":"Invalid Request","data":"If present, 'params' must be an Array or Object, but was {}"}}"#,
                    expected_invalid_type_msg
                );
                assert_eq!(serde_json::to_string(&error).unwrap(), expected_error);
            }
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    fn should_convert_params_from_null() {
        let original_id = Value::from(1_i8);

        let params = Params::try_from(&original_id, Value::Null).unwrap();
        assert!(matches!(params, Params::Array(v) if v.is_empty()));
    }

    #[test]
    fn should_fail_to_convert_params_from_false() {
        should_fail_to_convert_invalid_params(Value::Bool(false), "'false'")
    }

    #[test]
    fn should_fail_to_convert_params_from_true() {
        should_fail_to_convert_invalid_params(Value::Bool(true), "'true'")
    }

    #[test]
    fn should_fail_to_convert_params_from_a_number() {
        should_fail_to_convert_invalid_params(Value::from(9_u8), "a Number")
    }

    #[test]
    fn should_fail_to_convert_params_from_a_string() {
        should_fail_to_convert_invalid_params(Value::from("s"), "a String")
    }

    #[test]
    fn should_convert_params_from_an_array() {
        let original_id = Value::from(1_i8);

        let params = Params::try_from(&original_id, Value::Array(vec![])).unwrap();
        assert!(matches!(params, Params::Array(v) if v.is_empty()));

        let array = vec![Value::from(9_i16), Value::Bool(false)];
        let params = Params::try_from(&original_id, Value::Array(array.clone())).unwrap();
        assert!(matches!(params, Params::Array(v) if v == array));
    }

    #[test]
    fn should_convert_params_from_an_object() {
        let original_id = Value::from(1_i8);

        let params = Params::try_from(&original_id, Value::Object(Map::new())).unwrap();
        assert!(matches!(params, Params::Object(v) if v.is_empty()));

        let mut map = Map::new();
        map.insert("a".to_string(), Value::from(9_i16));
        map.insert("b".to_string(), Value::Bool(false));
        let params = Params::try_from(&original_id, Value::Object(map.clone())).unwrap();
        assert!(matches!(params, Params::Object(v) if v == map));
    }
}
