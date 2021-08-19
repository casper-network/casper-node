mod data;
mod error;
mod executor;
mod json_source;
mod test_runner;
mod test_suite;
mod validator;

use crate::{
    executor::Executor, json_source::JsonSource, test_runner::TestRunner, test_suite::TestSuite,
};

struct Args {
    /// Node address, for example `127.0.0.1:11011`
    node_address: String,
}

#[tokio::main]
async fn main() {
    let node_address = "172.16.0.8:11101";
    let api_path = "/rpc";

    let query = r#"
        {
            "id": 1,
            "jsonrpc": "2.0",
            "method": "info_get_status"
        }
    "#;

    let test_suite = TestSuite {
        input: JsonSource::from_raw_string(query),
        expected: data::SCHEMA.to_string(),
    };

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
