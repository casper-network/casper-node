use std::str;

use rand::Rng;
use reqwest::{Client, StatusCode};
use structopt::StructOpt;

use casperlabs_node::types::Deploy;

const DEPLOY_API_PATH: &str = "deploy";

#[derive(Debug, StructOpt)]
/// CasperLabs client.
pub enum Args {
    /// Store a new random deploy.
    PutDeploy {
        /// Address of the casperlabs-node HTTP service to contact.  Example format:
        /// http://localhost:7777
        node_address: String,
    },
    /// Retrieve a stored deploy.
    GetDeploy {
        /// Address of the casperlabs-node HTTP service to contact.  Example format:
        /// http://localhost:7777
        node_address: String,
        /// Hex-encoded deploy hash.
        deploy_hash: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Args::from_args() {
        Args::PutDeploy { node_address } => put_deploy(node_address).await,
        Args::GetDeploy {
            node_address,
            deploy_hash,
        } => get_deploy(node_address, deploy_hash).await,
    }
}

async fn put_deploy(node_address: String) -> anyhow::Result<()> {
    let mut rng = rand::thread_rng();
    let deploy = Deploy::new(rng.gen());
    let deploy_hash = deploy.id();
    let body = deploy.to_json()?;

    let client = Client::new();
    let url = format!("{}/{}", node_address, DEPLOY_API_PATH);
    let response = client.post(&url).body(body).send().await?;

    if response.status() == StatusCode::OK {
        println!("Stored deploy with deploy-hash:\n{:?}", deploy_hash.inner());
    } else {
        println!("Storing {} failed\n{:?}", deploy_hash, response);
    }

    Ok(())
}

async fn get_deploy(node_address: String, deploy_hash: String) -> anyhow::Result<()> {
    let url = format!("{}/{}/{}", node_address, DEPLOY_API_PATH, deploy_hash);
    let body = reqwest::get(&url).await?.bytes().await?;

    let json_encoded = str::from_utf8(body.as_ref())?;
    if json_encoded == "null" {
        println!("Deploy not found");
    } else {
        let deploy = Deploy::from_json(json_encoded)?;
        println!("{}", deploy);
    }

    Ok(())
}
