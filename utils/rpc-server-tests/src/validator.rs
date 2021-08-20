use jsonschema::JSONSchema;
use reqwest::StatusCode;
use serde_json::Value;

use crate::{error::RpcServerTestError, test_suite::TestSuite};

pub(crate) type ValidationErrors = Vec<String>;

pub(crate) struct Validator {
    schema: JSONSchema,
    expected_response_code: StatusCode,
}

impl Validator {
    pub(crate) fn try_from_test_suite(test_suite: &TestSuite) -> Result<Self, RpcServerTestError> {
        let schema = serde_json::from_str(&test_suite.expected_schema)
            .map_err(|err| RpcServerTestError::SchemaIsNotAJson(err.to_string()))?;
        let compiled = JSONSchema::compile(&schema)
            .map_err(|err| RpcServerTestError::SchemaSyntax(err.to_string()))?;
        Ok(Self {
            schema: compiled,
            expected_response_code: test_suite.expected_response_code,
        })
    }

    pub(crate) fn validate(
        &self,
        actual_response_status_code: StatusCode,
        actual_response_body: &str,
    ) -> Option<ValidationErrors> {
        if actual_response_status_code != self.expected_response_code {
            return Some(vec![format!(
                "Return code mismatch. Expected '{}' got '{}'",
                self.expected_response_code, actual_response_status_code
            )]);
        }

        let actual: Result<Value, _> = serde_json::from_str(actual_response_body);
        if let Ok(actual) = actual {
            match self.schema.validate(&actual) {
                Ok(_) => None,
                Err(errors) => Some(errors.into_iter().map(|err| err.to_string()).collect()),
            }
        } else {
            Some(vec!["Provided 'actual' is not a correct JSON".to_string()])
        }
    }
}
