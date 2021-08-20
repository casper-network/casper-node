use std::{convert::TryInto, path::Path};

use crate::{data_source::DataSource, error::RpcServerTestError};

const QUERY_FILE_NAME: &str = "query.json";
const RESPONSE_SCHEMA_FILE_NAME: &str = "response_schema.json";

pub(crate) struct TestSuite {
    pub(crate) input: String,
    pub(crate) expected: String, // TODO: Schema
}

impl TestSuite {
    #[allow(dead_code)]
    pub(crate) fn new(input: DataSource, expected: DataSource) -> Result<Self, RpcServerTestError> {
        Ok(Self {
            input: input.try_into()?,
            expected: expected.try_into()?,
        })
    }

    pub(crate) fn from_test_directory(input: impl AsRef<Path>) -> Result<Self, RpcServerTestError> {
        let query_path = input.as_ref().to_path_buf().join(QUERY_FILE_NAME);
        let response_schema_path = input.as_ref().to_path_buf().join(RESPONSE_SCHEMA_FILE_NAME);

        let query = DataSource::from_file(query_path.as_path());
        let expected = DataSource::from_file(response_schema_path.as_path());

        Ok(Self {
            input: query.try_into()?,
            expected: expected.try_into()?,
        })
    }
}
