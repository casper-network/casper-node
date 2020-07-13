use std::{
    fs,
    path::PathBuf,
    str,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use reqwest::{Client, StatusCode};
use structopt::StructOpt;

use casperlabs_node::{
    components::contract_runtime::core::engine_state::executable_deploy_item::ExecutableDeployItem,
    crypto::{
        asymmetric_key::{self, PublicKey, SecretKey},
        hash::Digest,
    },
    types::{Deploy, DeployHeader},
};

const DEPLOY_API_PATH: &str = "deploys";

#[derive(Debug, StructOpt)]
/// CasperLabs client.
pub enum Args {
    /// Store a new random deploy.
    PutDeploy {
        /// Address of the casperlabs-node HTTP service to contact.  Example format:
        /// http://localhost:7777
        node_address: String,
        #[structopt(parse(from_os_str), long)]
        /// Secret key to sign deploys with.
        secret_key: PathBuf,
        #[structopt(parse(from_os_str), long)]
        /// Path to payment code.
        payment_code: PathBuf,
        #[structopt(parse(from_os_str), long)]
        /// Path to session code.
        session_code: PathBuf,
        #[structopt(long)]
        /// Current time milliseconds.
        timestamp: Option<u64>,
        #[structopt(long, default_value = "10")]
        /// Conversion rate between the cost of Wasm opcodes and the motes sent by the `payment_code`.
        gas_price: u64,
        #[structopt(long, default_value = "0")]
        /// Time to live of the deploy, in milliseconds. A deploy can only be included
        /// in a block between `timestamp` and `timestamp + ttl`.
        /// If this option is not present a default value will be assigned instead.
        ttl: u32,
        /// If present, the deploy can only be included in a block on the right chain.
        /// This can be used to preotect against accidental or malicious cross chain
        /// deploys, in case the same account exists on multiple networks.
        #[structopt(long, default_value = "Test")]
        chain_name: String,
        // TODO: There are also deploy dependencies, but this whole structure
        // is subject to changes.
    },
    /// Retrieve a stored deploy.
    GetDeploy {
        /// Address of the casperlabs-node HTTP service to contact.  Example format:
        /// http://localhost:7777
        node_address: String,
        /// Hex-encoded deploy hash.
        deploy_hash: String,
    },
    /// Get the list of all stored deploys' hashes.
    ListDeploys {
        /// Address of the casperlabs-node HTTP service to contact.  Example format:
        /// http://localhost:7777
        node_address: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match Args::from_args() {
        Args::PutDeploy {
            node_address,
            secret_key,
            payment_code,
            session_code,
            timestamp,
            gas_price,
            ttl,
            chain_name,
        } => {
            put_deploy(
                node_address,
                secret_key,
                payment_code,
                session_code,
                timestamp,
                gas_price,
                ttl,
                chain_name,
            )
            .await
        }
        Args::GetDeploy {
            node_address,
            deploy_hash,
        } => get_deploy(node_address, deploy_hash).await,
        Args::ListDeploys { node_address } => list_deploys(node_address).await,
    }
}

fn get_file_bytes(filename: PathBuf) -> anyhow::Result<Vec<u8>> {
    fs::read(&filename).with_context(|| format!("Failed to read {}", filename.display()))
}

#[allow(clippy::too_many_arguments)]
async fn put_deploy(
    node_address: String,
    secret_key: PathBuf,
    payment_code: PathBuf,
    session_code: PathBuf,
    timestamp: Option<u64>,
    gas_price: u64,
    ttl: u32,
    chain_name: String,
) -> anyhow::Result<()> {
    let timestamp = timestamp.unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    });

    let payment_bytes = get_file_bytes(payment_code)?;
    let session_bytes = get_file_bytes(session_code)?;
    let secret_key_bytes = get_file_bytes(secret_key)?;
    let secret_key = SecretKey::ed25519_from_bytes(secret_key_bytes)?;
    let public_key = PublicKey::from(&secret_key);

    let msg = b"Message"; // TODO
    let sig = asymmetric_key::sign(msg, &secret_key, &public_key);

    let deploy_hash: Digest = [42; 32].into();

    let deploy: Deploy = Deploy::new(
        deploy_hash.into(),
        DeployHeader {
            account: public_key,
            timestamp,
            gas_price,
            body_hash: [1; 32].into(),
            ttl_millis: ttl,
            dependencies: vec![],
            chain_name,
        },
        ExecutableDeployItem::ModuleBytes {
            module_bytes: payment_bytes,
            args: vec![],
        },
        ExecutableDeployItem::ModuleBytes {
            module_bytes: session_bytes,
            args: vec![],
        },
        vec![sig],
    );
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

async fn list_deploys(node_address: String) -> anyhow::Result<()> {
    let url = format!("{}/{}", node_address, DEPLOY_API_PATH);
    let body = reqwest::get(&url).await?.bytes().await?;

    let json_encoded = str::from_utf8(body.as_ref())?;
    println!("{}", json_encoded);

    Ok(())
}
