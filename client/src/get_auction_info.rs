use std::str;

use clap::{App, ArgMatches, SubCommand};
use serde::Serialize;

use casper_client::Error as ClientError;
use casper_node::rpcs::state::GetAuctionInfo;

use crate::{command::ClientCommand, common};
use std::fmt::{Display, Formatter};

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    Verbose,
    NodeAddress,
    RpcId,
}

/// This enum defines the errors associated with this specific RPC request.
#[derive(Serialize)]
enum Error {
    /// The block required for the auction information was not found
    BlockNotFound(String),
    /// An unknown error was encountered
    UnknownError(String),
}

impl From<ClientError> for Error {
    fn from(error: ClientError) -> Self {
        if let ClientError::ResponseIsError(rpc_error) = error {
            match rpc_error.code {
                -32001 => Self::BlockNotFound(rpc_error.message),
                _ => Self::UnknownError(rpc_error.message),
            }
        } else {
            panic!("Failed to parse error from node: {:?}", error)
        }
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BlockNotFound(message) => {
                write!(f, "Could not find block for auction info: {}", message)
            }
            Self::UnknownError(message) => {
                write!(f, "An unknown error was encountered: {}", message)
            }
        }
    }
}

impl<'a, 'b> ClientCommand<'a, 'b> for GetAuctionInfo {
    const NAME: &'static str = "get-auction-info";
    const ABOUT: &'static str =
        "Retrieves the bids and validators as of the most recently added block";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::verbose::arg(DisplayOrder::Verbose as usize))
            .arg(common::node_address::arg(
                DisplayOrder::NodeAddress as usize,
            ))
            .arg(common::rpc_id::arg(DisplayOrder::RpcId as usize))
    }

    fn run(matches: &ArgMatches<'_>) {
        let maybe_rpc_id = common::rpc_id::get(matches);
        let node_address = common::node_address::get(matches);
        let mut verbosity_level = common::verbose::get(matches);

        let response = casper_client::get_auction_info(maybe_rpc_id, node_address, verbosity_level);

        if verbosity_level == 0 {
            verbosity_level += 1
        }

        match response {
            Ok(response) => casper_client::pretty_print_at_level(&response, verbosity_level),
            Err(error) => println!("{}", Error::from(error)),
        }
    }
}
