mod config;
mod http_server;
mod node_client;
mod rpcs;
mod speculative_exec_config;
mod speculative_exec_server;

pub use config::{Config as RpcConfig, NodeClientConfig};
pub use http_server::run as run_rpc_server;
pub use node_client::{Error as ClientError, JulietNodeClient, NodeClient};
pub use speculative_exec_config::Config as SpeculativeExecConfig;
pub use speculative_exec_server::run as run_speculative_exec_server;

#[cfg(test)]
mod tests {
    use std::fs;

    use assert_json_diff::{assert_json_eq, assert_json_matches_no_panic, CompareMode, Config};
    use regex::Regex;
    use serde_json::Value;
    use std::io::Write;

    use crate::rpcs::docs::OPEN_RPC_SCHEMA;

    use crate::rpcs::info::GetStatusResult;
    use crate::rpcs::{
        docs::OpenRpcSchema,
        info::{GetChainspecResult, GetValidatorChangesResult},
    };
    use schemars::schema_for;

    #[test]
    fn json_schema_check() {
        let schema_path = format!(
            "{}/../resources/test/rpc_schema.json",
            env!("CARGO_MANIFEST_DIR")
        );
        assert_schema(
            &schema_path,
            &serde_json::to_string_pretty(&*OPEN_RPC_SCHEMA).unwrap(),
        );

        let schema = fs::read_to_string(&schema_path).unwrap();

        // Check for the following pattern in the JSON as this points to a byte array or vec (e.g.
        // a hash digest) not being represented as a hex-encoded string:
        //
        // ```json
        // "type": "array",
        // "items": {
        //   "type": "integer",
        //   "format": "uint8",
        //   "minimum": 0.0
        // },
        // ```
        //
        // The type/variant in question (most easily identified from the git diff) might be easily
        // fixed via application of a serde attribute, e.g.
        // `#[serde(with = "serde_helpers::raw_32_byte_array")]`.  It will likely require a
        // schemars attribute too, indicating it is a hex-encoded string.  See for example
        // `TransactionInvocationTarget::Package::addr`.
        let regex = Regex::new(
            r#"\s*"type":\s*"array",\s*"items":\s*\{\s*"type":\s*"integer",\s*"format":\s*"uint8",\s*"minimum":\s*0\.0\s*\},"#
        ).unwrap();
        assert!(
            !regex.is_match(&schema),
            "seems like a byte array is not hex-encoded - see comment in `json_schema_check` for \
            further info"
        );
    }

    #[test]
    fn json_schema_status_check() {
        let schema_path = format!(
            "{}/../resources/test/schema_status.json",
            env!("CARGO_MANIFEST_DIR")
        );
        assert_schema(
            &schema_path,
            &serde_json::to_string_pretty(&schema_for!(GetStatusResult)).unwrap(),
        );
    }

    #[test]
    fn json_schema_validator_changes_check() {
        let schema_path = format!(
            "{}/../resources/test/schema_validator_changes.json",
            env!("CARGO_MANIFEST_DIR")
        );
        assert_schema(
            &schema_path,
            &serde_json::to_string_pretty(&schema_for!(GetValidatorChangesResult)).unwrap(),
        );
    }

    #[test]
    fn json_schema_rpc_schema_check() {
        let schema_path = format!(
            "{}/../resources/test/schema_rpc_schema.json",
            env!("CARGO_MANIFEST_DIR")
        );
        assert_schema(
            &schema_path,
            &serde_json::to_string_pretty(&schema_for!(OpenRpcSchema)).unwrap(),
        );
    }

    #[test]
    fn json_schema_chainspec_bytes_check() {
        let schema_path = format!(
            "{}/../resources/test/schema_chainspec_bytes.json",
            env!("CARGO_MANIFEST_DIR")
        );
        assert_schema(
            &schema_path,
            &serde_json::to_string_pretty(&schema_for!(GetChainspecResult)).unwrap(),
        );
    }

    /// Assert that the file at `schema_path` matches the provided `actual_schema`, which can be
    /// derived from `schemars::schema_for!` or `schemars::schema_for_value!`, for example. This
    /// method will create a temporary file with the actual schema and print the location if it
    /// fails.
    pub fn assert_schema(schema_path: &str, actual_schema: &str) {
        let expected_schema = fs::read_to_string(&schema_path).unwrap();
        let expected_schema: Value = serde_json::from_str(&expected_schema).unwrap();
        let mut temp_file = tempfile::Builder::new()
            .suffix(".json")
            .tempfile_in(env!("OUT_DIR"))
            .unwrap();
        temp_file.write_all(actual_schema.as_bytes()).unwrap();
        let actual_schema: Value = serde_json::from_str(&actual_schema).unwrap();
        let (_file, temp_file_path) = temp_file.keep().unwrap();

        let result = assert_json_matches_no_panic(
            &actual_schema,
            &expected_schema,
            Config::new(CompareMode::Strict),
        );
        assert_eq!(
            result,
            Ok(()),
            "schema does not match:\nexpected:\n{}\nactual:\n{}\n",
            schema_path,
            temp_file_path.display()
        );
        assert_json_eq!(actual_schema, expected_schema);
    }
}
