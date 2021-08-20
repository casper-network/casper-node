use jsonschema::JSONSchema;
use reqwest::StatusCode;
use serde_json::Value;
use std::convert::TryFrom;

use crate::{error::RpcServerTestError, test_case::TestCase};

pub(crate) type ValidationErrors = Vec<String>;

pub(crate) struct Validator {
    schema: JSONSchema,
    expected_http_response_code: StatusCode,
    expected_error_response_code: Option<i16>,
}

impl Validator {
    pub(crate) fn try_from_test_case(test_case: &TestCase) -> Result<Self, RpcServerTestError> {
        let schema = serde_json::from_str(&test_case.expected_schema)
            .map_err(|err| RpcServerTestError::SchemaIsNotAJson(err.to_string()))?;
        let compiled = JSONSchema::compile(&schema)
            .map_err(|err| RpcServerTestError::SchemaSyntax(err.to_string()))?;
        Ok(Self {
            schema: compiled,
            expected_http_response_code: test_case.expected_response_code,
            expected_error_response_code: test_case.expected_error_response_code,
        })
    }

    pub(crate) fn validate(
        &self,
        actual_response_status_code: StatusCode,
        actual_response_body: &str,
    ) -> Result<Option<ValidationErrors>, RpcServerTestError> {
        if actual_response_status_code != self.expected_http_response_code {
            return Ok(Some(vec![format!(
                "Return code mismatch. Expected '{}' got '{}'",
                self.expected_http_response_code, actual_response_status_code
            )]));
        }

        let actual: Result<Value, _> = serde_json::from_str(actual_response_body);
        if let Ok(actual) = actual {
            match self.schema.validate(&actual) {
                Ok(_) => {
                    if let Some(expected_error_code) = self.expected_error_response_code {
                        let actual_error_code = Validator::extract_error_code(&actual)?;
                        if actual_error_code != expected_error_code {
                            return Ok(Some(vec![format!(
                                "Error code mismatch. Expected '{}' got '{}'",
                                expected_error_code, actual_error_code
                            )]));
                        } else {
                            Ok(None)
                        }
                    } else {
                        Ok(None)
                    }
                }

                Err(errors) => Ok(Some(
                    errors.into_iter().map(|err| err.to_string()).collect(),
                )),
            }
        } else {
            Ok(Some(vec![
                "Provided 'actual' is not a correct JSON".to_string()
            ]))
        }
    }

    fn extract_error_code(body: &Value) -> Result<i16, RpcServerTestError> {
        let error_code = body
            .get("error")
            .ok_or(RpcServerTestError::CantExtractErrorCode())?
            .get("code")
            .ok_or(RpcServerTestError::CantExtractErrorCode())?
            .as_i64()
            .ok_or(RpcServerTestError::CantExtractErrorCode())?;

        Ok(i16::try_from(error_code).map_err(|_| RpcServerTestError::CantExtractErrorCode())?)
    }
}
