use crate::{
    error::RpcServerTestError,
    executor::Executor,
    test_suite::TestSuite,
    validator::{ValidationErrors, Validator},
};

pub(crate) struct TestRunner {}

impl TestRunner {
    pub(crate) async fn run(
        test_suite: TestSuite,
        node_address: &str,
        api_path: &str,
    ) -> Result<Option<ValidationErrors>, RpcServerTestError> {
        let executor = Executor::new(node_address, api_path);
        let (status_code, body) = executor
            .execute(&test_suite.input)
            .await
            .map_err(|err| RpcServerTestError::Other(err.to_string()))?;

        let validator = Validator::try_from_test_suite(&test_suite)?;
        Ok(validator.validate(status_code, &body))
    }
}
