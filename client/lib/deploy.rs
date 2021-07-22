use rand::{self, distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{self, BufReader, Read, Write},
    path::{Path, PathBuf},
};

use casper_execution_engine::core::engine_state::ExecutableDeployItem;
use casper_node::{
    rpcs::{account::PutDeploy, chain::GetBlockResult, info::GetDeploy, RpcWithParams},
    types::{Deploy, DeployHash, TimeDiff, Timestamp},
};
use casper_types::{ProtocolVersion, RuntimeArgs, SecretKey, URef, U512};

use crate::{
    error::{Error, Result},
    rpc::{RpcClient, TransferTarget},
};

/// The maximum permissible size in bytes of a Deploy when serialized via `ToBytes`.
///
/// Note: this should be kept in sync with the value of `[deploys.max_deploy_size]` in the
/// production chainspec.
const MAX_SERIALIZED_SIZE: u32 = 1_024 * 1_024;

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
    pub api_version: ProtocolVersion,
    /// The deploy hashes of the block, if found.
    pub deploy_hashes: Option<Vec<DeployHash>>,
    /// The transfer deploy hashes of the block, if found.
    pub transfer_hashes: Option<Vec<DeployHash>>,
}

impl From<GetBlockResult> for ListDeploysResult {
    fn from(get_block_result: GetBlockResult) -> Self {
        ListDeploysResult {
            api_version: get_block_result.api_version,
            deploy_hashes: get_block_result
                .block
                .as_ref()
                .map(|block| block.deploy_hashes().clone()),
            transfer_hashes: get_block_result
                .block
                .as_ref()
                .map(|block| block.transfer_hashes().clone()),
        }
    }
}

/// An output abstraction for associating a Write with some metadata.
pub(super) enum OutputKind<'a> {
    File {
        /// The path of the output file.
        path: &'a str,
        /// The path to a temp file in the same directory as the output file, this is used to make
        /// the write operation transactional. This is used to make sure that the file at `path` is
        /// not damaged if it exists.
        tmp_path: PathBuf,
        /// If `overwrite_if_exists` is `true`, then the file at `path` will be overwritten.
        overwrite_if_exists: bool,
    },
    Stdout,
}

impl<'a> OutputKind<'a> {
    /// This is a convenience method that acts as a constructor for a new `OutputKind::File` enum
    /// variant.
    pub(super) fn file(path: &'a str, overwrite_if_exists: bool) -> Self {
        let collision_resistant_string = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(5)
            .map(char::from)
            .collect::<String>();
        let extension = format!(".{}.tmp", &collision_resistant_string);
        let tmp_path = Path::new(path).with_extension(extension);
        OutputKind::File {
            path,
            tmp_path,
            overwrite_if_exists,
        }
    }

    /// `get()` returns a Result containing a Write trait object.
    pub(super) fn get(&self) -> Result<Box<dyn Write>> {
        match self {
            OutputKind::File {
                path,
                tmp_path,
                overwrite_if_exists,
                ..
            } => {
                let path = PathBuf::from(path);
                if path.exists() && !overwrite_if_exists {
                    return Err(Error::FileAlreadyExists(path));
                }
                let file = File::create(&tmp_path).map_err(|error| Error::IoError {
                    context: format!("failed to create {}", tmp_path.display()),
                    error,
                })?;

                let write: Box<dyn Write> = Box::new(file);
                Ok(write)
            }
            OutputKind::Stdout if cfg!(test) => Ok(Box::new(io::sink())),
            OutputKind::Stdout => Ok(Box::new(io::stdout())),
        }
    }

    /// `commit()` When called on an `OutputKind::File` causes the temp file to be renamed (moved)
    /// to its `path`. When called on an `OutputKind::Stdout` it acts as a noop function.
    pub(super) fn commit(self) -> Result<()> {
        match self {
            OutputKind::File { path, tmp_path, .. } => {
                fs::rename(&tmp_path, path).map_err(|error| Error::IoError {
                    context: format!(
                        "Could not move tmp file {} to destination {}",
                        tmp_path.display(),
                        path
                    ),
                    error,
                })
            }
            OutputKind::Stdout => Ok(()),
        }
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
    ) -> Result<Deploy>;

