use std::str;

use clap::{App, ArgMatches, SubCommand};
use futures::executor;

use crate::{command::ClientCommand, common};

pub struct ListDeploys {}

impl<'a, 'b> ClientCommand<'a, 'b> for ListDeploys {
    const NAME: &'static str = "list-deploys";
    const ABOUT: &'static str = "Gets the list of all stored deploys' hashes";

    fn build(display_order: usize) -> App<'a, 'b> {
        SubCommand::with_name(Self::NAME)
            .about(Self::ABOUT)
            .display_order(display_order)
            .arg(common::node_address::arg(0))
    }

    fn run(matches: &ArgMatches<'_>) {
        let node_address = common::node_address::get(matches);
        let url = format!("{}/{}", node_address, common::DEPLOY_API_PATH);
        let body = executor::block_on(async {
            reqwest::get(&url)
                .await
                .unwrap_or_else(|error| panic!("should get response from node: {}", error))
                .bytes()
                .await
                .unwrap_or_else(|error| panic!("should get bytes from node response: {}", error))
        });

        let json_encoded = str::from_utf8(body.as_ref())
            .unwrap_or_else(|error| panic!("should parse node response as JSON: {}", error));
        println!("{}", json_encoded);
    }
}
