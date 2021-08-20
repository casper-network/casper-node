use structopt::StructOpt;

#[derive(StructOpt, Debug)]
pub(crate) struct Args {
    /// Node address
    #[structopt(short, long, default_value = "127.0.0.1:11101")]
    pub(crate) node_address: String,

    /// RPC api path
    #[structopt(short, long, default_value = "/rpc")]
    pub(crate) api_path: String,

    /// Directory containing the test specification
    #[structopt(short, long)]
    pub(crate) test_directory: String,
}
