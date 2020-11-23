use std::{
    fs::File,
    io::{self, BufReader, Read, Write},
};

use semver::Version;
use serde::{Deserialize, Serialize};

use casper_execution_engine::core::engine_state::ExecutableDeployItem;
use casper_node::{
    crypto::asymmetric_key::SecretKey,
    rpcs::{account::PutDeploy, chain::GetBlockResult, info::GetDeploy, RpcWithParams},
    types::{Deploy, DeployHash, TimeDiff, Timestamp},
};

use crate::{
    error::{Error, Result},
    rpc::RpcClient,
};

/// SendDeploy allows sending a deploy to the node.
pub(crate) struct SendDeploy;

/// Transfer allows transferring an amount between accounts.
pub(crate) struct Transfer {}

impl RpcClient for PutDeploy {
    const RPC_METHOD: &'static str = Self::METHOD;
}

impl RpcClient for GetDeploy {
    const RPC_METHOD: &'static str = Self::METHOD;
}

impl RpcClient for SendDeploy {
    const RPC_METHOD: &'static str = PutDeploy::METHOD;
}

impl RpcClient for Transfer {
    const RPC_METHOD: &'static str = PutDeploy::METHOD;
}

/// Result for "chain_get_block" RPC response.
#[derive(Serialize, Deserialize, Debug)]
pub struct ListDeploysResult {
    /// The RPC API version.
    pub api_version: Version,
    /// The deploy hashes of the block, if found.
    pub deploy_hashes: Option<Vec<DeployHash>>,
}

impl From<GetBlockResult> for ListDeploysResult {
    fn from(get_block_result: GetBlockResult) -> Self {
        ListDeploysResult {
            api_version: get_block_result.api_version,
            deploy_hashes: get_block_result
                .block
                .map(|block| block.deploy_hashes().clone()),
        }
    }
}

/// Creates a Write trait object for File or Stdout respective to the path value passed
/// Stdout is used when None
pub(super) fn output_or_stdout(maybe_path: Option<&str>) -> io::Result<Box<dyn Write>> {
    match maybe_path {
        Some(output_path) => File::create(&output_path).map(|file| {
            let write: Box<dyn Write> = Box::new(file);
            write
        }),
        None => Ok(Box::new(io::stdout())),
    }
}

/// `DeployParams` are used as a helper to construct a `Deploy` with
/// `DeployExt::with_payment_and_session`.
pub struct DeployParams {
    /// The secret key for this `Deploy`.
    pub secret_key: SecretKey,

    /// The creation timestamp of this `Deploy`.
    pub timestamp: Timestamp,

    /// The time to live for this `Deploy`.
    pub ttl: TimeDiff,

    /// The gas price for this `Deploy`.
    pub gas_price: u64,

    /// A list of other `Deploy`s (hashes) that this `Deploy` depends upon.
    pub dependencies: Vec<DeployHash>,

    /// The name of the chain this `Deploy` will be considered for inclusion in.
    pub chain_name: String,
}

/// An extension trait that adds some client-specific functionality to `Deploy`.
pub(super) trait DeployExt {
    /// Constructs a `Deploy`.
    fn with_payment_and_session(
        params: DeployParams,
        payment: ExecutableDeployItem,
        session: ExecutableDeployItem,
    ) -> Deploy;

    /// Writes the `Deploy` to `output`.
    fn write_deploy<W>(&self, output: W) -> Result<()>
    where
        W: Write;

    /// Reads a `Deploy` from the `input`.
    fn read_deploy<R>(input: R) -> Result<Deploy>
    where
        R: Read;

    /// Reads a `Deploy` from the reader at `input`, signs it, then writes it back to `output`.
    fn sign_and_write_deploy<R, W>(input: R, secret_key: SecretKey, output: W) -> Result<()>
    where
        R: Read,
        W: Write;
}

impl DeployExt for Deploy {
    fn with_payment_and_session(
        params: DeployParams,
        payment: ExecutableDeployItem,
        session: ExecutableDeployItem,
    ) -> Deploy {
        let DeployParams {
            timestamp,
            ttl,
            gas_price,
            dependencies,
            chain_name,
            secret_key,
        } = params;
        let mut rng = casper_node::new_rng();
        Deploy::new(
            timestamp,
            ttl,
            gas_price,
            dependencies,
            chain_name,
            payment,
            session,
            &secret_key,
            &mut rng,
        )
    }

    fn write_deploy<W>(&self, mut output: W) -> Result<()>
    where
        W: Write,
    {
        let content = serde_json::to_string_pretty(self)?;
        output
            .write_all(content.as_bytes())
            .map_err(|error| Error::IoError {
                context: "unable to write deploy".to_owned(),
                error,
            })
    }

    fn read_deploy<R>(input: R) -> Result<Deploy>
    where
        R: Read,
    {
        let reader = BufReader::new(input);
        Ok(serde_json::from_reader(reader)?)
    }

