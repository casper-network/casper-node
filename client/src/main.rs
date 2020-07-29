mod common;
mod get_deploy;
mod keygen;
mod list_deploys;
mod put_deploy;

use std::convert::{TryFrom, TryInto};

use clap::{crate_version, App, ArgMatches};
use serde::{self, Deserialize};

use casperlabs_types::{bytesrepr, CLValue, NamedArg, RuntimeArgs};

use get_deploy::GetDeploy;
use keygen::Keygen;
use list_deploys::ListDeploys;
use put_deploy::PutDeploy;

const APP_NAME: &str = "CasperLabs client";
const ABOUT: &str = "A client for interacting with the CasperLabs network";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DeployArgValue {
    /// Contains `CLValue` serialized into bytes in base16 form.
    #[serde(deserialize_with = "hex::deserialize")]
    RawBytes(Vec<u8>),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct DeployArg {
    /// Deploy argument's name.
    name: String,
    value: DeployArgValue,
}

impl TryFrom<DeployArgValue> for CLValue {
    type Error = bytesrepr::Error;
    fn try_from(value: DeployArgValue) -> Result<Self, Self::Error> {
        match value {
            DeployArgValue::RawBytes(bytes) => bytesrepr::deserialize(bytes),
        }
    }
}

impl TryFrom<DeployArg> for NamedArg {
    type Error = bytesrepr::Error;

    fn try_from(deploy_arg: DeployArg) -> Result<Self, Self::Error> {
        let cl_value = deploy_arg.value.try_into()?;
        Ok(NamedArg::new(deploy_arg.name, cl_value))
    }
}

pub trait Subcommand<'a, 'b> {
    const NAME: &'static str;
    const ABOUT: &'static str;
    /// Constructs the clap `SubCommand` and returns the clap `App`.
    fn build(display_order: usize) -> App<'a, 'b>;
    /// Parses the arg matches and runs the subcommand.
    fn run(matches: &ArgMatches<'_>);
}

/// This struct defines the order in which the subcommands are shown in the app's help message.
enum DisplayOrder {
    PutDeploy,
    GetDeploy,
    ListDeploys,
    Keygen,
}

#[tokio::main]
async fn main() {
    let arg_matches = App::new(APP_NAME)
        .version(crate_version!())
        .about(ABOUT)
        .set_term_width(0)
        .subcommand(PutDeploy::build(DisplayOrder::PutDeploy as usize))
        .subcommand(GetDeploy::build(DisplayOrder::GetDeploy as usize))
        .subcommand(ListDeploys::build(DisplayOrder::ListDeploys as usize))
        .subcommand(Keygen::build(DisplayOrder::Keygen as usize))
        .get_matches();
    match arg_matches.subcommand() {
        (PutDeploy::NAME, Some(matches)) => PutDeploy::run(matches),
        (GetDeploy::NAME, Some(matches)) => GetDeploy::run(matches),
        (ListDeploys::NAME, Some(matches)) => ListDeploys::run(matches),
        (Keygen::NAME, Some(matches)) => Keygen::run(matches),
        _ => panic!("You must choose a subcommand to execute"),
    }
}

// fn get_runtime_args_from_file(filename: PathBuf) -> anyhow::Result<RuntimeArgs> {
//     let reader = File::open(&filename)?;
//     // Received structured args in json format.
//     let args: Vec<DeployArg> = serde_json::from_reader(reader)?;
//     // Convert JSON deploy args into vector of named args.
//     let maybe_named_args: Result<Vec<NamedArg>, _> =
//         args.into_iter().map(TryInto::try_into).collect();
//     let named_args = maybe_named_args
//         .map_err(Error::msg)
//         .with_context(|| format!("trying to deserialize args from {}", filename.display()))?;
//     Ok(RuntimeArgs::from(named_args))
// }
