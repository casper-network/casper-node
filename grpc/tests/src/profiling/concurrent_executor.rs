//! This executable is designed to send a series of contracts in parallel to a running instance of
//! `casper-engine-grpc-server` in order to gauge the server's performance.
//!
//! For details of how to run this executable, see the README in this directory or at
//! <https://github.com/CasperLabs/casper-node/tree/master/grpc/tests/src/profiling#concurrent-executor>

use std::{
    iter::Sum,
    sync::Arc,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use clap::{crate_version, App, Arg};
use crossbeam_channel::{Iter, Receiver, Sender};
use grpc::{ClientStubExt, RequestOptions};
use log::info;

use casper_engine_grpc_server::engine_server::{
    ipc::ExecuteRequest,
    ipc_grpc::{ExecutionEngineService, ExecutionEngineServiceClient},
};
use casper_engine_test_support::internal::{
    DeployItemBuilder, ExecuteRequestBuilder, DEFAULT_PAYMENT,
};
use casper_types::{runtime_args, RuntimeArgs, U512};

use casper_engine_tests::profiling;

use crate::profiling::TransferMode;

const APP_NAME: &str = "Concurrent Executor";
const ABOUT: &str =
    "A client which constructs several 'ExecuteRequest's and sends them concurrently to the \
     Execution Engine server.\n\nFirst run the 'state-initializer' executable to set up the \
     required global state, specifying a data dir.  This outputs the pre-state hash required as an \
     input to 'concurrent-executor'.  Then run the 'casper-engine-grpc-server' specifying the \
     same data dir and a socket file.  Finally, 'concurrent-executor' can be run using the same \
     socket file.\n\nTo enable logging, set the env var 'RUST_LOG=concurrent_executor=info'.";

const SOCKET_ARG_NAME: &str = "socket";
const SOCKET_ARG_SHORT: &str = "s";
const SOCKET_ARG_VALUE_NAME: &str = "SOCKET";
const SOCKET_ARG_HELP: &str = "Path to Execution Engine server's socket file";

const PRE_STATE_HASH_ARG_NAME: &str = "pre-state-hash";
const PRE_STATE_HASH_ARG_SHORT: &str = "p";
const PRE_STATE_HASH_ARG_VALUE_NAME: &str = "HEX-ENCODED HASH";
const PRE_STATE_HASH_ARG_HELP: &str =
    "Pre-state hash; the output of running the 'state-initializer' executable";

const THREAD_COUNT_ARG_NAME: &str = "threads";
const THREAD_COUNT_ARG_SHORT: &str = "t";
const THREAD_COUNT_ARG_DEFAULT: &str = "8";
const THREAD_COUNT_ARG_VALUE_NAME: &str = "NUM";
const THREAD_COUNT_ARG_HELP: &str = "Worker thread count";

const REQUEST_COUNT_ARG_NAME: &str = "requests";
const REQUEST_COUNT_ARG_SHORT: &str = "r";
const REQUEST_COUNT_ARG_DEFAULT: &str = "100";
const REQUEST_COUNT_ARG_VALUE_NAME: &str = "NUM";
const REQUEST_COUNT_ARG_HELP: &str = "Total number of 'ExecuteRequest's to send";

const TRANSFER_MODE_ARG_NAME: &str = "transfer-mode";
const TRANSFER_MODE_ARG_SHORT: &str = "m";
const TRANSFER_MODE_ARG_DEFAULT: &str = "WASMLESS";
const TRANSFER_MODE_ARG_VALUE_NAME: &str = "&str";
const TRANSFER_MODE_ARG_HELP: &str = "Transfer mode [WASMLESS|WASM]";

const CONTRACT_NAME: &str = "transfer_to_existing_account.wasm";
const THREAD_PREFIX: &str = "client-worker-";
const ARG_AMOUNT: &str = "amount";
const ARG_TARGET: &str = "target";

fn socket_arg() -> Arg<'static, 'static> {
    Arg::with_name(SOCKET_ARG_NAME)
        .long(SOCKET_ARG_NAME)
        .short(SOCKET_ARG_SHORT)
        .required(true)
        .value_name(SOCKET_ARG_VALUE_NAME)
        .help(SOCKET_ARG_HELP)
}

fn pre_state_hash_arg() -> Arg<'static, 'static> {
    Arg::with_name(PRE_STATE_HASH_ARG_NAME)
        .long(PRE_STATE_HASH_ARG_NAME)
        .short(PRE_STATE_HASH_ARG_SHORT)
        .required(true)
        .value_name(PRE_STATE_HASH_ARG_VALUE_NAME)
        .help(PRE_STATE_HASH_ARG_HELP)
}

fn thread_count_arg() -> Arg<'static, 'static> {
    Arg::with_name(THREAD_COUNT_ARG_NAME)
        .long(THREAD_COUNT_ARG_NAME)
        .short(THREAD_COUNT_ARG_SHORT)
        .default_value(THREAD_COUNT_ARG_DEFAULT)
        .value_name(THREAD_COUNT_ARG_VALUE_NAME)
        .help(THREAD_COUNT_ARG_HELP)
}

fn request_count_arg() -> Arg<'static, 'static> {
    Arg::with_name(REQUEST_COUNT_ARG_NAME)
        .long(REQUEST_COUNT_ARG_NAME)
        .short(REQUEST_COUNT_ARG_SHORT)
        .default_value(REQUEST_COUNT_ARG_DEFAULT)
        .value_name(REQUEST_COUNT_ARG_VALUE_NAME)
        .help(REQUEST_COUNT_ARG_HELP)
}

fn transfer_mode_arg() -> Arg<'static, 'static> {
    Arg::with_name(TRANSFER_MODE_ARG_NAME)
        .long(TRANSFER_MODE_ARG_NAME)
        .short(TRANSFER_MODE_ARG_SHORT)
        .default_value(TRANSFER_MODE_ARG_DEFAULT)
        .value_name(TRANSFER_MODE_ARG_VALUE_NAME)
        .help(TRANSFER_MODE_ARG_HELP)
}

struct Args {
    socket: String,
    pre_state_hash: Vec<u8>,
    thread_count: usize,
    request_count: usize,
    transfer_mode: TransferMode,
}

impl Args {
    fn new() -> Self {
        let arg_matches = App::new(APP_NAME)
            .version(crate_version!())
            .about(ABOUT)
            .arg(socket_arg())
            .arg(pre_state_hash_arg())
            .arg(thread_count_arg())
            .arg(request_count_arg())
            .arg(transfer_mode_arg())
            .get_matches();

        let socket = arg_matches
            .value_of(SOCKET_ARG_NAME)
            .expect("Expected path to socket file")
            .to_string();
        let pre_state_hash = arg_matches
            .value_of(PRE_STATE_HASH_ARG_NAME)
            .map(profiling::parse_hash)
            .expect("Expected a pre-state hash");
        let thread_count = arg_matches
            .value_of(THREAD_COUNT_ARG_NAME)
            .map(profiling::parse_count)
            .expect("Expected thread count");
        let request_count = arg_matches
            .value_of(REQUEST_COUNT_ARG_NAME)
            .map(profiling::parse_count)
            .expect("Expected request count");
        let transfer_mode = arg_matches
            .value_of(TRANSFER_MODE_ARG_NAME)
            .map(profiling::parse_transfer_mode)
            .expect("Expected transfer mode");

        Args {
            socket,
            pre_state_hash,
            thread_count,
            request_count,
            transfer_mode,
        }
    }
}

/// Sent via a channel to the worker thread.
enum Message {
    /// Instruction to handle the wrapped request.
    Run {
        request_num: usize,
        request: ExecuteRequest,
    },
    /// Instruction to stop processing any further requests.
    Close,
}

/// A `Worker` represents a thread that waits for `ExecuteRequest`s to arrive on one channel, uses
/// the EE client to send them to the EE server, and sends the duration for receiving the response
/// on another channel.
struct Worker {
    handle: Option<JoinHandle<()>>,
}

impl Worker {
    fn new(
        id: usize,
        message_receiver: Receiver<Message>,
        result_sender: Sender<Duration>,
        client: Arc<ExecutionEngineServiceClient>,
    ) -> Self {
        let thread_name = format!("{}{}", THREAD_PREFIX, id);
        let handle = thread::Builder::new()
            .name(thread_name.clone())
            .spawn(move || loop {
                let message = message_receiver
                    .recv()
                    .expect("Expected to receive a message");
                match message {
                    Message::Run {
                        request_num,
                        request,
                    } => Self::do_work(request_num, request, &thread_name, &result_sender, &client),
                    Message::Close => {
                        break;
                    }
                };
            })
            .expect("Expected to spawn worker");

        Worker {
            handle: Some(handle),
        }
    }

    fn do_work(
        request_num: usize,
        request: ExecuteRequest,
        thread_name: &str,
        result_sender: &Sender<Duration>,
        client: &ExecutionEngineServiceClient,
    ) {
        info!(
            "Client sending 'execute' request {} on {}",
            request_num, thread_name
        );
        let start = Instant::now();
        let response = client
            .execute(RequestOptions::new(), request)
            .wait_drop_metadata()
            .expect("Expected ExecuteResponse");
        let duration = Instant::now() - start;

        let deploy_result = response
            .get_success()
            .get_deploy_results()
            .get(0)
            .expect("Expected single deploy result");
        if !deploy_result.has_execution_result() {
            panic!("Expected ExecutionResult, got {:?} instead", deploy_result);
        }
        if deploy_result.get_execution_result().has_error() {
            panic!(
                "Expected successful execution result, but instead got: {:?}",
                deploy_result.get_execution_result().get_error(),
            );
        }

        info!(
            "Client received successful response {} on {} in {:?}",
            request_num, thread_name, duration
        );

        result_sender
            .send(duration)
            .expect("Expected to send result");
    }

    /// Takes the value out of the option in the `handle` field, leaving a `None` in its place.
    fn take(&mut self) -> Option<JoinHandle<()>> {
        self.handle.take()
    }
}

/// Wrapper around `crossbeam_channel::Sender`, used to restrict its API.
pub struct MessageSender(crossbeam_channel::Sender<Message>);

impl MessageSender {
    /// Blocks the current thread until a message is sent or the channel is disconnected.
    pub fn send(&self, request_num: usize, request: ExecuteRequest) {
        self.0
            .send(Message::Run {
                request_num,
                request,
            })
            .expect("Expected to send message");
    }
}

/// Wrapper around `crossbeam_channel::Receiver`, used to restrict its API.
pub struct ResultReceiver(crossbeam_channel::Receiver<Duration>);

impl ResultReceiver {
    /// A blocking iterator over messages in the channel.
    pub fn iter(&self) -> Iter<Duration> {
        self.0.iter()
    }
}

/// A thread pool intended to run the client.
pub struct ClientPool {
    workers: Vec<Worker>,
    message_sender: crossbeam_channel::Sender<Message>,
    result_receiver: crossbeam_channel::Receiver<Duration>,
}

impl ClientPool {
    /// Creates a new thread pool with `size` worker threads associated with it.
    pub fn new(size: usize, client: Arc<ExecutionEngineServiceClient>) -> Self {
        assert!(size > 0);
        let mut workers = Vec::with_capacity(size);
        let (message_sender, message_receiver) = crossbeam_channel::unbounded();
        let (result_sender, result_receiver) = crossbeam_channel::unbounded();
        for id in 0..size {
            let message_receiver = message_receiver.clone();
            let result_sender = result_sender.clone();
            let client = Arc::clone(&client);
            workers.push(Worker::new(id, message_receiver, result_sender, client));
        }
        ClientPool {
            workers,
            message_sender,
            result_receiver,
        }
    }

    /// Returns the sending side of the channel used to convey `ExecuteRequest`s to the client.
    pub fn message_sender(&self) -> MessageSender {
        MessageSender(self.message_sender.clone())
    }

    /// Returns the receiving side of the channel used to convey `Duration`s from the worker.
    pub fn result_receiver(&self) -> ResultReceiver {
        ResultReceiver(self.result_receiver.clone())
    }
}

impl Drop for ClientPool {
    fn drop(&mut self) {
        for _ in &self.workers {
            self.message_sender
                .send(Message::Close)
                .expect("Expected to send Close");
        }

        for worker in &mut self.workers {
            if let Some(handle) = worker.take() {
                handle.join().expect("Expected to join worker");
            }
        }
    }
}

fn new_execute_request(args: &Args) -> ExecuteRequest {
    let account_1_addr = profiling::account_1_account_hash();
    let transfer_args = runtime_args! { ARG_TARGET => profiling::account_2_account_hash(), ARG_AMOUNT => U512::one() };
    let deploy_item = match args.transfer_mode {
        TransferMode::WASM => DeployItemBuilder::new()
            .with_address(account_1_addr)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT })
            .with_session_code(CONTRACT_NAME, transfer_args)
            .with_authorization_keys(&[account_1_addr])
            .build(),
        TransferMode::WASMLESS => DeployItemBuilder::new()
            .with_address(account_1_addr)
            .with_empty_payment_bytes(runtime_args! {})
            .with_transfer_args(transfer_args)
            .with_authorization_keys(&[account_1_addr])
            .build(),
    };

    ExecuteRequestBuilder::from_deploy_item(deploy_item)
        .with_pre_state_hash(&args.pre_state_hash)
        .build()
        .into()
}

