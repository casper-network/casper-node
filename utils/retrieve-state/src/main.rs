use std::{
    env,
    fmt::{self, Display, Formatter},
    fs,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::PathBuf,
    str::FromStr,
};

use reqwest::ClientBuilder;
use structopt::StructOpt;

use casper_node::{
    rpcs::chain::{BlockIdentifier, GetBlockParams},
    types::JsonBlock,
};

use retrieve_state::{address_to_url, storage::create_storage};
use tracing::info;

use casper_types::EraId;

const DOWNLOAD_TRIES: &str = "download-tries";
const DOWNLOAD_BLOCKS: &str = "download-blocks";
const CONVERT_BLOCK_FILES: &str = "convert-block-files";
const DEFAULT_PEER_MAILBOX_SIZE: usize = 1;

#[derive(Debug, StructOpt)]
struct Opts {
    #[structopt(
        short = "n",
        default_value = "127.0.0.1",
        about = "Specifies the host address for the node to be
         reached. e.g. \"127.0.0.1\""
    )]
    server_ip: Ipv4Addr,

    #[structopt(
        long = "port",
        about = "Port to reach peer RPC. For --port-derived-from-peers, this is used as the base for node-1."
    )]
    port: Option<u16>,

    #[structopt(
        short = "s",
        long,
        about = "Block from which to start downloading state. \
            Can be either a block hash or a height, where a height is considered not cryptographically secure. \
            Defaults to the highest block (by height) that the node contacted knows about."
    )]
    highest_block: Option<BlockIdentifier>,

    #[structopt(
        required = true,
        short = "a",
        long,
        default_value,
        possible_values = &[DOWNLOAD_TRIES, DOWNLOAD_BLOCKS, CONVERT_BLOCK_FILES],
        about = "Specify the mode of operation for this tool."
    )]
    action: Action,

    #[structopt(
        required = true,
        long,
        default_value = retrieve_state::CHAIN_DOWNLOAD_PATH,
        about = "Specify the path to the folder to be used for downloading blocks."
    )]
    chain_download_path: PathBuf,

    #[structopt(
        long,
        default_value = retrieve_state::LMDB_PATH,
        about = "Specify the path to the folder containing the trie LMDB data files."
    )]
    lmdb_path: PathBuf,

    #[structopt(
        long,
        about = "Specify the max size (in bytes) that the underlying LMDB can grow to."
    )]
    max_db_size: Option<usize>,

    #[structopt(long, about = "Disable gzip for requests.")]
    disable_gzip: bool,

    #[structopt(
        long,
        about = "Disable manual syncing after each block to LMDB (on by default)."
    )]
    disable_manual_sync: bool,

    #[structopt(
        long,
        about = "Derive the port from peer's address. Useful for nctl-based networks that have different RPC ports."
    )]
    port_derived_from_peers: bool,

    #[structopt(
        long,
        about = "Max number of peers to download tries from in parallel.",
        default_value = "25"
    )]
    max_workers: usize,

    #[structopt(
        long,
        about = "Use channels-based download-tries method, defaults to false (work_queue)."
    )]
    use_download_trie_channels: bool,
}

#[derive(Debug)]
enum Action {
    DownloadTries,
    DownloadBlocks,
}

impl Default for Action {
    fn default() -> Self {
        Action::DownloadTries
    }
}

impl FromStr for Action {
    type Err = anyhow::Error;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let value = match input {
            DOWNLOAD_TRIES => Self::DownloadTries,
            DOWNLOAD_BLOCKS => Self::DownloadBlocks,
            _ => {
                return Err(anyhow::Error::msg(format!(
                    "should be one of '{}' or '{}'.",
                    DOWNLOAD_TRIES, DOWNLOAD_BLOCKS,
                )))
            }
        };
        Ok(value)
    }
}

impl Display for Action {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Action::DownloadTries => formatter.write_str(DOWNLOAD_TRIES),
            Action::DownloadBlocks => formatter.write_str(DOWNLOAD_BLOCKS),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let opts = Opts::from_args();