    fn sign_and_write_deploy<R, W>(input: R, secret_key: SecretKey, output: W) -> Result<()>
    where
        R: Read,
        W: Write,
    {
        let mut deploy = Deploy::read_deploy(input)?;
        let mut rng = casper_node::new_rng();
        deploy.sign(&secret_key, &mut rng);
        deploy.write_deploy(output)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{DeployStrParams, PaymentStrParams, SessionStrParams};
    use std::convert::TryInto;

    const PKG_HASH: &str = "09dcee4b212cfd53642ab323fbef07dafafc6f945a80a00147f62910a915c4e6";
    const ENTRYPOINT: &str = "entrypoint";
    const VERSION: &str = "0.1.0";
    const SAMPLE_DEPLOY: &str = r#"{
        "hash": "c38849ad9057a368caf3e62799c5368de9e656185f781e434e16757c6e8ce9f4",
        "header": {
          "account": "01f60bce2bb1059c41910eac1e7ee6c3ef4c8fcc63a901eb9603c1524cadfb0c18",
          "timestamp": "2020-11-23T19:20:23.015Z",
          "ttl": "10s",
          "gas_price": 1,
          "body_hash": "1edd9716bc3b94fb2e4bdc769a1bcb0b3c7c4df2135ff1c2a405f0ae22e47646",
          "dependencies": [
            "be5fdeea0240e999e376f8ecbce1bd4fd9336f58dae4a5842558a4da6ad35aa8",
            "168d7ea9c88e76b3eef72759f2a7af24663cc871a469c7ba1387ca479e82fb41"
          ],
          "chain_name": "casper-test-chain-name-1"
        },
        "payment": {
          "StoredVersionedContractByHash": {
            "hash": "09dcee4b212cfd53642ab323fbef07dafafc6f945a80a00147f62910a915c4e6",
            "version": null,
            "entry_point": "entrypoint",
            "args": "02000000070000006e616d655f3031010000000000070000006e616d655f3032040000002a00000001"
          }
        },
        "session": {
          "StoredVersionedContractByHash": {
            "hash": "09dcee4b212cfd53642ab323fbef07dafafc6f945a80a00147f62910a915c4e6",
            "version": null,
            "entry_point": "entrypoint",
            "args": "02000000070000006e616d655f3031010000000000070000006e616d655f3032040000002a00000001"
          }
        },
        "approvals": [
            {
                "signer": "0129559e33ff6e1917c0bb6890ac5cf6087c9c79440b42b035741e6b2075a23637",
                "signature": "0184ac4c736ad2cc715bf67408f7d5c5495a53b5d52e7a0bd67696b2acf20736f0970f8909cd80ff50fcc104bc78d2a44134deabf8bd60ec60ee80bafd39b5e60c"
            },
            {
                "signer": "01e204c257ab9ba52b5635d3904112ddc8339472d8fa07a9bed0f2cd7196e6f2b1",
                "signature": "0102ad384f4754d1564fa10d65b7df7d3caeeb0b81acd0cecb65d95ed146dc5a1a87f65ae6d86ff120ded8cf84ae1e3fd05a06c0fe3c9de6ad562fbfb707329904"
            }
        ]
      }"#;

    #[derive(Debug)]
    struct ErrWrapper(pub Error);

    impl PartialEq for ErrWrapper {
        fn eq(&self, other: &ErrWrapper) -> bool {
            format!("{:?}", self.0) == format!("{:?}", other.0)
        }
    }

    pub fn deploy_params() -> DeployStrParams<'static> {
        DeployStrParams {
            secret_key: "../resources/local/secret_keys/node-1.pem",
            ttl: "10s",
            chain_name: "casper-test-chain-name-1",
            gas_price: "1",
            dependencies: vec![
                "be5fdeea0240e999e376f8ecbce1bd4fd9336f58dae4a5842558a4da6ad35aa8",
                "168d7ea9c88e76b3eef72759f2a7af24663cc871a469c7ba1387ca479e82fb41",
            ],
            ..Default::default()
        }
    }

    fn args_simple() -> Vec<&'static str> {
        vec!["name_01:bool='false'", "name_02:i32='42'"]
    }

    #[test]
    fn should_create_deploy() {
        let deploy_params = deploy_params();
        let payment_params =
            PaymentStrParams::with_package_hash(PKG_HASH, VERSION, ENTRYPOINT, args_simple(), "");
        let session_params =
            SessionStrParams::with_package_hash(PKG_HASH, VERSION, ENTRYPOINT, args_simple(), "");

        let mut output = Vec::new();

        let deploy = Deploy::with_payment_and_session(
            deploy_params.try_into().unwrap(),
            payment_params.try_into().unwrap(),
            session_params.try_into().unwrap(),
        );
        deploy.write_deploy(&mut output).unwrap();

        let result = String::from_utf8(output).unwrap();

        let expected = Deploy::read_deploy(SAMPLE_DEPLOY.as_bytes()).unwrap();
        let actual = Deploy::read_deploy(result.as_bytes()).unwrap();
        assert_eq!(expected.header().account(), actual.header().account());
        assert_eq!(expected.header().ttl(), actual.header().ttl());
        assert_eq!(expected.header().gas_price(), actual.header().gas_price());
        assert_eq!(expected.header().body_hash(), actual.header().body_hash());
        assert_eq!(expected.payment(), actual.payment());
        assert_eq!(expected.session(), actual.session());
    }

    #[test]
    fn should_read_deploy() {
        let bytes = SAMPLE_DEPLOY.as_bytes();
        assert_eq!(
            Deploy::read_deploy(bytes).map(|_| ()).map_err(ErrWrapper),
            Ok(())
        );
    }

    #[test]
    fn should_sign_deploy() {
        let bytes = SAMPLE_DEPLOY.as_bytes();
        let mut deploy = Deploy::read_deploy(bytes).unwrap();
        assert!(
            deploy.is_valid(),
            "deploy should be is_valid() {:#?}",
            deploy
        );
        assert_eq!(deploy.approvals().len(), 2);
        let mut result = Vec::new();
        Deploy::sign_and_write_deploy(bytes, SecretKey::generate_ed25519(), &mut result).unwrap();
        let signed_deploy = Deploy::read_deploy(&result[..]).unwrap();
        assert_eq!(
            signed_deploy.approvals().len(),
            deploy.approvals().len() + 1,
            "deploy should be is_valid() because it has been signed {:#?}",
            signed_deploy
        );
    }
}
