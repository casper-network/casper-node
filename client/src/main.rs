mod common;
mod deploy;
mod generate_completion;
mod keygen;

use clap::{crate_description, crate_version, App, ArgMatches};

use deploy::{GetDeploy, ListDeploys, PutDeploy, Transfer};
use generate_completion::GenerateCompletion;
use keygen::Keygen;

const APP_NAME: &str = "CasperLabs client";

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
    Transfer,
    GetDeploy,
    ListDeploys,
    Keygen,
    GenerateCompletion,
}

fn cli<'a, 'b>() -> App<'a, 'b> {
    App::new(APP_NAME)
        .version(crate_version!())
        .about(crate_description!())
        .subcommand(PutDeploy::build(DisplayOrder::PutDeploy as usize))
        .subcommand(Transfer::build(DisplayOrder::Transfer as usize))
        .subcommand(GetDeploy::build(DisplayOrder::GetDeploy as usize))
        .subcommand(ListDeploys::build(DisplayOrder::ListDeploys as usize))
        .subcommand(Keygen::build(DisplayOrder::Keygen as usize))
        .subcommand(GenerateCompletion::build(
            DisplayOrder::GenerateCompletion as usize,
        ))
}

#[tokio::main]
async fn main() {
    let arg_matches = cli().get_matches();
    match arg_matches.subcommand() {
        (PutDeploy::NAME, Some(matches)) => PutDeploy::run(matches),
        (Transfer::NAME, Some(matches)) => Transfer::run(matches),
        (GetDeploy::NAME, Some(matches)) => GetDeploy::run(matches),
        (ListDeploys::NAME, Some(matches)) => ListDeploys::run(matches),
        (Keygen::NAME, Some(matches)) => Keygen::run(matches),
        (GenerateCompletion::NAME, Some(matches)) => GenerateCompletion::run(matches),
        _ => panic!("You must choose a subcommand to execute"),
    }
}