    env_logger::init();

    let chain_download_path = env::current_dir()?.join(&opts.chain_download_path);

    // port used for peers as well as initial target node
    let port = opts.port.unwrap_or(7777);
    let initial_server_address = SocketAddr::V4(SocketAddrV4::new(opts.server_ip, port));
    let url = address_to_url(initial_server_address);

    // TODO: Consider reading the proper `chainspec` in the `retrieve-state` tool.
    let verifiable_chunked_hash_activation = EraId::from(0u64);

    let maybe_highest_block = opts.highest_block;
    let maybe_download_block = opts
        .highest_block
        .map(|block_identifier| GetBlockParams { block_identifier });

    let port_derived_from_peers = opts.port_derived_from_peers;

    match opts.action {
        Action::DownloadBlocks => {
            let client = ClientBuilder::new()
                .gzip(!opts.disable_gzip)
                .build()
                .unwrap();
            if !chain_download_path.exists() {
                info!(
                    "creating new chain download data dir {}",
                    chain_download_path.display()
                );
                fs::create_dir_all(&chain_download_path)?;
            }

            let mut storage =
                create_storage(&chain_download_path, verifiable_chunked_hash_activation)
                    .expect("should create storage");
            info!("Downloading all blocks to genesis...");
            let (downloaded, read_from_disk) = retrieve_state::download_or_read_blocks(
                &client,
                &mut storage,
                &url,
                maybe_highest_block.as_ref(),
            )
            .await?;
            info!(
                "Downloaded {} blocks, read {} already-downloaded blocks from disk.",
                downloaded, read_from_disk
            );
        }
        Action::DownloadTries => {
            let client = ClientBuilder::new()
                .gzip(!opts.disable_gzip)
                .build()
                .unwrap();

            let mut peers_list = retrieve_state::get_peers_list(&client, &url)
                .await?
                .peers
                .into_inner()
                .into_iter()
                .flat_map(|peer| {
                    let address = peer.address.parse::<SocketAddrV4>().ok()?;
                    // for nctl-networks we want our bound address
                    let address = if port_derived_from_peers {
                        SocketAddr::V4(SocketAddrV4::new(
                            *address.ip(),
                            address.port().saturating_sub(11000),
                        ))
                    } else {
                        // the peer's list gives us the port for the node, and instead we want to
                        // use the user-provided RPC port
                        SocketAddr::V4(SocketAddrV4::new(*address.ip(), port))
                    };
                    Some(address)
                })
                .collect::<Vec<_>>();

            // also use the server we provided initially that peers were gathered from.
            peers_list.push(initial_server_address);

            let highest_block: JsonBlock =
                retrieve_state::get_block(&client, &url, maybe_download_block)
                    .await?
                    .block
                    .expect("unable to download highest block");

            info!(
                "retrieving global state at height {}...",
                highest_block.header.height
            );
            let lmdb_path = env::current_dir()?.join(opts.lmdb_path);
            let (engine_state, _environment) = retrieve_state::storage::create_execution_engine(
                lmdb_path,
                opts.max_db_size
                    .unwrap_or(retrieve_state::DEFAULT_MAX_DB_SIZE),
                !opts.disable_manual_sync,
            )
            .expect("unable to create execution engine");
            if !opts.use_download_trie_channels {
                retrieve_state::download_trie_work_queue(
                    &client,
                    &peers_list,
                    engine_state,
                    highest_block.header.state_root_hash,
                    opts.max_workers,
                )
                .await
                .expect("should download trie");
            } else {
                retrieve_state::download_trie_channels(
                    &client,
                    &peers_list,
                    engine_state,
                    highest_block.header.state_root_hash,
                    DEFAULT_PEER_MAILBOX_SIZE,
                )
                .await
                .expect("should download trie");
            }
            info!("Finished downloading global state.");
        }
    }
    Ok(())
}
