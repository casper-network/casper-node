mod balance;
mod block;
mod command;
mod common;
mod deploy;
mod generate_completion;
mod get_state_hash;
mod keygen;
mod merkle_proofs;
mod query_state;
mod rpc;

use clap::{crate_description, crate_version, App};

use casper_node::rpcs::{
    account::PutDeploy,
    chain::{GetBlock, GetStateRootHash},
    info::GetDeploy,
    state::{GetBalance, GetItem as QueryState},
};

use deploy::{MakeDeploy, SendDeploy, SignDeploy};

use command::ClientCommand;
use deploy::{ListDeploys, Transfer};
use generate_completion::GenerateCompletion;
use keygen::Keygen;
use rpc::RpcClient;

const APP_NAME: &str = "Casper client";

/// This struct defines the order in which the subcommands are shown in the app's help message.
enum DisplayOrder {
    PutDeploy,
    MakeDeploy,
    SignDeploy,
    SendDeploy,
    Transfer,
    GetDeploy,
    GetBlock,
    ListDeploys,
    GetBalance,
    GetStateRootHash,
    QueryState,
    Keygen,
    GenerateCompletion,
}

fn cli<'a, 'b>() -> App<'a, 'b> {
    App::new(APP_NAME)
        .version(crate_version!())
        .about(crate_description!())
        .subcommand(PutDeploy::build(DisplayOrder::PutDeploy as usize))
        .subcommand(MakeDeploy::build(DisplayOrder::MakeDeploy as usize))
        .subcommand(SignDeploy::build(DisplayOrder::SignDeploy as usize))
        .subcommand(SendDeploy::build(DisplayOrder::SendDeploy as usize))
        .subcommand(Transfer::build(DisplayOrder::Transfer as usize))
        .subcommand(GetDeploy::build(DisplayOrder::GetDeploy as usize))
        .subcommand(GetBlock::build(DisplayOrder::GetBlock as usize))
        .subcommand(ListDeploys::build(DisplayOrder::ListDeploys as usize))
        .subcommand(GetBalance::build(DisplayOrder::GetBalance as usize))
        .subcommand(GetStateRootHash::build(
            DisplayOrder::GetStateRootHash as usize,
        ))
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
        (MakeDeploy::NAME, Some(matches)) => MakeDeploy::run(matches),
        (SignDeploy::NAME, Some(matches)) => SignDeploy::run(matches),
        (SendDeploy::NAME, Some(matches)) => SendDeploy::run(matches),
        (Transfer::NAME, Some(matches)) => Transfer::run(matches),
        (GetDeploy::NAME, Some(matches)) => GetDeploy::run(matches),
        (GetBlock::NAME, Some(matches)) => GetBlock::run(matches),
        (ListDeploys::NAME, Some(matches)) => ListDeploys::run(matches),
        (GetBalance::NAME, Some(matches)) => GetBalance::run(matches),
        (GetStateRootHash::NAME, Some(matches)) => GetStateRootHash::run(matches),
        (QueryState::NAME, Some(matches)) => QueryState::run(matches),
        (Keygen::NAME, Some(matches)) => Keygen::run(matches),
        (GenerateCompletion::NAME, Some(matches)) => GenerateCompletion::run(matches),
        _ => {
            let _ = cli().print_long_help();
            println!();
        }
    }
}
