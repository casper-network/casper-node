use std::{
    convert::{TryFrom, TryInto},
    fs::{self, File},
    path::PathBuf,
    str,
};

use anyhow::{Context, Error};
use rand::Rng;
use reqwest::{Client, StatusCode};
use serde::{self, Deserialize};
use structopt::StructOpt;

use casperlabs_node::{
    components::contract_runtime::core::engine_state::executable_deploy_item::ExecutableDeployItem,
    crypto::{
        asymmetric_key::{self, PublicKey, SecretKey},
        hash::Digest,
    },
    types::{Deploy, DeployHeader, Timestamp},
};
use types::{
    bytesrepr::{self, ToBytes},
    CLValue, NamedArg, RuntimeArgs,
};

const DEPLOY_API_PATH: &str = "deploys";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DeployArgValue {
    /// Contains `CLValue` serialized into bytes in base16 form.
    #[serde(deserialize_with = "hex::deserialize")]
    RawBytes(Vec<u8>),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
struct DeployArg {
    /// Deploy argument's name.
    name: String,
    value: DeployArgValue,
}

impl TryFrom<DeployArgValue> for CLValue {
    type Error = bytesrepr::Error;
    fn try_from(value: DeployArgValue) -> Result<Self, Self::Error> {
        match value {
            DeployArgValue::RawBytes(bytes) => bytesrepr::deserialize(bytes),
        }
    }
}

impl TryFrom<DeployArg> for NamedArg {
    type Error = bytesrepr::Error;

    fn try_from(deploy_arg: DeployArg) -> Result<Self, Self::Error> {
        let cl_value = deploy_arg.value.try_into()?;
        Ok(NamedArg::new(deploy_arg.name, cl_value))
    }
}

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
        /// Path to payment code args.
        payment_args: PathBuf,
        #[structopt(parse(from_os_str), long)]
        /// Path to session code.
        session_code: PathBuf,
        #[structopt(parse(from_os_str), long)]
        /// Path to session code args.
        session_args: PathBuf,
        #[structopt(long)]
        /// Current time in milliseconds since UNIX epoch.
        timestamp: Option<Timestamp>,
        #[structopt(long, default_value = "10")]
        /// Conversion rate between the cost of Wasm opcodes and the motes sent by the `payment_code`.
        gas_price: u64,
        #[structopt(long, default_value = "3600000")]
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
            payment_args,
            session_code,
            session_args,
            timestamp,
            gas_price,
            ttl,
            chain_name,
        } => {
            put_deploy(
                node_address,
                secret_key,
                payment_code,
                payment_args,
                session_code,
                session_args,
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

fn get_runtime_args_from_file(filename: PathBuf) -> anyhow::Result<RuntimeArgs> {
    let reader = File::open(&filename)?;
    // Received structured args in json format.
    let args: Vec<DeployArg> = serde_json::from_reader(reader)?;
    // Convert JSON deploy args into vector of named args.
    let maybe_named_args: Result<Vec<NamedArg>, _> =
        args.into_iter().map(TryInto::try_into).collect();
    let named_args = maybe_named_args
        .map_err(Error::msg)
        .with_context(|| format!("trying to deserialize args from {}", filename.display()))?;
    Ok(RuntimeArgs::from(named_args))
}

#[allow(clippy::too_many_arguments)]
async fn put_deploy(
    node_address: String,
    secret_key: PathBuf,
    payment_code: PathBuf,
    payment_args: PathBuf,
    session_code: PathBuf,
    session_args: PathBuf,
    timestamp: Option<Timestamp>,
    gas_price: u64,
    ttl: u32,
    chain_name: String,
) -> anyhow::Result<()> {
    let timestamp = timestamp.unwrap_or_else(Timestamp::now);

    let payment_bytes = get_file_bytes(payment_code)?;
    let payment_runtime_args = get_runtime_args_from_file(payment_args)?;

    let session_bytes = get_file_bytes(session_code)?;
    let session_runtime_args = get_runtime_args_from_file(session_args)?;

    let secret_key_bytes = get_file_bytes(secret_key)?;
    let secret_key = SecretKey::ed25519_from_bytes(secret_key_bytes)?;
    let public_key = PublicKey::from(&secret_key);

    let msg = b"Message"; // TODO
    let sig = asymmetric_key::sign(msg, &secret_key, &public_key);

    let deploy_hash_bytes: [u8; 32] = rand::thread_rng().gen();
    let deploy_hash: Digest = Digest::from(deploy_hash_bytes);

    let deploy: Deploy = Deploy::new(
        deploy_hash.into(),
        DeployHeader {
            account: public_key,
            timestamp: timestamp.millis(),
            gas_price,
            body_hash: [1; 32].into(),
            ttl_millis: ttl,
            dependencies: vec![],
            chain_name,
        },
        ExecutableDeployItem::ModuleBytes {
            module_bytes: payment_bytes,
            args: payment_runtime_args.to_bytes().expect("should serialize"),
        },
        ExecutableDeployItem::ModuleBytes {
            module_bytes: session_bytes,
            args: session_runtime_args.to_bytes().expect("should serialize"),
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
