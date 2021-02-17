use std::str;

use clap::{App, ArgMatches, SubCommand};
use serde::Serialize;

use casper_client::Error as ClientError;
use casper_node::rpcs::chain::GetEraInfoBySwitchBlock;

use crate::{command::ClientCommand, common};
use std::fmt::{Display, Formatter};

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    Verbose,
    NodeAddress,
    RpcId,
    BlockIdentifier,
}

/// This enum defines the possible errors associated with this specific RPC request.
#[derive(Serialize)]
enum Error {
    /// Failed to get the switch block.
    BlockNotFound(String),
    /// Failed to get the queried era information
    EraInfoQueryFailed(String),
    /// Failed to execute the query for era information.
    EraInfoQueryExecutionFailure(String),
    /// An unknown error was encountered.
    Unknown(String),
}

impl From<ClientError> for Error {
    fn from(error: ClientError) -> Self {
        if let ClientError::ResponseIsError(rpc_error) = error {
            match rpc_error.code {
                -32001 => Self::BlockNotFound(rpc_error.message),
                -32003 => Self::EraInfoQueryFailed(rpc_error.message),
                -32004 => Self::EraInfoQueryExecutionFailure(rpc_error.message),
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
            Self::BlockNotFound(message) => {
                write!(f, "Failed to get the switch block: {}", message)
            }
            Self::EraInfoQueryFailed(message) => {
                write!(f, "The era info query failed: {}", message)
            }
            Self::EraInfoQueryExecutionFailure(message) => write!(
                f,
                "The era information query failed to execute: {}",
                message
            ),
            Self::Unknown(message) => {
                write!(f, "An unknown error was encountered: {}", message)
            }
        }
    }
}

impl<'a, 'b> ClientCommand<'a, 'b> for GetEraInfoBySwitchBlock {
    const NAME: &'static str = "get-era-info-by-switch-block";
    const ABOUT: &'static str = "Retrieves era information from the network";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::verbose::arg(DisplayOrder::Verbose as usize))
            .arg(common::node_address::arg(
                DisplayOrder::NodeAddress as usize,
            ))
            .arg(common::rpc_id::arg(DisplayOrder::RpcId as usize))
            .arg(common::block_identifier::arg(
                DisplayOrder::BlockIdentifier as usize,
            ))
    }

    fn run(matches: &ArgMatches<'_>) {
        let maybe_rpc_id = common::rpc_id::get(matches);
        let node_address = common::node_address::get(matches);
        let mut verbosity_level = common::verbose::get(matches);
        let maybe_block_id = common::block_identifier::get(&matches);

        let response = casper_client::get_era_info_by_switch_block(
            maybe_rpc_id,
            node_address,
            verbosity_level,
            maybe_block_id,
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
