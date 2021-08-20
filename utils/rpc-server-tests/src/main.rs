mod data_source;
mod error;
mod executor;
mod test_runner;
mod test_suite;
mod validator;

use std::path::Path;

use crate::{data_source::DataSource, test_runner::TestRunner, test_suite::TestSuite};

struct Args {
    /// Node address, for example `127.0.0.1:11011`
    node_address: String,
}

#[tokio::main]
async fn main() {
    let node_address = "172.16.0.8:11101";
    let api_path = "/rpc";

    let query = DataSource::from_file(Path::new("test_suites/info_get_status/query.json"));
    let expected = DataSource::from_file(Path::new(
        "test_suites/info_get_status/response_schema.json",
    ));

    match TestSuite::new(query, expected) {
        Ok(test_suite) => {
            let test_result = TestRunner::run(test_suite, node_address, api_path).await;
            match test_result {
                Ok(validation_errors) => validation_errors.map_or_else(
                    || {
                        println!("OK");
                    },
                    |validation_errors| {
                        validation_errors
                            .into_iter()
                            .for_each(|err| println!("{}", err));
                    },
                ),
                Err(err) => println!("TestRunner encountered an error: {:?}", err),
            }
        }
        Err(err) => println!("Can't prepare data for test: {:?}", err),
    }
}
