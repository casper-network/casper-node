use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use clap::{App, Arg, ArgMatches};
use dirs::home_dir;
use lmdb::DatabaseFlags;
use log::{error, info, Level, LevelFilter};

use casper_engine_grpc_server::engine_server;
use casper_execution_engine::{
    core::engine_state::{EngineConfig, EngineState},
    shared::{
        logging::{self, Settings, Style},
        socket,
        utils::OS_PAGE_SIZE,
    },
    storage::{
        global_state::lmdb::LmdbGlobalState, protocol_data_store::lmdb::LmdbProtocolDataStore,
        transaction_source::lmdb::LmdbEnvironment, trie_store::lmdb::LmdbTrieStore,
    },
};

// exe / proc
const PROC_NAME: &str = "casper-engine-grpc-server";
const APP_NAME: &str = "Casper Execution Engine Server";
const SERVER_LISTENING_TEMPLATE: &str = "{listener} is listening on socket: {socket}";
const SERVER_START_EXPECT: &str = "failed to start Execution Engine Server";

// data-dir / lmdb
const ARG_DATA_DIR: &str = "data-dir";
const ARG_DATA_DIR_SHORT: &str = "d";
const ARG_DATA_DIR_VALUE: &str = "DIR";
const ARG_DATA_DIR_HELP: &str = "Sets the data directory";
const DEFAULT_DATA_DIR_RELATIVE: &str = ".casperlabs";
const GLOBAL_STATE_DIR: &str = "global_state";
const GET_HOME_DIR_EXPECT: &str = "Could not get home directory";
const CREATE_DATA_DIR_EXPECT: &str = "Could not create directory";
const LMDB_ENVIRONMENT_EXPECT: &str = "Could not create LmdbEnvironment";
const LMDB_TRIE_STORE_EXPECT: &str = "Could not create LmdbTrieStore";
const LMDB_PROTOCOL_DATA_STORE_EXPECT: &str = "Could not create LmdbProtocolDataStore";
const LMDB_GLOBAL_STATE_EXPECT: &str = "Could not create LmdbGlobalState";

// pages / lmdb
const ARG_PAGES: &str = "pages";
const ARG_PAGES_SHORT: &str = "p";
const ARG_PAGES_VALUE: &str = "NUM";
const ARG_PAGES_HELP: &str = "Sets the max number of pages to use for lmdb's mmap";
const GET_PAGES_EXPECT: &str = "Could not parse pages argument";
// 750 GiB = 805306368000 bytes
// page size on x86_64 linux = 4096 bytes
// 805306368000 / 4096 = 196608000
const DEFAULT_PAGES: usize = 196_608_000;

// max_readers / lmdb
const ARG_MAX_READERS: &str = "max-readers";
const ARG_MAX_READERS_SHORT: &str = "r";
const ARG_MAX_READERS_VALUE: &str = "NUM";
const ARG_MAX_READERS_HELP: &str = "Sets the max number of readers to use for lmdb";
const GET_MAX_READERS_EXPECT: &str = "Could not parse max-readers argument";
const DEFAULT_MAX_READERS: u32 = 512;

// socket
const ARG_SOCKET: &str = "socket";
const ARG_SOCKET_HELP: &str =
    "Path to socket.  Note that this path is independent of the data directory.";
const ARG_SOCKET_EXPECT: &str = "socket required";

// log level
const ARG_LOG_LEVEL: &str = "log-level";
const ARG_LOG_LEVEL_VALUE: &str = "LEVEL";
const ARG_LOG_LEVEL_HELP: &str = "Sets the max logging level";
const LOG_LEVEL_OFF: &str = "off";
const LOG_LEVEL_ERROR: &str = "error";
const LOG_LEVEL_WARN: &str = "warn";
const LOG_LEVEL_INFO: &str = "info";
const LOG_LEVEL_DEBUG: &str = "debug";
const LOG_LEVEL_TRACE: &str = "trace";

// metrics
const ARG_LOG_METRICS: &str = "log-metrics";
const ARG_LOG_METRICS_HELP: &str = "Enables logging of metrics regardless of log-level setting";

