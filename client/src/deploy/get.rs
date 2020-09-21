use std::str;

use clap::{App, Arg, ArgMatches, SubCommand};
use serde_json::json;

// use casper_node::types::Deploy;

use crate::{command::ClientCommand, common, RpcClient};

/// This struct defines the order in which the args are shown for this subcommand's help message.
enum DisplayOrder {
    NodeAddress,
    DeployHash,
}

/// Handles providing the arg for and retrieval of the deploy hash.
mod deploy_hash {
    use super::*;

    const ARG_NAME: &str = "deploy-hash";
    const ARG_VALUE_NAME: &str = "HEX STRING";
    const ARG_HELP: &str = "Hex-encoded deploy hash";

    pub(super) fn arg() -> Arg<'static, 'static> {
        Arg::with_name(ARG_NAME)
            .required(true)
            .value_name(ARG_VALUE_NAME)
            .help(ARG_HELP)
            .display_order(DisplayOrder::DeployHash as usize)
    }

    pub(super) fn get(matches: &ArgMatches) -> String {
        matches
            .value_of(ARG_NAME)
            .unwrap_or_else(|| panic!("should have {} arg", ARG_NAME))
            .to_string()
    }
}

pub struct GetDeploy {}

impl RpcClient for GetDeploy {
    const RPC_METHOD: &'static str = "info_get_deploy";
}

impl<'a, 'b> ClientCommand<'a, 'b> for GetDeploy {
    const NAME: &'static str = "get-deploy";
    const ABOUT: &'static str = "Retrieves a stored deploy";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::node_address::arg(
                DisplayOrder::NodeAddress as usize,
            ))
            .arg(deploy_hash::arg())
    }

    fn run(matches: &ArgMatches<'_>) {
        let node_address = common::node_address::get(matches);
        let params = vec![json!(deploy_hash::get(matches))];

        let response_value = Self::request_with_array_params(&node_address, params)
            .unwrap_or_else(|error| panic!("response error: {}", error));
        println!("{}", response_value);

        todo!("parse deploy for printing");
        // let url = format!(
        //     "{}/{}/{}",
        //     node_address,
        //     common::DEPLOY_API_PATH,
        //     deploy_hash
        // );
        // let body = executor::block_on(async {
        //     reqwest::get(&url)
        //         .await
        //         .unwrap_or_else(|error| panic!("should get response from node: {}", error))
        //         .bytes()
        //         .await
        //         .unwrap_or_else(|error| panic!("should get bytes from node response: {}", error))
        // });
        //
        // let json_encoded = str::from_utf8(body.as_ref())
        //     .unwrap_or_else(|error| panic!("should parse node response as JSON: {}", error));
        // if json_encoded == "null" {
        //     println!("Deploy not found");
        // } else {
        //     let deploy = Deploy::from_json(json_encoded)
        //         .unwrap_or_else(|error| panic!("should deserialize deploy from JSON: {}",
        // error));     println!("{}", deploy);
        // }
    }
}
