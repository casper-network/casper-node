use std::fs;

use crate::{error::RpcServerTestError, json_source::JsonSource};

pub(crate) struct TestSuite {
    pub(crate) input: String,
    pub(crate) expected: String, // TODO: Schema
}

impl TestSuite {
    pub(crate) fn new(input: JsonSource, expected: String) -> Result<Self, RpcServerTestError> {
        Ok(Self {
            input: match input {
                JsonSource::Raw(query) => query.to_string(),
                JsonSource::File(path) => fs::read_to_string(path)
                    .map_err(|err| RpcServerTestError::UnableToCreateQuery(err.to_string()))?,
            },
            expected,
        })
    }
}
