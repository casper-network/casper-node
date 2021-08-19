use jsonschema::JSONSchema;
use serde_json::Value;

use crate::error::RpcServerTestError;

pub(crate) type ValidationErrors = Vec<String>;

pub(crate) struct Validator {
    schema: JSONSchema,
}

impl Validator {
    pub(crate) fn try_from_schema(schema: &str) -> Result<Self, RpcServerTestError> {
        let schema = serde_json::from_str(schema)
            .map_err(|err| RpcServerTestError::SchemaIsNotAJson(err.to_string()))?;
        let compiled = JSONSchema::compile(&schema)
            .map_err(|err| RpcServerTestError::SchemaSyntax(err.to_string()))?;
        Ok(Self { schema: compiled })
    }

    pub(crate) fn validate(&self, actual: &str) -> Option<ValidationErrors> {
        let actual: Result<Value, _> = serde_json::from_str(actual);
        if let Ok(actual) = actual {
            // TODO: map_err() ?
            match self.schema.validate(&actual) {
                Ok(_) => None,
                Err(errors) => Some(errors.into_iter().map(|err| err.to_string()).collect()),
            }
        } else {
            Some(vec!["Provided 'actual' is not a correct JSON".to_string()])
        }
    }
}
