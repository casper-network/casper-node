use std::{convert::TryInto, fs::File, io::Read, path::PathBuf, str};

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
        /// Private key to sign deploys with.
        private_key: PathBuf,
        #[structopt(parse(from_os_str), long)]
        /// Path to payment code.
        payment_code: PathBuf,
        #[structopt(parse(from_os_str), long)]
        /// Path to session code.
        session_code: PathBuf,
        #[structopt(long, default_value = "0")]
        /// Deploy's timestamp.
        timestamp: u64,
        #[structopt(long, default_value = "100")]
        /// Gas price for running deploy.
        gas_price: u64,
        #[structopt(long, default_value = "1000")]
        /// Time to live.
        ttl: u32,
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
            private_key,
            payment_code,
            session_code,
            timestamp,
            gas_price,
            ttl,
        } => {
            put_deploy(
                node_address,
                private_key,
                payment_code,
                session_code,
                timestamp,
                gas_price,
                ttl,
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
    let mut data = Vec::new();
    let mut file = File::open(filename)?;
    file.read_to_end(&mut data)?;
    Ok(data)
}

async fn put_deploy(
    node_address: String,
    private_key: PathBuf,
    payment_code: PathBuf,
    session_code: PathBuf,
    timestamp: u64,
    gas_price: u64,
    ttl: u32,
) -> anyhow::Result<()> {
    let payment_bytes = get_file_bytes(payment_code)?;
    let session_bytes = get_file_bytes(session_code)?;
    let private_key_bytes = get_file_bytes(private_key)?;
    let private_key = SecretKey::new_ed25519(private_key_bytes.as_slice().try_into()?);
    let public_key = PublicKey::from(&private_key);

    let msg = b"Message"; // TODO
    let sig = asymmetric_key::sign(msg, &private_key, &public_key);

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
            chain_name: "Chain name".to_string(),
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
