use std::{
    env,
    fmt::{self, Display, Formatter},
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use indicatif::ProgressBar;
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
        default_value = "http://localhost:11101",
        about = "Specifies the host address and port for the node to be
         reached. e.g. \"http://hostname:9001\""
    )]
    server_host: String,

    #[structopt(
        short,
        long,
        about = "Specifies the block from which to start downloading state."
    )]
    highest_block: Option<BlockIdentifier>,

    #[structopt(
        required = true,
        short,
        long,
        default_value,
        possible_values = &[DOWNLOAD_TRIES, DOWNLOAD_BLOCKS, CONVERT_BLOCK_FILES],
        about = "Specify the mode of operation for this tool."
    )]
    action: Action,

    #[structopt(
        required = true,
        short,
        long,
        default_value = retrieve_state::CHAIN_DOWNLOAD_PATH,
        about = "Specify the path to the folder to be used for downloading blocks."
    )]
    chain_download_path: PathBuf,

    #[structopt(
        short,
        long,
        default_value = retrieve_state::LMDB_PATH,
        about = "Specify the path to the folder containing the trie LMDB data files."
    )]
    lmdb_path: PathBuf,

    #[structopt(
        long,
        default_value = retrieve_state::ROCKSDB_PATH,
        about = "Specify the path to the folder containing the trie rocksdb data files."
    )]
    rocksdb_path: PathBuf,

    #[structopt(
        short,
        long,
        about = "Specify the max size (in bytes) that the underlying LMDB can grow to."
    )]
    max_db_size: Option<usize>,

    #[structopt(short, long)]
    use_gzip: bool,

    #[structopt(
        short,
        long,
        about = "Specify the path to the folder to be used for loading block files to be converted."
    )]
    block_file_download_path: Option<PathBuf>,

    #[structopt(short, long, about = "Enable manual syncing after each block to LMDB")]
    manual_sync_enabled: bool,
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
    let url = format!("{}/rpc", opts.server_host);

    let maybe_highest_block = opts.highest_block;
    let maybe_download_block = opts
        .highest_block
        .map(|block_identifier| GetBlockParams { block_identifier });

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
            let progress = ProgressBar::new(block_files.len() as u64);
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
                progress.inc(1);
            }
            println!(
                "Converted and loaded {} blocks, with {} blocks already present in db.",
                blocks_converted, blocks_already_present
            );
        }
        Action::DownloadBlocks => {
            let mut client = ClientBuilder::new().gzip(opts.use_gzip).build().unwrap();
            if !chain_download_path.exists() {
                println!(
                    "creating new chain download data dir {}",
                    chain_download_path.display()
                );
                fs::create_dir_all(&chain_download_path)?;
            }
            let highest_block: JsonBlock =
                retrieve_state::get_block(&mut client, &url, maybe_download_block)
                    .await?
                    .block
                    .unwrap();
            let progress = ProgressBar::new(highest_block.header.height);

            let mut storage = create_storage(&chain_download_path).expect("should create storage");
            println!("Downloading all blocks to genesis...");
            let (downloaded, read_from_disk) = retrieve_state::download_or_read_blocks(
                &mut client,
                &mut storage,
                &url,
                maybe_highest_block.as_ref(),
                || progress.inc(1),
            )
            .await?;
            println!(
                "Downloaded {} blocks, read {} already-downloaded blocks from disk.",
                downloaded, read_from_disk
            );
        }
        Action::DownloadTries => {
            let mut client = ClientBuilder::new().gzip(opts.use_gzip).build().unwrap();
            let highest_block: JsonBlock =
                retrieve_state::get_block(&mut client, &url, maybe_download_block)
                    .await?
                    .block
                    .unwrap();
            let progress = ProgressBar::new(highest_block.header.height);
            println!(
                "retrieving global state at height {}...",
                highest_block.header.height
            );
            let lmdb_path = env::current_dir()?.join(opts.lmdb_path);
            let rocksdb_path = env::current_dir()?.join(opts.rocksdb_path);
            let (engine_state, _environment) = retrieve_state::storage::create_execution_engine(
                lmdb_path,
                rocksdb_path,
                opts.max_db_size
                    .unwrap_or(retrieve_state::DEFAULT_MAX_DB_SIZE),
                opts.manual_sync_enabled,
            )?;
            retrieve_state::download_trie(
                &mut client,
                &url,
                &engine_state,
                highest_block.header.state_root_hash,
                || progress.inc(1),
            )
            .await
            .expect("should download trie");
            println!("Finished downloading global state.");
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
