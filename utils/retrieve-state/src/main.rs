use std::{
    env,
    fmt::{self, Display, Formatter},
    fs,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

use reqwest::ClientBuilder;
use retrieve_state::{put_block_with_deploys, BlockWithDeploys};
use structopt::StructOpt;
use tokio::{fs::File, io::AsyncReadExt};
use walkdir::DirEntry;

use casper_node::{
    rpcs::chain::{BlockIdentifier, GetBlockParams},
    types::JsonBlock,
};

use retrieve_state::storage::create_storage;

const DOWNLOAD_TRIES: &str = "download-tries";
const DOWNLOAD_BLOCKS: &str = "download-blocks";
const CONVERT_BLOCK_FILES: &str = "convert-block-files";

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
        about = "Specifies the port to reach peer RPC endpoints at."
    )]
    port: Option<u16>,

    #[structopt(
        short = "s",
        long,
        about = "Specifies the block from which to start downloading state."
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

    #[structopt(short, long)]
    use_gzip: bool,

    #[structopt(
        short = "b",
        long,
        about = "Specify the path to the folder to be used for loading block files to be converted."
    )]
    block_file_download_path: Option<PathBuf>,

    #[structopt(long, about = "Enable manual syncing after each block to LMDB")]
    manual_sync_enabled: Option<bool>,

    #[structopt(
        long,
        about = "Derive the port from peer's address. Useful for nctl-based networks that have different RPC ports."
    )]
    port_derived_from_peers: bool,
}

#[derive(Debug)]
enum Action {
    DownloadTries,
    DownloadBlocks,
    ConvertBlockFiles,
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
            CONVERT_BLOCK_FILES => Self::ConvertBlockFiles,
            _ => {
                return Err(anyhow::Error::msg(format!(
                    "should be one of '{}', '{}, or '{}'.",
                    DOWNLOAD_TRIES, DOWNLOAD_BLOCKS, CONVERT_BLOCK_FILES,
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
            Action::ConvertBlockFiles => formatter.write_str(CONVERT_BLOCK_FILES),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let opts = Opts::from_args();

    let chain_download_path = env::current_dir()?.join(&opts.chain_download_path);

    // port used for peers as well as initial target node
    let port = opts.port.unwrap_or(7777);
    let initial_server_address = SocketAddr::V4(SocketAddrV4::new(opts.server_ip, port));
    let url = format!(
        "http://{}:{}/rpc",
        initial_server_address.ip(),
        initial_server_address.port()
    );

    let maybe_highest_block = opts.highest_block;
    let maybe_download_block = opts
        .highest_block
        .map(|block_identifier| GetBlockParams { block_identifier });

    let port_derived_from_peers = opts.port_derived_from_peers;

    match opts.action {
        Action::ConvertBlockFiles => {
            if !chain_download_path.exists() {
                println!(
                    "creating new chain download data dir {}",
                    chain_download_path.display()
                );
                fs::create_dir_all(&chain_download_path)?;
            }
            let block_file_download_path = match opts.block_file_download_path {
                None => {
                    return Err(anyhow::anyhow!("--block-file-download-path <path> is required for action 'convert-block-files'"))
                }
                Some(path) => path
            };
            let mut storage = create_storage(&chain_download_path).expect("should create storage");
            let block_files = get_block_files(&block_file_download_path);
            println!(
                "loading {} blocks from {} into db at {}",
                block_files.len(),
                block_file_download_path.display(),
                opts.chain_download_path.display()
            );
            let mut blocks_converted = 0;
            let mut blocks_already_present = 0;
            for block_file in block_files {
                let block_with_deploys = read_block_file(&block_file).await?;
                match storage.read_block(&block_with_deploys.block.hash)? {
                    Some(_found) => blocks_already_present += 1,
                    None => {
                        put_block_with_deploys(&mut storage, &block_with_deploys)?;
                        blocks_converted += 1;
                    }
                }
            }
            println!(
                "Converted and loaded {} blocks, with {} blocks already present in db.",
                blocks_converted, blocks_already_present
            );
        }
        Action::DownloadBlocks => {
            let client = ClientBuilder::new().gzip(opts.use_gzip).build().unwrap();
            if !chain_download_path.exists() {
                println!(
                    "creating new chain download data dir {}",
                    chain_download_path.display()
                );
                fs::create_dir_all(&chain_download_path)?;
            }
            let highest_block: JsonBlock =
                retrieve_state::get_block(&client, &url, maybe_download_block)
                    .await?
                    .block
                    .unwrap();

            let mut storage = create_storage(&chain_download_path).expect("should create storage");
            println!("Downloading all blocks to genesis...");
            let (downloaded, read_from_disk) = retrieve_state::download_or_read_blocks(
                &client,
                &mut storage,
                &url,
                maybe_highest_block.as_ref(),
            )
            .await?;
            println!(
                "Downloaded {} blocks, read {} already-downloaded blocks from disk.",
                downloaded, read_from_disk
            );
        }
        Action::DownloadTries => {
            let client = ClientBuilder::new().gzip(opts.use_gzip).build().unwrap();

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

            // also use the server we provided inititially that peers were gathered from.
            peers_list.push(initial_server_address);

            let highest_block: JsonBlock =
                retrieve_state::get_block(&client, &url, maybe_download_block)
                    .await?
                    .block
                    .expect("unable to download highest block");

            eprintln!(
                "retrieving global state at height {}...",
                highest_block.header.height
            );
            let lmdb_path = env::current_dir()?.join(opts.lmdb_path);
            let (engine_state, _environment) = retrieve_state::storage::create_execution_engine(
                lmdb_path,
                opts.max_db_size
                    .unwrap_or(retrieve_state::DEFAULT_MAX_DB_SIZE),
                opts.manual_sync_enabled.unwrap_or(true),
            )
            .expect("unable to create execution engine");
            retrieve_state::download_trie(
                &client,
                &peers_list,
                engine_state,
                highest_block.header.state_root_hash,
            )
            .await
            .expect("should download trie");
            eprintln!("Finished downloading global state.");
        }
    }
    Ok(())
}

async fn read_block_file(block_file_entry: &DirEntry) -> Result<BlockWithDeploys, anyhow::Error> {
    let mut file = File::open(block_file_entry.path()).await?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).await?;
    Ok(serde_json::from_slice::<BlockWithDeploys>(&buffer)?)
}

fn get_block_files(chain_path: impl AsRef<Path>) -> Vec<DirEntry> {
    let mut block_files = walkdir::WalkDir::new(chain_path)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let file_name = entry.file_name().to_str()?;
            let split = file_name.split('-').collect::<Vec<&str>>();
            if let ["block", _height, _hash] = &split[..] {
                Some(entry)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    block_files.sort_by_key(|entry| entry.file_name().to_str().unwrap().to_string());
    block_files
}
