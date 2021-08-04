mod account_address;
mod block;
mod command;
mod common;
mod deploy;
mod docs;
mod generate_completion;
mod get_account_info;
mod get_auction_info;
mod get_balance;
mod get_era_info_by_switch_block;
mod get_state_hash;
mod keygen;
mod query_dictionary;
mod query_global_state;

use std::process;

use clap::{crate_description, crate_version, App};

use casper_client::Error;
use casper_node::rpcs::{
    account::PutDeploy,
    chain::{GetBlock, GetBlockTransfers, GetEraInfoBySwitchBlock, GetStateRootHash},
    docs::ListRpcs,
    info::GetDeploy,
    state::{GetAccountInfo, GetAuctionInfo, GetBalance, GetDictionaryItem, QueryGlobalState},
};

use account_address::GenerateAccountHash as AccountAddress;
use command::{ClientCommand, Success};
use deploy::{ListDeploys, MakeDeploy, MakeTransfer, SendDeploy, SignDeploy, Transfer};
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
    MakeTransfer,
    GetDeploy,
    GetBlock,
    GetBlockTransfers,
    ListDeploys,
    GetStateRootHash,
    GetBalance,
    GetAccountInfo,
    GetEraInfo,
    GetAuctionInfo,
    Keygen,
    GenerateCompletion,
    GetRpcs,
    AccountAddress,
    GetDictionaryItem,
    QueryGlobalState,
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
        .subcommand(MakeTransfer::build(DisplayOrder::MakeTransfer as usize))
        .subcommand(GetDeploy::build(DisplayOrder::GetDeploy as usize))
        .subcommand(GetBlock::build(DisplayOrder::GetBlock as usize))
        .subcommand(GetBlockTransfers::build(
            DisplayOrder::GetBlockTransfers as usize,
        ))
        .subcommand(ListDeploys::build(DisplayOrder::ListDeploys as usize))
        .subcommand(GetBalance::build(DisplayOrder::GetBalance as usize))
        .subcommand(GetAccountInfo::build(DisplayOrder::GetAccountInfo as usize))
        .subcommand(GetStateRootHash::build(
            DisplayOrder::GetStateRootHash as usize,
        ))
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
        .subcommand(GetDictionaryItem::build(
            DisplayOrder::GetDictionaryItem as usize,
        ))
        .subcommand(QueryGlobalState::build(
            DisplayOrder::QueryGlobalState as usize,
        ))
}

#[tokio::main]
async fn main() {
    let arg_matches = cli().get_matches();
    let (result, matches) = match arg_matches.subcommand() {
        (PutDeploy::NAME, Some(matches)) => (PutDeploy::run(matches), matches),
        (MakeDeploy::NAME, Some(matches)) => (MakeDeploy::run(matches), matches),
        (SignDeploy::NAME, Some(matches)) => (SignDeploy::run(matches), matches),
        (SendDeploy::NAME, Some(matches)) => (SendDeploy::run(matches), matches),
        (Transfer::NAME, Some(matches)) => (Transfer::run(matches), matches),
        (MakeTransfer::NAME, Some(matches)) => (MakeTransfer::run(matches), matches),
        (GetDeploy::NAME, Some(matches)) => (GetDeploy::run(matches), matches),
        (GetBlock::NAME, Some(matches)) => (GetBlock::run(matches), matches),
        (GetBlockTransfers::NAME, Some(matches)) => (GetBlockTransfers::run(matches), matches),
        (ListDeploys::NAME, Some(matches)) => (ListDeploys::run(matches), matches),
        (GetBalance::NAME, Some(matches)) => (GetBalance::run(matches), matches),
        (GetAccountInfo::NAME, Some(matches)) => (GetAccountInfo::run(matches), matches),
        (GetStateRootHash::NAME, Some(matches)) => (GetStateRootHash::run(matches), matches),
        (GetEraInfoBySwitchBlock::NAME, Some(matches)) => {
            (GetEraInfoBySwitchBlock::run(matches), matches)
        }
        (GetAuctionInfo::NAME, Some(matches)) => (GetAuctionInfo::run(matches), matches),
        (Keygen::NAME, Some(matches)) => (Keygen::run(matches), matches),
        (GenerateCompletion::NAME, Some(matches)) => (GenerateCompletion::run(matches), matches),
        (ListRpcs::NAME, Some(matches)) => (ListRpcs::run(matches), matches),
        (AccountAddress::NAME, Some(matches)) => (AccountAddress::run(matches), matches),
        (GetDictionaryItem::NAME, Some(matches)) => (GetDictionaryItem::run(matches), matches),
        (QueryGlobalState::NAME, Some(matches)) => (QueryGlobalState::run(matches), matches),
        _ => {
            let _ = cli().print_long_help();
            println!();
            process::exit(1);
        }
    };

    let mut verbosity_level = common::verbose::get(matches);
    if verbosity_level == 0 {
        verbosity_level += 1
    }

    match &result {
        Ok(Success::Response(response)) => {
            casper_client::pretty_print_at_level(&response, verbosity_level)
        }
        Ok(Success::Output(output)) => println!("{}", output),
        Err(Error::ResponseIsError(error)) => {
            casper_client::pretty_print_at_level(&error, verbosity_level);
            process::exit(1);
        }
        Err(error) => {
            println!("{}", error);
            process::exit(1);
        }
    }
}
