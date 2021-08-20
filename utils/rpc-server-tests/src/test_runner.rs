use crate::{
    error::RpcServerTestError,
    executor::Executor,
    test_case::TestCase,
    validator::{ValidationErrors, Validator},
};

pub(crate) struct TestRunner {}

impl TestRunner {
    pub(crate) async fn run(
        test_case: TestCase,
        node_address: &str,
        api_path: &str,
    ) -> Result<Option<ValidationErrors>, RpcServerTestError> {
        let executor = Executor::new(node_address, api_path);
        let (status_code, body) = executor
            .execute(&test_case.input)
            .await
            .map_err(|err| RpcServerTestError::Other(err.to_string()))?;

        let validator = Validator::try_from_test_case(&test_case)?;
        validator.validate(status_code, &body)
    }
}
