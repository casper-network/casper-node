use std::{convert::TryInto, fs, path::Path};

use reqwest::StatusCode;

use crate::{data_source::DataSource, error::RpcServerTestError};

const QUERY_FILE_NAME: &str = "query.json";
const RESPONSE_SCHEMA_FILE_NAME: &str = "response_schema.json";
const RESPONSE_CODE_FILE_NAME: &str = "expected_response_code.txt";
const ERROR_RESPONSE_CODE_FILE_NAME: &str = "expected_error_response_code.txt";

pub(crate) struct TestCase {
    pub(crate) input: String,
    pub(crate) expected_schema: String, // TODO: Schema
    pub(crate) expected_response_code: StatusCode,
    pub(crate) expected_error_response_code: Option<i16>,
}

impl TestCase {
    pub(crate) fn from_test_directory(input: impl AsRef<Path>) -> Result<Self, RpcServerTestError> {
        let query_path = input.as_ref().to_path_buf().join(QUERY_FILE_NAME);
        let response_schema_path = input.as_ref().to_path_buf().join(RESPONSE_SCHEMA_FILE_NAME);
        let response_code_path = input.as_ref().to_path_buf().join(RESPONSE_CODE_FILE_NAME);
        let error_response_code_path = input
            .as_ref()
            .to_path_buf()
            .join(ERROR_RESPONSE_CODE_FILE_NAME);

        let query = DataSource::from_file(query_path.as_path());
        let expected_schema = DataSource::from_file(response_schema_path.as_path());
        let expected_code = TestCase::status_code_from_path(response_code_path)?;
        let expected_error_code = TestCase::integer_from_path(error_response_code_path).ok();

        Ok(Self {
            input: query.try_into()?,
            expected_schema: expected_schema.try_into()?,
            expected_response_code: expected_code,
            expected_error_response_code: expected_error_code,
        })
    }

    fn status_code_from_path(path: impl AsRef<Path>) -> Result<StatusCode, RpcServerTestError> {
        let code = fs::read_to_string(path)
            .map_err(|err| RpcServerTestError::ExpectedCodeFileError(err.to_string()))?;
        let code = code
            .parse()
            .map_err(|_| RpcServerTestError::IncorrectExpectedCode())?;
        Ok(code)
    }

    fn integer_from_path(path: impl AsRef<Path>) -> Result<i16, RpcServerTestError> {
        let code = fs::read_to_string(path)
            .map_err(|err| RpcServerTestError::ExpectedCodeFileError(err.to_string()))?;
        let code = code
            .parse()
            .map_err(|_| RpcServerTestError::IncorrectExpectedCode())?;
        Ok(code)
    }
}
