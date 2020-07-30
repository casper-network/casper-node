mod common;
mod get_deploy;
mod keygen;
mod list_deploys;
mod put_deploy;

use clap::{crate_version, App, ArgMatches};

use get_deploy::GetDeploy;
use keygen::Keygen;
use list_deploys::ListDeploys;
use put_deploy::PutDeploy;

const APP_NAME: &str = "CasperLabs client";
const ABOUT: &str = "A client for interacting with the CasperLabs network";

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