// log style
const ARG_LOG_STYLE: &str = "log-style";
const ARG_LOG_STYLE_VALUE: &str = "STYLE";
const ARG_LOG_STYLE_HELP: &str = "Sets logging style to structured or human-readable";
const LOG_STYLE_STRUCTURED: &str = "structured";
const LOG_STYLE_HUMAN_READABLE: &str = "human";

// thread count
const ARG_THREAD_COUNT: &str = "threads";
const ARG_THREAD_COUNT_SHORT: &str = "t";
const ARG_THREAD_COUNT_DEFAULT: &str = "1";
const ARG_THREAD_COUNT_VALUE: &str = "NUM";
const ARG_THREAD_COUNT_HELP: &str = "Worker thread count";
const ARG_THREAD_COUNT_EXPECT: &str = "expected valid thread count";

// use system contracts
const ARG_USE_SYSTEM_CONTRACTS: &str = "use-system-contracts";
const ARG_USE_SYSTEM_CONTRACTS_SHORT: &str = "z";
const ARG_USE_SYSTEM_CONTRACTS_HELP: &str =
    "Use system contracts instead of host-side logic for Mint, Proof of Stake and Standard Payment";

// runnable
const SIGINT_HANDLE_EXPECT: &str = "Error setting Ctrl-C handler";
const RUNNABLE_CHECK_INTERVAL_SECONDS: u64 = 3;

fn main() {
    set_panic_hook();

    let arg_matches = get_args();

    let _ = logging::initialize(get_log_settings(&arg_matches));

    info!("starting Execution Engine Server");

    let socket = get_socket(&arg_matches);

    match socket.remove_file() {
        Err(e) => panic!("failed to remove old socket file: {:?}", e),
        Ok(_) => info!("removing old socket file"),
    };

    let data_dir = get_data_dir(&arg_matches);

    let map_size = get_map_size(&arg_matches);

    let max_readers = get_max_readers(&arg_matches);

    let thread_count = get_thread_count(&arg_matches);

    let engine_config: EngineConfig = get_engine_config(&arg_matches);

    let _server = get_grpc_server(
        &socket,
        data_dir,
        map_size,
        max_readers,
        thread_count,
        engine_config,
    );

    log_listening_message(&socket);

    let interval = Duration::from_secs(RUNNABLE_CHECK_INTERVAL_SECONDS);

    let runnable = get_sigint_handle();

    while runnable.load(Ordering::SeqCst) {
        std::thread::park_timeout(interval);
    }

    info!("stopping Execution Engine Server");
}

/// Sets panic hook for logging panic info
fn set_panic_hook() {
    let hook: Box<dyn Fn(&std::panic::PanicInfo) + 'static + Sync + Send> = Box::new(
        move |panic_info| match panic_info.payload().downcast_ref::<&str>() {
            Some(s) => {
                error!("{:?}", s);
            }
            None => {
                error!("{:?}", panic_info);
            }
        },
    );
    std::panic::set_hook(hook);
}

