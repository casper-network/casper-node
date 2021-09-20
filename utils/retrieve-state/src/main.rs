use std::{
    env,
    fmt::{self, Display, Formatter},
    str::FromStr,
};

use reqwest::ClientBuilder;
use structopt::StructOpt;

use casper_node::{
    rpcs::chain::{BlockIdentifier, GetBlockParams},
    types::JsonBlock,
};

const DOWNLOAD_TRIES: &str = "download-tries";
const DOWNLOAD_BLOCKS: &str = "download-blocks";

#[derive(Debug, StructOpt)]
struct Opts {
    #[structopt(
        short = "n",
        default_value = "http://localhost:11101",
        about = "Specifies the host address and port for the node to be
         reached. e.g. \"http://hosname:9001\""
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
        possible_values = &[DOWNLOAD_TRIES, DOWNLOAD_BLOCKS],
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
    chain_download_path: String,

    #[structopt(
        short,
        long,
        default_value = retrieve_state::LMDB_PATH,
        about = "Specify the path to the folder containing the underlying LMDB data files."
    )]
    lmdb_path: String,

    #[structopt(
        short,
        long,
        about = "Specify the max size (in bytes) that the underlying LMDB can grow to."
    )]
    max_db_size: Option<usize>,

    #[structopt(short, long)]
    use_gzip: bool,
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
                    DOWNLOAD_TRIES, DOWNLOAD_BLOCKS
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
    let mut client = ClientBuilder::new().gzip(opts.use_gzip).build().unwrap();
    let chain_download_path = env::current_dir()?.join(&opts.chain_download_path);
    let url = format!("{}/rpc", opts.server_host);

    let maybe_download_block = opts
        .highest_block
        .map(|block_identifier| GetBlockParams { block_identifier });

    let highest_block: JsonBlock =
        retrieve_state::get_block(&mut client, &url, maybe_download_block)
            .await?
            .block
            .unwrap();

    match opts.action {
        Action::DownloadBlocks => {
            println!("Downloading all blocks to genesis...");
            let _block_files = retrieve_state::download_blocks(
                &mut client,
                &url,
                &chain_download_path,
                opts.highest_block.as_ref(),
            )
            .await?;
            println!("Finished downloding blocks.")
        }
        Action::DownloadTries => {
            println!(
                "retrieving global state at height {}...",
                highest_block.header.height
            );
            let lmdb_path = env::current_dir()?.join(opts.lmdb_path);
            let engine_state = retrieve_state::offline::create_execution_engine(
                lmdb_path,
                opts.max_db_size
                    .unwrap_or(retrieve_state::DEFAULT_MAX_DB_SIZE),
            )?;
            retrieve_state::download_trie(
                &mut client,
                &url,
                &engine_state,
                highest_block.header.state_root_hash,
            )
            .await
            .expect("should download trie");
            println!("Finished downloading global state.");
        }
    }
    Ok(())
}
