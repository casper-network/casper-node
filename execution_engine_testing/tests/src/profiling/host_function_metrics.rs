//! This executable is for outputting metrics on each of the EE host functions.
//!
//! In order to set up the required global state, the `state-initializer` should have been run
//! first.

use std::{
    collections::BTreeMap,
    env,
    fs::{self, File},
    io::{self, Write},
    iter,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
};

use clap::{crate_version, App, Arg};
use log::LevelFilter;
use rand::{self, Rng};
use serde_json::Value;

use casper_engine_test_support::{DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder};
use casper_execution_engine::{
    core::engine_state::EngineConfig,
    shared::logging::{self, Settings},
};
use casper_types::{bytesrepr::Bytes, runtime_args, ApiError, RuntimeArgs, U512};

use casper_engine_tests::profiling;
use casper_hashing::Digest;

const ABOUT: &str =
    "Executes a contract which logs metrics for all host functions.  Note that the \
     'state-initializer' executable should be run first to set up the required global state.";

const EXECUTE_AS_SUBPROCESS_ARG: &str = "execute-as-subprocess";

const ROOT_HASH_ARG_NAME: &str = "root-hash";
const ROOT_HASH_ARG_VALUE_NAME: &str = "HEX-ENCODED HASH";
const ROOT_HASH_ARG_HELP: &str =
    "Initial root hash; the output of running the 'state-initializer' executable";

const REPETITIONS_ARG_NAME: &str = "repetitions";
const REPETITIONS_ARG_SHORT: &str = "r";
const REPETITIONS_ARG_DEFAULT: &str = "10000";
const REPETITIONS_ARG_VALUE_NAME: &str = "NUM";
const REPETITIONS_ARG_HELP: &str = "Number of repetitions of each host function call";

const OUTPUT_DIR_ARG_NAME: &str = "output-dir";
const OUTPUT_DIR_ARG_SHORT: &str = "o";
const OUTPUT_DIR_ARG_VALUE_NAME: &str = "DIR";
const OUTPUT_DIR_ARG_HELP: &str =
    "Path to output directory.  It will be created if it doesn't exist.  If unspecified, the \
    current working directory will be used";

const HOST_FUNCTION_METRICS_CONTRACT: &str = "host_function_metrics.wasm";
const PAYMENT_AMOUNT: u64 = profiling::ACCOUNT_1_INITIAL_AMOUNT - 1_000_000_000;
const EXPECTED_REVERT_VALUE: u16 = 9;
const CSV_HEADER: &str = "args,n_exec,total_elapsed_time";
const ARG_AMOUNT: &str = "amount";
const ARG_SEED: &str = "seed";
const ARG_OTHERS: &str = "others";

fn execute_as_subprocess_arg() -> Arg<'static, 'static> {
    Arg::with_name(EXECUTE_AS_SUBPROCESS_ARG)
        .long(EXECUTE_AS_SUBPROCESS_ARG)
        .hidden(true)
}

fn root_hash_arg() -> Arg<'static, 'static> {
    Arg::with_name(ROOT_HASH_ARG_NAME)
        .value_name(ROOT_HASH_ARG_VALUE_NAME)
        .help(ROOT_HASH_ARG_HELP)
}

fn repetitions_arg() -> Arg<'static, 'static> {
    Arg::with_name(REPETITIONS_ARG_NAME)
        .long(REPETITIONS_ARG_NAME)
        .short(REPETITIONS_ARG_SHORT)
        .default_value(REPETITIONS_ARG_DEFAULT)
        .value_name(REPETITIONS_ARG_VALUE_NAME)
        .help(REPETITIONS_ARG_HELP)
}

fn output_dir_arg() -> Arg<'static, 'static> {
    Arg::with_name(OUTPUT_DIR_ARG_NAME)
        .long(OUTPUT_DIR_ARG_NAME)
        .short(OUTPUT_DIR_ARG_SHORT)
        .value_name(OUTPUT_DIR_ARG_VALUE_NAME)
        .help(OUTPUT_DIR_ARG_HELP)
}

#[derive(Debug)]
struct Args {
    execute_as_subprocess: bool,
    root_hash: Option<String>,
    repetitions: usize,
    output_dir: PathBuf,
    data_dir: PathBuf,
}

