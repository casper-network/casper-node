mod account_address;
mod block;
mod command;
mod common;
mod deploy;
mod docs;
mod generate_completion;
mod get_auction_info;
mod get_balance;
mod get_era_info_by_switch_block;
mod get_state_hash;
mod keygen;
mod query_state;

use clap::{crate_description, crate_version, App};

use casper_node::rpcs::{
    account::PutDeploy,
    chain::{GetBlock, GetBlockTransfers, GetEraInfoBySwitchBlock, GetStateRootHash},
    docs::ListRpcs,
    info::GetDeploy,
    state::{GetAuctionInfo, GetBalance, GetItem as QueryState},
};

use deploy::{ListDeploys, MakeDeploy, SendDeploy, SignDeploy};

use account_address::GenerateAccountHash as AccountAddress;
use command::ClientCommand;
use deploy::Transfer;
use generate_completion::GenerateCompletion;
use keygen::Keygen;

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
    GetBlockTransfers,
    ListDeploys,
    GetStateRootHash,
    QueryState,
    GetBalance,
    GetEraInfo,
    GetAuctionInfo,
    Keygen,
    GenerateCompletion,
    GetRpcs,
    AccountAddress,
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
        .subcommand(GetBlockTransfers::build(
            DisplayOrder::GetBlockTransfers as usize,
        ))
        .subcommand(ListDeploys::build(DisplayOrder::ListDeploys as usize))
        .subcommand(GetBalance::build(DisplayOrder::GetBalance as usize))
        .subcommand(GetStateRootHash::build(
            DisplayOrder::GetStateRootHash as usize,
        ))
        .subcommand(QueryState::build(DisplayOrder::QueryState as usize))
        .subcommand(GetEraInfoBySwitchBlock::build(
            DisplayOrder::GetEraInfo as usize,
        ))
        .subcommand(GetAuctionInfo::build(DisplayOrder::GetAuctionInfo as usize))
        .subcommand(Keygen::build(DisplayOrder::Keygen as usize))
        .subcommand(GenerateCompletion::build(
            DisplayOrder::GenerateCompletion as usize,
        ))
        .subcommand(ListRpcs::build(DisplayOrder::GetRpcs as usize))
        .subcommand(AccountAddress::build(DisplayOrder::AccountAddress as usize))
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
        (GetBlockTransfers::NAME, Some(matches)) => GetBlockTransfers::run(matches),
        (ListDeploys::NAME, Some(matches)) => ListDeploys::run(matches),
        (GetBalance::NAME, Some(matches)) => GetBalance::run(matches),
        (GetStateRootHash::NAME, Some(matches)) => GetStateRootHash::run(matches),
        (QueryState::NAME, Some(matches)) => QueryState::run(matches),
        (GetEraInfoBySwitchBlock::NAME, Some(matches)) => GetEraInfoBySwitchBlock::run(matches),
        (GetAuctionInfo::NAME, Some(matches)) => GetAuctionInfo::run(matches),
        (Keygen::NAME, Some(matches)) => Keygen::run(matches),
        (GenerateCompletion::NAME, Some(matches)) => GenerateCompletion::run(matches),
        (ListRpcs::NAME, Some(matches)) => ListRpcs::run(matches),
        (AccountAddress::NAME, Some(matches)) => AccountAddress::run(matches),
        _ => {
            let _ = cli().print_long_help();
            println!();
        }
    }
}
