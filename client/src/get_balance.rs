use std::str;

use clap::{App, Arg, ArgMatches, SubCommand};
use serde::Serialize;

use casper_client::Error as ClientError;
use casper_node::rpcs::state::GetBalance;

use crate::{command::ClientCommand, common};
use std::fmt::{Display, Formatter};

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    Verbose,
    NodeAddress,
    RpcId,
    StateRootHash,
    PurseURef,
}

/// This enum defines the possible errors associated with this specific RPC request.
#[derive(Serialize)]
enum Error {
    /// Failure to retrieve the balance.
    GetBalanceFailure(String),
    /// Failure in executing the RPC request
    ExecutionFailure(String),
    /// Encoding error in warp_json_rpc
    EncodingFailure(String),
    /// Failure to parse the purse URef.
    URefParseFailure(String),
    /// An unknown error was encountered.
    Unknown(String),
}

impl From<ClientError> for Error {
    fn from(error: ClientError) -> Self {
        if let ClientError::ResponseIsError(rpc_error) = error {
            match rpc_error.code {
                -32006 => Self::GetBalanceFailure(rpc_error.message),
                -32007 => Self::ExecutionFailure(rpc_error.message),
                -32603 => Self::EncodingFailure(rpc_error.message),
                -32005 => Self::URefParseFailure(rpc_error.message),
                _ => Self::Unknown(rpc_error.message),
            }
        } else {
            panic!("Failed to parse client error: {:?}", error)
        }
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GetBalanceFailure(message) => write!(f, "Failed to get balance: {}", message),
            Self::ExecutionFailure(message) => write!(
                f,
                "RPC request to get balance failed to execute: {}",
                message
            ),
            Self::EncodingFailure(message) => {
                write!(f, "A warp_json_rpc error in encoding occurred: {}", message)
            }
            Self::URefParseFailure(message) => {
                write!(f, "Failed to parse the purse URef: {}", message)
            }
            Self::Unknown(message) => {
                write!(f, "An unknown error was encountered: {}", message)
            }
        }
    }
}

/// Handles providing the arg for and retrieval of the purse URef.
mod purse_uref {
    use super::*;

    const ARG_NAME: &str = "purse-uref";
    const ARG_SHORT: &str = "p";
    const ARG_VALUE_NAME: &str = "FORMATTED STRING";
    const ARG_HELP: &str =
        "The URef under which the purse is stored. This must be a properly formatted URef \
        \"uref-<HEX STRING>-<THREE DIGIT INTEGER>\"";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .long(ARG_NAME)
            .short(ARG_SHORT)
            .required(true)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::PurseURef as usize)
    }

    pub(super) fn get<'a>(matches: &'a ArgMatches) -> &'a str {
        matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME))
    }
}

impl<'a, 'b> ClientCommand<'a, 'b> for GetBalance {
    const NAME: &'static str = "get-balance";
    const ABOUT: &'static str = "Retrieves a purse's balance from the network";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::verbose::arg(DisplayOrder::Verbose as usize))
            .arg(common::node_address::arg(
                DisplayOrder::NodeAddress as usize,
            ))
            .arg(common::rpc_id::arg(DisplayOrder::RpcId as usize))
            .arg(common::state_root_hash::arg(
                DisplayOrder::StateRootHash as usize,
            ))
            .arg(purse_uref::arg())
    }

    fn run(matches: &ArgMatches<'_>) {
        let maybe_rpc_id = common::rpc_id::get(matches);
        let node_address = common::node_address::get(matches);
        let mut verbosity_level = common::verbose::get(matches);
        let state_root_hash = common::state_root_hash::get(&matches);
        let purse_uref = purse_uref::get(&matches);

        let response = casper_client::get_balance(
            maybe_rpc_id,
            node_address,
            verbosity_level,
            state_root_hash,
            purse_uref,
        );

        if verbosity_level == 0 {
            verbosity_level += 1
        }

        match response {
            Ok(response) => casper_client::pretty_print_at_level(&response, verbosity_level),
            Err(error) => println!("{}", Error::from(error)),
        }
    }
}