/// Gets command line arguments
fn get_args() -> ArgMatches<'static> {
    App::new(APP_NAME)
        .version(env!("CARGO_PKG_VERSION"))
        .arg(
            Arg::with_name(ARG_LOG_LEVEL)
                .required(false)
                .long(ARG_LOG_LEVEL)
                .takes_value(true)
                .possible_value(LOG_LEVEL_OFF)
                .possible_value(LOG_LEVEL_ERROR)
                .possible_value(LOG_LEVEL_WARN)
                .possible_value(LOG_LEVEL_INFO)
                .possible_value(LOG_LEVEL_DEBUG)
                .possible_value(LOG_LEVEL_TRACE)
                .default_value(LOG_LEVEL_INFO)
                .value_name(ARG_LOG_LEVEL_VALUE)
                .help(ARG_LOG_LEVEL_HELP),
        )
        .arg(
            Arg::with_name(ARG_LOG_METRICS)
                .required(false)
                .long(ARG_LOG_METRICS)
                .takes_value(false)
                .help(ARG_LOG_METRICS_HELP),
        )
        .arg(
            Arg::with_name(ARG_LOG_STYLE)
                .required(false)
                .long(ARG_LOG_STYLE)
                .takes_value(true)
                .possible_value(LOG_STYLE_STRUCTURED)
                .possible_value(LOG_STYLE_HUMAN_READABLE)
                .default_value(LOG_STYLE_STRUCTURED)
                .value_name(ARG_LOG_STYLE_VALUE)
                .help(ARG_LOG_STYLE_HELP),
        )
        .arg(
            Arg::with_name(ARG_DATA_DIR)
                .short(ARG_DATA_DIR_SHORT)
                .long(ARG_DATA_DIR)
                .value_name(ARG_DATA_DIR_VALUE)
                .help(ARG_DATA_DIR_HELP)
                .takes_value(true),
        )
        .arg(
            Arg::with_name(ARG_PAGES)
                .short(ARG_PAGES_SHORT)
                .long(ARG_PAGES)
                .value_name(ARG_PAGES_VALUE)
                .help(ARG_PAGES_HELP)
                .takes_value(true),
        )
        .arg(
            Arg::with_name(ARG_MAX_READERS)
                .short(ARG_MAX_READERS_SHORT)
                .long(ARG_MAX_READERS)
                .value_name(ARG_MAX_READERS_VALUE)
                .help(ARG_MAX_READERS_HELP)
                .takes_value(true),
        )
        .arg(
            Arg::with_name(ARG_THREAD_COUNT)
                .short(ARG_THREAD_COUNT_SHORT)
                .long(ARG_THREAD_COUNT)
                .takes_value(true)
                .default_value(ARG_THREAD_COUNT_DEFAULT)
                .value_name(ARG_THREAD_COUNT_VALUE)
                .help(ARG_THREAD_COUNT_HELP),
        )
        .arg(
            Arg::with_name(ARG_USE_SYSTEM_CONTRACTS)
                .short(ARG_USE_SYSTEM_CONTRACTS_SHORT)
                .long(ARG_USE_SYSTEM_CONTRACTS)
                .help(ARG_USE_SYSTEM_CONTRACTS_HELP),
        )
        .arg(
            Arg::with_name(ARG_SOCKET)
                .required(true)
                .help(ARG_SOCKET_HELP)
                .index(1),
        )
        .get_matches()
}

/// Gets SIGINT handle to allow clean exit
fn get_sigint_handle() -> Arc<AtomicBool> {
    let handle = Arc::new(AtomicBool::new(true));
    let h = handle.clone();
    ctrlc::set_handler(move || {
        h.store(false, Ordering::SeqCst);
    })
    .expect(SIGINT_HANDLE_EXPECT);
    handle
}

/// Gets value of socket argument
fn get_socket(arg_matches: &ArgMatches) -> socket::Socket {
    let socket = arg_matches.value_of(ARG_SOCKET).expect(ARG_SOCKET_EXPECT);

    socket::Socket::new(socket.to_owned())
}

/// Gets value of data-dir argument
fn get_data_dir(arg_matches: &ArgMatches) -> PathBuf {
    let mut buf = arg_matches.value_of(ARG_DATA_DIR).map_or(
        {
            let mut dir = home_dir().expect(GET_HOME_DIR_EXPECT);
            dir.push(DEFAULT_DATA_DIR_RELATIVE);
            dir
        },
        PathBuf::from,
    );
    buf.push(GLOBAL_STATE_DIR);
    fs::create_dir_all(&buf).unwrap_or_else(|_| panic!("{}: {:?}", CREATE_DATA_DIR_EXPECT, buf));
    buf
}

///  Parses pages argument and returns map size
fn get_map_size(arg_matches: &ArgMatches) -> usize {
    let page_size = *OS_PAGE_SIZE;
    let pages = arg_matches
        .value_of(ARG_PAGES)
        .map_or(Ok(DEFAULT_PAGES), usize::from_str)
        .expect(GET_PAGES_EXPECT);
    page_size * pages
}

