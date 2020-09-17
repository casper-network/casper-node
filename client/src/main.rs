mod balance;
mod command;
mod common;
mod deploy;
mod error;
mod generate_completion;
mod keygen;
mod params;
mod query_state;
mod rpc;

use clap::{crate_description, crate_version, App};

use balance::GetBalance;
use command::ClientCommand;
use deploy::{GetDeploy, ListDeploys, PutDeploy, Transfer};
use error::{Error, Result};
use generate_completion::GenerateCompletion;
use keygen::Keygen;
use query_state::QueryState;
use rpc::RpcClient;

const APP_NAME: &str = "Casper client";

/// This struct defines the order in which the subcommands are shown in the app's help message.
enum DisplayOrder {
    PutDeploy,
    Transfer,
    GetDeploy,
    ListDeploys,
    GetBalance,
    QueryState,
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
        .subcommand(GetBalance::build(DisplayOrder::GetBalance as usize))
        .subcommand(QueryState::build(DisplayOrder::QueryState as usize))
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
        (GetBalance::NAME, Some(matches)) => GetBalance::run(matches),
        (QueryState::NAME, Some(matches)) => QueryState::run(matches),
        (Keygen::NAME, Some(matches)) => Keygen::run(matches),
        (GenerateCompletion::NAME, Some(matches)) => GenerateCompletion::run(matches),
        _ => panic!("You must choose a subcommand to execute"),
    }
}