fn main() {
    env_logger::init();

    let args = Args::new();
    let client_config = Default::default();
    let client = Arc::new(
        ExecutionEngineServiceClient::new_plain_unix(args.socket.as_str(), client_config)
            .expect("Expected to create Test Client"),
    );
    let pool = ClientPool::new(args.thread_count, client);

    let message_sender = pool.message_sender();
    let result_receiver = pool.result_receiver();

    let result_consumer = thread::Builder::new()
        .name("result-consumer".to_string())
        .spawn(move || result_receiver.iter().collect::<Vec<Duration>>())
        .expect("Expected to spawn result-consumer");

    let execute_request = new_execute_request(&args);
    let request_count = args.request_count;
    let start = Instant::now();
    let message_producer = thread::Builder::new()
        .name("message-producer".to_string())
        .spawn(move || {
            for request_num in 0..request_count {
                message_sender.send(request_num, execute_request.clone());
            }
        })
        .expect("Expected to spawn message-producer");

    message_producer
        .join()
        .expect("Expected to join message-producer");

    drop(pool);

    let durations = result_consumer
        .join()
        .expect("Expected to join response-consumer");
    let overall_duration = Instant::now() - start;

    assert_eq!(durations.len(), args.request_count);

    let mean_duration = Duration::sum(durations.iter()) / durations.len() as u32;
    println!(
        "Server handled {} requests in {:?} with an average response time of {:?}",
        args.request_count, overall_duration, mean_duration
    );
}