/// Gets value of max readers argument
fn get_max_readers(arg_matches: &ArgMatches) -> u32 {
    let max_readers = arg_matches
        .value_of(ARG_MAX_READERS)
        .map_or(Ok(DEFAULT_MAX_READERS), u32::from_str)
        .expect(GET_MAX_READERS_EXPECT);
    max_readers
}

fn get_thread_count(arg_matches: &ArgMatches) -> usize {
    arg_matches
        .value_of(ARG_THREAD_COUNT)
        .map(str::parse)
        .expect(ARG_THREAD_COUNT_EXPECT)
        .expect(ARG_THREAD_COUNT_EXPECT)
}

/// Returns an [`EngineConfig`].
fn get_engine_config(arg_matches: &ArgMatches) -> EngineConfig {
    // feature flags go here
    let use_system_contracts = arg_matches.is_present(ARG_USE_SYSTEM_CONTRACTS);
    EngineConfig::new().with_use_system_contracts(use_system_contracts)
}

/// Builds and returns a gRPC server.
fn get_grpc_server(
    socket: &socket::Socket,
    data_dir: PathBuf,
    map_size: usize,
    max_readers: u32,
    thread_count: usize,
    engine_config: EngineConfig,
) -> grpc::Server {
    let engine_state = get_engine_state(data_dir, map_size, max_readers, engine_config);

    engine_server::new(socket.as_str(), thread_count, engine_state)
        .build()
        .expect(SERVER_START_EXPECT)
}

/// Builds and returns engine global state
fn get_engine_state(
    data_dir: PathBuf,
    map_size: usize,
    max_readers: u32,
    engine_config: EngineConfig,
) -> EngineState<LmdbGlobalState> {
    let environment = {
        let ret =
            LmdbEnvironment::new(&data_dir, map_size, max_readers).expect(LMDB_ENVIRONMENT_EXPECT);
        Arc::new(ret)
    };

    let trie_store = {
        let ret = LmdbTrieStore::new(&environment, None, DatabaseFlags::empty())
            .expect(LMDB_TRIE_STORE_EXPECT);
        Arc::new(ret)
    };

    let protocol_data_store = {
        let ret = LmdbProtocolDataStore::new(&environment, None, DatabaseFlags::empty())
            .expect(LMDB_PROTOCOL_DATA_STORE_EXPECT);
        Arc::new(ret)
    };

    let global_state = LmdbGlobalState::empty(environment, trie_store, protocol_data_store)
        .expect(LMDB_GLOBAL_STATE_EXPECT);

    EngineState::new(global_state, engine_config)
}

/// Builds and returns log settings
fn get_log_settings(arg_matches: &ArgMatches) -> Settings {
    let max_level = match arg_matches
        .value_of(ARG_LOG_LEVEL)
        .expect("should have default value if not explicitly set")
    {
        LOG_LEVEL_OFF => LevelFilter::Off,
        LOG_LEVEL_ERROR => LevelFilter::Error,
        LOG_LEVEL_WARN => LevelFilter::Warn,
        LOG_LEVEL_INFO => LevelFilter::Info,
        LOG_LEVEL_DEBUG => LevelFilter::Debug,
        LOG_LEVEL_TRACE => LevelFilter::Trace,
        _ => unreachable!("should validate log-level arg to match one of the options"),
    };

    let enable_metrics = arg_matches.is_present(ARG_LOG_METRICS);

    let style = match arg_matches.value_of(ARG_LOG_STYLE) {
        Some(LOG_STYLE_HUMAN_READABLE) => Style::HumanReadable,
        _ => Style::Structured,
    };

    Settings::new(max_level)
        .with_metrics_enabled(enable_metrics)
        .with_style(style)
}

/// Logs listening on socket message
fn log_listening_message(socket: &socket::Socket) {
    let mut properties = BTreeMap::new();
    properties.insert("listener", PROC_NAME.to_owned());
    properties.insert("socket", socket.value());

    logging::log_details(
        Level::Info,
        (&*SERVER_LISTENING_TEMPLATE).to_string(),
        properties,
    );
}
