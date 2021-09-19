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
    #[structopt(short = "n", default_value = "http://localhost:11101")]
    server_host: String,

    #[structopt(short, long)]
    download_height: Option<u64>,

    #[structopt(
        required = true,
        short,
        long,
        default_value,
        possible_values = &[DOWNLOAD_TRIES, DOWNLOAD_BLOCKS])
    ]
    action: Action,
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
    // TODO: enable gzip if the rpc endpoint supports it.
    let mut client = ClientBuilder::new().gzip(false).build().unwrap();
    let chain_download_path = env::current_dir()?.join(retrieve_state::CHAIN_DOWNLOAD_PATH);
    let url = format!("{}/rpc", opts.server_host);

    let maybe_download_block = opts.download_height.map(|height| GetBlockParams {
        block_identifier: BlockIdentifier::Height(height),
    });

    let highest_block: JsonBlock =
        retrieve_state::get_block(&mut client, &url, maybe_download_block)
            .await?
            .block
            .unwrap();

    match opts.action {
        Action::DownloadBlocks => {
            let download_block = {
                let block_files =
                    retrieve_state::offline::get_block_files(retrieve_state::CHAIN_DOWNLOAD_PATH);
                let lowest_block_file = block_files.get(0);
                match lowest_block_file {
                    Some(lowest_block_file) => {
                        println!(
                            "found lowest block downloaded at {}",
                            lowest_block_file.path().display()
                        );
                        retrieve_state::offline::read_block_file(lowest_block_file)
                            .await?
                            .block
                    }
                    _ => highest_block,
                }
            };
            println!(
                "downloading all blocks from {} to {}...",
                download_block.header.height, 0
            );
            let _block_files = retrieve_state::download_blocks(
                &mut client,
                &url,
                &chain_download_path,
                download_block.hash,
                0,
            )
            .await?;
        }
        Action::DownloadTries => {
            println!(
                "retrieving global state at height {}...",
                highest_block.header.height
            );
            let lmdb_path = env::current_dir()?.join(retrieve_state::LMDB_PATH);
            let engine_state = retrieve_state::offline::create_execution_engine(lmdb_path)?;
            retrieve_state::download_trie(
                &mut client,
                &url,
                &engine_state,
                highest_block.header.state_root_hash,
            )
            .await
            .expect("should download trie");
            println!("finished downloading global state");
        }
    }
    Ok(())
}
