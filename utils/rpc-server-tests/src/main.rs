mod args;
mod data_source;
mod error;
mod executor;
mod exit_codes;
mod test_case;
mod test_runner;
mod validator;

use std::path::Path;

use args::Args;
use structopt::StructOpt;
use validator::ValidationErrors;

use crate::{exit_codes::ExitCodes, test_case::TestCase, test_runner::TestRunner};

pub async fn run_test_case(
    directory: impl AsRef<Path>,
    node_address: &str,
    api_path: &str,
) -> (i32, Option<ValidationErrors>) {
    match TestCase::from_test_directory(directory) {
        Ok(test_case) => {
            let test_result = TestRunner::run(test_case, node_address, api_path).await;
            match test_result {
                Ok(validation_errors) => validation_errors.map_or_else(
                    || (ExitCodes::OK as i32, None),
                    |validation_errors| {
                        (ExitCodes::ValidationError as i32, Some(validation_errors))
                    },
                ),
                Err(err) => (
                    ExitCodes::TestRunnerError as i32,
                    Some(vec![format!("{:?}", err)]),
                ),
            }
        }
        Err(err) => (
            ExitCodes::InputDataError as i32,
            Some(vec![format!("{:?}", err)]),
        ),
    }
}

#[tokio::main]
async fn main() {
    let args = Args::from_args();

    std::process::exit({
        let (exit_code, validation_errors) =
            run_test_case(&args.test_directory, &args.node_address, &args.api_path).await;

        println!("Exit code: {:?}", exit_code);

        if let Some(validation_errors) = validation_errors {
            validation_errors
                .into_iter()
                .for_each(|err| println!("{}", err));
        } else {
            println!("No validation errors");
        }

        exit_code
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    async fn assert_test_case(directory: impl AsRef<Path>) {
        let node_address =
            env::var("RPC_SERVER_TESTS_NODE_ADDRESS").unwrap_or("127.0.0.1".to_string());
        let api_path = env::var("RPC_SERVER_TESTS_API_PATH").unwrap_or("/rpc".to_string());
        let (error_code, _) = run_test_case(directory, &node_address, &api_path).await;
        assert_eq!(error_code, 0);
    }

    mod ok {
        use super::*;

        #[tokio::test]
        async fn info_get_status() {
            assert_test_case("test_suites/ok/info_get_status/").await;
        }
    }

    mod err {
        use super::*;

        #[tokio::test]
        async fn random_request_body() {
            assert_test_case("test_suites/err/random_request_body").await;
        }

        #[tokio::test]
        async fn unknown_method() {
            assert_test_case("test_suites/err/unknown_method").await;
        }
    }
}