impl Args {
    fn new() -> Self {
        let exe_name = profiling::exe_name();
        let data_dir_arg = profiling::data_dir_arg();
        let arg_matches = App::new(&exe_name)
            .version(crate_version!())
            .about(ABOUT)
            .arg(execute_as_subprocess_arg())
            .arg(root_hash_arg())
            .arg(repetitions_arg())
            .arg(output_dir_arg())
            .arg(data_dir_arg)
            .get_matches();
        let execute_as_subprocess = arg_matches.is_present(EXECUTE_AS_SUBPROCESS_ARG);
        let root_hash = arg_matches
            .value_of(ROOT_HASH_ARG_NAME)
            .map(ToString::to_string);
        let repetitions = arg_matches
            .value_of(REPETITIONS_ARG_NAME)
            .map(profiling::parse_count)
            .expect("should have repetitions");
        let output_dir = match arg_matches.value_of(OUTPUT_DIR_ARG_NAME) {
            Some(dir) => PathBuf::from_str(dir).expect("Expected a valid unicode path"),
            None => env::current_dir().expect("Expected to be able to access current working dir"),
        };
        let data_dir = profiling::data_dir(&arg_matches);
        Args {
            execute_as_subprocess,
            root_hash,
            repetitions,
            output_dir,
            data_dir,
        }
    }
}

/// Executes the host-function-metrics contract repeatedly to generate metrics in stdout.
fn run_test(root_hash: Vec<u8>, repetitions: usize, data_dir: &Path) {
    let log_settings = Settings::new(LevelFilter::Warn).with_metrics_enabled(true);
    let _ = logging::initialize(log_settings);

    let account_1_account_hash = profiling::account_1_account_hash();
    let account_2_account_hash = profiling::account_2_account_hash();

    let engine_config = EngineConfig::default();

    let mut test_builder =
        LmdbWasmTestBuilder::open(data_dir, engine_config, Digest::hash(&root_hash));

    let mut rng = rand::thread_rng();

    for _ in 0..repetitions {
        let seed: u64 = rng.gen();
        let random_bytes_length: usize = rng.gen_range(0..10_000);
        let mut random_bytes = vec![0_u8; random_bytes_length];
        rng.fill(random_bytes.as_mut_slice());

        let deploy = DeployItemBuilder::new()
            .with_address(account_1_account_hash)
            .with_deploy_hash(rng.gen())
            .with_session_code(
                HOST_FUNCTION_METRICS_CONTRACT,
                runtime_args! {
                    ARG_SEED => seed,
                    ARG_OTHERS => (Bytes::from(random_bytes), account_1_account_hash, account_2_account_hash),
                },
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => U512::from(PAYMENT_AMOUNT) })
            .with_authorization_keys(&[account_1_account_hash])
            .build();
        let exec_request = ExecuteRequestBuilder::new()
            .push_deploy(deploy.clone())
            .build();

        test_builder.exec(exec_request);
        // Should revert with User error 10.
        let error_msg = test_builder
            .exec_error_message(0)
            .expect("should have error message");
        assert!(
            error_msg.contains(&format!("{:?}", ApiError::User(EXPECTED_REVERT_VALUE))),
            "{}",
            error_msg
        );
    }
}

#[derive(Debug)]
struct Metrics {
    duration: String,
    others: BTreeMap<String, String>,
}

