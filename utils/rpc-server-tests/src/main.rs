mod args;
mod data_source;
mod error;
mod executor;
mod exit_codes;
mod test_runner;
mod test_suite;
mod validator;

use args::Args;
use structopt::StructOpt;

use crate::{exit_codes::ExitCodes, test_runner::TestRunner, test_suite::TestSuite};

#[tokio::main]
async fn main() {
    let args = Args::from_args();

    std::process::exit(match TestSuite::from_test_directory(args.test_directory) {
        Ok(test_suite) => {
            let test_result = TestRunner::run(test_suite, &args.node_address, &args.api_path).await;
            match test_result {
                Ok(validation_errors) => validation_errors.map_or_else(
                    || {
                        println!("OK");
                        ExitCodes::OK as i32
                    },
                    |validation_errors| {
                        validation_errors
                            .into_iter()
                            .for_each(|err| println!("{}", err));
                        ExitCodes::ValidationError as i32
                    },
                ),
                Err(err) => {
                    println!("TestRunner encountered an error: {:?}", err);
                    ExitCodes::TestRunnerError as i32
                }
            }
        }
        Err(err) => {
            println!("Can't prepare data for test: {:?}", err);
            ExitCodes::InputDataError as i32
        }
    });
}