    /// Constructs a transfer `Deploy`.
    fn new_transfer(
        amount: U512,
        source_purse: Option<URef>,
        target: TransferTarget,
        transfer_id: u64,
        params: DeployParams,
        payment: ExecutableDeployItem,
    ) -> Result<Deploy>;

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
    ) -> Result<Deploy> {
        let DeployParams {
            timestamp,
            ttl,
            gas_price,
            dependencies,
            chain_name,
            secret_key,
        } = params;

        let deploy = Deploy::new(
            timestamp,
            ttl,
            gas_price,
            dependencies,
            chain_name,
            payment,
            session,
            &secret_key,
        );
        deploy.is_valid_size(MAX_SERIALIZED_SIZE)?;
        Ok(deploy)
    }

    fn new_transfer(
        amount: U512,
        source_purse: Option<URef>,
        target: TransferTarget,
        transfer_id: u64,
        params: DeployParams,
        payment: ExecutableDeployItem,
    ) -> Result<Deploy> {
        const TRANSFER_ARG_AMOUNT: &str = "amount";
        const TRANSFER_ARG_SOURCE: &str = "source";
        const TRANSFER_ARG_TARGET: &str = "target";
        const TRANSFER_ARG_ID: &str = "id";

        let mut transfer_args = RuntimeArgs::new();
        transfer_args.insert(TRANSFER_ARG_AMOUNT, amount)?;
        if let Some(source_purse) = source_purse {
            transfer_args.insert(TRANSFER_ARG_SOURCE, source_purse)?;
        }
        match target {
            TransferTarget::Account(target_account) => {
                let target_account_hash = target_account.to_account_hash().value();
                transfer_args.insert(TRANSFER_ARG_TARGET, target_account_hash)?;
            }
        }
        let maybe_transfer_id = Some(transfer_id);
        transfer_args.insert(TRANSFER_ARG_ID, maybe_transfer_id)?;
        let session = ExecutableDeployItem::Transfer {
            args: transfer_args,
        };
        Deploy::with_payment_and_session(params, payment, session)
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
        let deploy: Deploy = serde_json::from_reader(reader)?;
        deploy.is_valid_size(MAX_SERIALIZED_SIZE)?;
        Ok(deploy)
    }

    fn sign_and_write_deploy<R, W>(input: R, secret_key: SecretKey, output: W) -> Result<()>
    where
        R: Read,
        W: Write,
    {
        let mut deploy = Deploy::read_deploy(input)?;
        deploy.sign(&secret_key);
        deploy.is_valid_size(MAX_SERIALIZED_SIZE)?;
        deploy.write_deploy(output)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::convert::TryInto;

    use casper_node::{crypto::AsymmetricKeyExt, types::ExcessiveSizeDeployError};

    use super::*;
    use crate::{DeployStrParams, PaymentStrParams, SessionStrParams};

    const PKG_HASH: &str = "09dcee4b212cfd53642ab323fbef07dafafc6f945a80a00147f62910a915c4e6";
    const ENTRYPOINT: &str = "entrypoint";
    const VERSION: u32 = 10;
    const SAMPLE_DEPLOY: &str = r#"{
        "hash": "5e7763788281ddd6c5c404d0fb6010491703ccf497de6dcd2a6f893321de6b61",
        "header": {
            "account": "01f60bce2bb1059c41910eac1e7ee6c3ef4c8fcc63a901eb9603c1524cadfb0c18",
            "timestamp": "2021-07-22T16:40:08.250Z",
            "ttl": "10s",
            "gas_price": 1,
            "body_hash": "c3bee9451ed6d78ebc91387c18ff3c2013ec02c56cf5cd867ba32987850b78a8",
            "dependencies": [
            "be5fdeea0240e999e376f8ecbce1bd4fd9336f58dae4a5842558a4da6ad35aa8",
            "168d7ea9c88e76b3eef72759f2a7af24663cc871a469c7ba1387ca479e82fb41"
            ],
            "chain_name": "casper-test-chain-name-1"
        },
        "payment": {
            "StoredVersionedContractByHash": {
            "hash": "09dcee4b212cfd53642ab323fbef07dafafc6f945a80a00147f62910a915c4e6",
            "version": 10,
            "entry_point": "entrypoint",
            "args": [
                [
                "name_01",
                {
                    "cl_type": "Bool",
                    "bytes": "00",
                    "parsed": false
                }
                ],
                [
                "name_02",
                {
                    "cl_type": "I32",
                    "bytes": "2a000000",
                    "parsed": 42
                }
                ]
            ]
            }
        },
        "session": {
            "StoredVersionedContractByHash": {
            "hash": "09dcee4b212cfd53642ab323fbef07dafafc6f945a80a00147f62910a915c4e6",
            "version": 10,
            "entry_point": "entrypoint",
            "args": [
                [
                "name_01",
                {
                    "cl_type": "Bool",
                    "bytes": "00",
                    "parsed": false
                }
                ],
                [
                "name_02",
                {
                    "cl_type": "I32",
                    "bytes": "2a000000",
                    "parsed": 42
                }
                ]
            ]
            }
        },
        "approvals": [
            {
            "signer": "01f60bce2bb1059c41910eac1e7ee6c3ef4c8fcc63a901eb9603c1524cadfb0c18",
            "signature": "013b214df155e9b4a5180c801a9d08fdf4d03c5bafeb32e901c4d785bdcba5d862d039d2f335b192d9eedf71f79deb797c006006993175515531d69ebe9dbcef0a"
            },
            {
            "signer": "0148c2b17e1108f709c65aea7ed741f272b93adfe366584eadc4909bd666c2e0ce",
            "signature": "0179695d45022b5768a6de880db9add1b8590f7ca92da758b2c7c80bf6d876041657440b0cb5cd06707338a7de60c304a48eb1a7dd84e3ac3e9636a5c7fab29d01"
            }
        ]
    }"#;

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
        )
        .unwrap();
        deploy.write_deploy(&mut output).unwrap();

        // The test output can be used to generate data for SAMPLE_DEPLOY:
        // let secret_key = SecretKey::generate_ed25519().unwrap();
        // deploy.sign(&secret_key, &mut casper_node::new_rng());
        // println!("{}", serde_json::to_string_pretty(&deploy).unwrap());

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
    fn should_fail_to_create_large_deploy() {
        let deploy_params = deploy_params();
        let payment_params =
            PaymentStrParams::with_package_hash(PKG_HASH, VERSION, ENTRYPOINT, args_simple(), "");
        // Create a string arg of 1048576 letter 'a's to ensure the deploy is greater than 1048576
        // bytes.
        let large_args_simple = format!("name_01:string='{:a<1048576}'", "");

        let session_params = SessionStrParams::with_package_hash(
            PKG_HASH,
            VERSION,
            ENTRYPOINT,
            vec![large_args_simple.as_str()],
            "",
        );

        match Deploy::with_payment_and_session(
            deploy_params.try_into().unwrap(),
            payment_params.try_into().unwrap(),
            session_params.try_into().unwrap(),
        ) {
            Err(Error::DeploySizeTooLarge(ExcessiveSizeDeployError {
                max_deploy_size,
                actual_deploy_size,
            })) => {
                assert_eq!(max_deploy_size, MAX_SERIALIZED_SIZE);
                assert!(actual_deploy_size > MAX_SERIALIZED_SIZE as usize);
            }
            Err(error) => panic!("unexpected error: {}", error),
            Ok(_) => panic!("failed to error while creating an excessively large deploy"),
        }
    }

    #[test]
    fn should_read_deploy() {
        let bytes = SAMPLE_DEPLOY.as_bytes();
        assert!(matches!(Deploy::read_deploy(bytes), Ok(_)));
    }

    #[test]
    fn should_sign_deploy() {
        let bytes = SAMPLE_DEPLOY.as_bytes();
        let mut deploy = Deploy::read_deploy(bytes).unwrap();
        deploy
            .is_valid()
            .unwrap_or_else(|error| panic!("{} - {:#?}", error, deploy));
        assert_eq!(
            deploy.approvals().len(),
            2,
            "Sample deploy should have 2 approvals."
        );

        let mut result = Vec::new();
        let secret_key = SecretKey::generate_ed25519().unwrap();
        Deploy::sign_and_write_deploy(bytes, secret_key, &mut result).unwrap();
        let signed_deploy = Deploy::read_deploy(&result[..]).unwrap();

        assert_eq!(
            signed_deploy.approvals().len(),
            deploy.approvals().len() + 1,
            "deploy should be is_valid() because it has been signed {:#?}",
            signed_deploy
        );
    }
}