fn gather_metrics(stdout: String) -> BTreeMap<String, Vec<Metrics>> {
    const PAYLOAD_KEY: &str = "payload=";
    const DESCRIPTION_KEY: &str = "description";
    const HOST_FUNCTION_PREFIX: &str = "host_function_";
    const PROPERTIES_KEY: &str = "properties";
    const DURATION_KEY: &str = "duration_in_seconds";
    const MESSAGE_KEY: &str = "message";
    const MESSAGE_TEMPLATE_KEY: &str = "message_template";

    let mut result = BTreeMap::new();

    for line in stdout.lines() {
        if let Some(index) = line.find(PAYLOAD_KEY) {
            let (_, payload_slice) = line.split_at(index + PAYLOAD_KEY.len());
            let mut payload =
                serde_json::from_str::<Value>(payload_slice).expect("payload should parse as JSON");

            let description = payload
                .get_mut(DESCRIPTION_KEY)
                .expect("payload should have description field")
                .take();
            let function_id = description
                .as_str()
                .expect("description field should parse as string")
                .split(' ')
                .next()
                .expect("description field should consist of function name followed by a space");
            if !function_id.starts_with(HOST_FUNCTION_PREFIX) {
                continue;
            }
            let function_name = function_id
                .split_at(HOST_FUNCTION_PREFIX.len())
                .1
                .to_string();

            let metrics_vec = result.entry(function_name).or_insert_with(Vec::new);

            let mut properties: BTreeMap<String, String> = serde_json::from_value(
                payload
                    .get_mut(PROPERTIES_KEY)
                    .expect("payload should have properties field")
                    .take(),
            )
            .expect("properties should parse as pairs of strings");

            let duration = properties
                .remove(DURATION_KEY)
                .expect("properties should have a duration entry");
            let _ = properties.remove(MESSAGE_KEY);
            let _ = properties.remove(MESSAGE_TEMPLATE_KEY);

            let metrics = Metrics {
                duration,
                others: properties,
            };
            metrics_vec.push(metrics);
        }
    }

    result
}

fn generate_csv(function_name: String, metrics_vec: Vec<Metrics>, output_dir: &Path) {
    let file_path = output_dir.join(format!("{}.csv", function_name));
    let mut file = File::create(&file_path)
        .unwrap_or_else(|_| panic!("should create {}", file_path.display()));

    writeln!(file, "{}", CSV_HEADER)
        .unwrap_or_else(|_| panic!("should write to {}", file_path.display()));

    for metrics in metrics_vec {
        write!(file, "\"(").unwrap_or_else(|_| panic!("should write to {}", file_path.display()));
        for (_metric_key, metric_value) in metrics.others {
            write!(file, "{},", metric_value)
                .unwrap_or_else(|_| panic!("should write to {}", file_path.display()));
        }
        writeln!(file, ")\",1,{}", metrics.duration)
            .unwrap_or_else(|_| panic!("should write to {}", file_path.display()));
    }
}

fn main() {
    let args = Args::new();

    // If the required initial root hash wasn't passed as a command line arg, expect to read it in
    // from stdin to allow for it to be piped from the output of 'state-initializer'.
    let (root_hash, root_hash_read_from_stdin) = match args.root_hash {
        Some(root_hash) => (root_hash, false),
        None => {
            let mut input = String::new();
            let _ = io::stdin().read_line(&mut input);
            (input.trim_end().to_string(), true)
        }
    };

    // We're running as a subprocess - execute the test to output the metrics to stdout.
    if args.execute_as_subprocess {
        return run_test(
            profiling::parse_hash(&root_hash),
            args.repetitions,
            &args.data_dir,
        );
    }

    // We're running as the top-level process - invoke the current exe as a subprocess to capture
    // its stdout.
    let subprocess_flag = format!("--{}", EXECUTE_AS_SUBPROCESS_ARG);
    let mut subprocess_args = env::args().chain(iter::once(subprocess_flag));
    let mut subprocess = Command::new(
        subprocess_args
            .next()
            .expect("should get current executable's full path"),
    );
    subprocess.args(subprocess_args);
    if root_hash_read_from_stdin {
        subprocess.arg(root_hash);
    }

    let subprocess_output = subprocess
        .output()
        .expect("should run current executable as a subprocess");

    let stdout = String::from_utf8(subprocess_output.stdout).expect("should be valid UTF-8");
    if !subprocess_output.status.success() {
        let stderr = String::from_utf8(subprocess_output.stderr).expect("should be valid UTF-8");
        panic!(
            "\nFailed to execute as subprocess:\n{}\n\n{}\n\n",
            stdout, stderr
        );
    }

    let all_metrics = gather_metrics(stdout);
    let output_dir = &args.output_dir;
    fs::create_dir_all(output_dir)
        .unwrap_or_else(|_| panic!("should create {}", output_dir.display()));
    for (function_id, metrics_vec) in all_metrics {
        generate_csv(function_id, metrics_vec, &args.output_dir);
    }
}
