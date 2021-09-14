// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use std::{
    cell::RefCell,
    fmt::{self, Debug, Display, Formatter},
    rc::Rc,
};

use datasize::DataSize;
use hex_buffer_serde::{Hex, HexForm};
use hex_fmt::HexFmt;
use parity_wasm::elements::Module;
use rand::{
    distributions::{Alphanumeric, Distribution, Standard},
    Rng,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use casper_types::{
    account::{Account, AccountHash},
    bytesrepr::{self, Bytes, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    checksummed_hex::{self, CheckSummedHex, CheckSummedHexForm},
    contracts::{ContractVersion, DEFAULT_ENTRY_POINT_NAME},
    system::{mint::ARG_AMOUNT, CallStackElement, HANDLE_PAYMENT, STANDARD_PAYMENT},
    CLValue, Contract, ContractHash, ContractPackage, ContractPackageHash, ContractVersionKey,
    EntryPoint, EntryPointType, Key, Phase, ProtocolVersion, RuntimeArgs, StoredValue, U512,
};

use super::error;
use crate::{
    core::{
        engine_state::{genesis::SystemContractRegistry, Error, ExecError, MAX_PAYMENT_AMOUNT},
        execution,
        tracking_copy::{TrackingCopy, TrackingCopyExt},
    },
    shared::{newtypes::CorrelationId, wasm, wasm_prep, wasm_prep::Preprocessor},
    storage::global_state::StateReader,
};

const TAG_LENGTH: usize = U8_SERIALIZED_LENGTH;
const MODULE_BYTES_TAG: u8 = 0;
const STORED_CONTRACT_BY_HASH_TAG: u8 = 1;
const STORED_CONTRACT_BY_NAME_TAG: u8 = 2;
const STORED_VERSIONED_CONTRACT_BY_HASH_TAG: u8 = 3;
const STORED_VERSIONED_CONTRACT_BY_NAME_TAG: u8 = 4;
const TRANSFER_TAG: u8 = 5;

#[derive(
    Clone, DataSize, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub enum ExecutableDeployItem {
    ModuleBytes {
        #[serde(with = "HexForm")]
        #[schemars(with = "String", description = "Hex-encoded raw Wasm bytes.")]
        module_bytes: Bytes,
        // assumes implicit `call` noarg entrypoint
        args: RuntimeArgs,
    },
    StoredContractByHash {
        #[serde(with = "CheckSummedHexForm")]
        #[schemars(with = "String", description = "Checksummed hex-encoded hash.")]
        hash: ContractHash,
        entry_point: String,
        args: RuntimeArgs,
    },
    StoredContractByName {
        name: String,
        entry_point: String,
        args: RuntimeArgs,
    },
    StoredVersionedContractByHash {
        #[serde(with = "CheckSummedHexForm")]
        #[schemars(with = "String", description = "Checksummed hex-encoded hash.")]
        hash: ContractPackageHash,
        version: Option<ContractVersion>, // defaults to highest enabled version
        entry_point: String,
        args: RuntimeArgs,
    },
    StoredVersionedContractByName {
        name: String,
        version: Option<ContractVersion>, // defaults to highest enabled version
        entry_point: String,
        args: RuntimeArgs,
    },
    Transfer {
        args: RuntimeArgs,
    },
}

impl ExecutableDeployItem {
    pub fn entry_point_name(&self) -> &str {
        match self {
            ExecutableDeployItem::ModuleBytes { .. } | ExecutableDeployItem::Transfer { .. } => {
                DEFAULT_ENTRY_POINT_NAME
            }
            ExecutableDeployItem::StoredVersionedContractByName { entry_point, .. }
            | ExecutableDeployItem::StoredVersionedContractByHash { entry_point, .. }
            | ExecutableDeployItem::StoredContractByHash { entry_point, .. }
            | ExecutableDeployItem::StoredContractByName { entry_point, .. } => entry_point,
        }
    }

    pub fn args(&self) -> &RuntimeArgs {
        match self {
            ExecutableDeployItem::ModuleBytes { args, .. }
            | ExecutableDeployItem::StoredContractByHash { args, .. }
            | ExecutableDeployItem::StoredContractByName { args, .. }
            | ExecutableDeployItem::StoredVersionedContractByHash { args, .. }
            | ExecutableDeployItem::StoredVersionedContractByName { args, .. }
            | ExecutableDeployItem::Transfer { args } => args,
        }
    }

    pub fn is_transfer(&self) -> bool {
        matches!(self, ExecutableDeployItem::Transfer { .. })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn get_deploy_metadata<R>(
        &self,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
        account: &Account,
        correlation_id: CorrelationId,
        preprocessor: &Preprocessor,
        protocol_version: &ProtocolVersion,
        system_contract_registry: SystemContractRegistry,
        phase: Phase,
    ) -> Result<DeployMetadata, Error>
    where
        R: StateReader<Key, StoredValue>,
        R::Error: Into<ExecError>,
    {
        let contract_hash: ContractHash;
        let contract_package: ContractPackage;
        let contract: Contract;
        let base_key: Key;

        let account_hash = account.account_hash();

        match self {
            ExecutableDeployItem::Transfer { .. } => {
                return Err(error::Error::InvalidDeployItemVariant("Transfer".into()))
            }
            ExecutableDeployItem::ModuleBytes { module_bytes, .. }
                if module_bytes.is_empty() && phase == Phase::Payment =>
            {
                let base_key = account_hash.into();
                let contract_hash =
                    *system_contract_registry
                        .get(STANDARD_PAYMENT)
                        .ok_or_else(|| {
                            error!("Missing handle payment contract hash");
                            Error::MissingSystemContractHash(HANDLE_PAYMENT.to_string())
                        })?;
                let module = wasm::do_nothing_module(preprocessor)?;
                return Ok(DeployMetadata {
                    kind: DeployKind::System,
                    account_hash,
                    base_key,
                    module,
                    contract_hash,
                    contract: Default::default(),
                    contract_package: Default::default(),
                    entry_point: Default::default(),
                    is_stored: true,
                });
            }
            ExecutableDeployItem::ModuleBytes { module_bytes, .. } => {
                let base_key = account_hash.into();
                let module = preprocessor.preprocess(module_bytes.as_ref())?;
                return Ok(DeployMetadata {
                    kind: DeployKind::Session,
                    account_hash,
                    base_key,
                    module,
                    contract_hash: Default::default(),
                    contract: Default::default(),
                    contract_package: Default::default(),
                    entry_point: Default::default(),
                    is_stored: false,
                });
            }
            ExecutableDeployItem::StoredContractByHash { hash, .. } => {
                base_key = Key::Hash(hash.value());
                contract_hash = *hash;
                contract = tracking_copy
                    .borrow_mut()
                    .get_contract(correlation_id, contract_hash)?;

                if !contract.is_compatible_protocol_version(*protocol_version) {
                    let exec_error = execution::Error::IncompatibleProtocolMajorVersion {
                        expected: protocol_version.value().major,
                        actual: contract.protocol_version().value().major,
                    };
                    return Err(error::Error::Exec(exec_error));
                }

                contract_package = tracking_copy
                    .borrow_mut()
                    .get_contract_package(correlation_id, contract.contract_package_hash())?;
            }
            ExecutableDeployItem::StoredContractByName { name, .. } => {
                // `ContractHash` is stored in named keys.
                base_key = account.named_keys().get(name).cloned().ok_or_else(|| {
                    error::Error::Exec(execution::Error::NamedKeyNotFound(name.to_string()))
                })?;

                contract_hash =
                    ContractHash::new(base_key.into_hash().ok_or(Error::InvalidKeyVariant)?);

                contract = tracking_copy
                    .borrow_mut()
                    .get_contract(correlation_id, contract_hash)?;

                if !contract.is_compatible_protocol_version(*protocol_version) {
                    let exec_error = execution::Error::IncompatibleProtocolMajorVersion {
                        expected: protocol_version.value().major,
                        actual: contract.protocol_version().value().major,
                    };
                    return Err(error::Error::Exec(exec_error));
                }

                contract_package = tracking_copy
                    .borrow_mut()
                    .get_contract_package(correlation_id, contract.contract_package_hash())?;
            }
            ExecutableDeployItem::StoredVersionedContractByName { name, version, .. } => {
                // `ContractPackageHash` is stored in named keys.
                let contract_package_hash: ContractPackageHash = {
                    account
                        .named_keys()
                        .get(name)
                        .cloned()
                        .ok_or_else(|| {
                            error::Error::Exec(execution::Error::NamedKeyNotFound(name.to_string()))
                        })?
                        .into_hash()
                        .ok_or(Error::InvalidKeyVariant)?
                        .into()
                };

                contract_package = tracking_copy
                    .borrow_mut()
                    .get_contract_package(correlation_id, contract_package_hash)?;

                let maybe_version_key =
                    version.map(|ver| ContractVersionKey::new(protocol_version.value().major, ver));

                let contract_version_key = maybe_version_key
                    .or_else(|| contract_package.current_contract_version())
                    .ok_or(error::Error::Exec(
                        execution::Error::NoActiveContractVersions(contract_package_hash),
                    ))?;

                if !contract_package.is_version_enabled(contract_version_key) {
                    return Err(error::Error::Exec(
                        execution::Error::InvalidContractVersion(contract_version_key),
                    ));
                }

                let looked_up_contract_hash: ContractHash = contract_package
                    .lookup_contract_hash(contract_version_key)
                    .ok_or(error::Error::Exec(
                        execution::Error::InvalidContractVersion(contract_version_key),
                    ))?
                    .to_owned();

                contract = tracking_copy
                    .borrow_mut()
                    .get_contract(correlation_id, looked_up_contract_hash)?;

                base_key = looked_up_contract_hash.into();
                contract_hash = looked_up_contract_hash;
            }
            ExecutableDeployItem::StoredVersionedContractByHash {
                hash: contract_package_hash,
                version,
                ..
            } => {
                contract_package = tracking_copy
                    .borrow_mut()
                    .get_contract_package(correlation_id, *contract_package_hash)?;

                let maybe_version_key =
                    version.map(|ver| ContractVersionKey::new(protocol_version.value().major, ver));

                let contract_version_key = maybe_version_key
                    .or_else(|| contract_package.current_contract_version())
                    .ok_or_else(|| {
                        error::Error::Exec(execution::Error::NoActiveContractVersions(
                            *contract_package_hash,
                        ))
                    })?;

                if !contract_package.is_version_enabled(contract_version_key) {
                    return Err(error::Error::Exec(
                        execution::Error::InvalidContractVersion(contract_version_key),
                    ));
                }

                let looked_up_contract_hash = *contract_package
                    .lookup_contract_hash(contract_version_key)
                    .ok_or(error::Error::Exec(
                        execution::Error::InvalidContractVersion(contract_version_key),
                    ))?;
                contract = tracking_copy
                    .borrow_mut()
                    .get_contract(correlation_id, looked_up_contract_hash)?;

                base_key = looked_up_contract_hash.into();
                contract_hash = looked_up_contract_hash;
            }
        };

        let entry_point_name = self.entry_point_name();

        let entry_point = contract
            .entry_point(entry_point_name)
            .cloned()
            .ok_or_else(|| {
                error::Error::Exec(execution::Error::NoSuchMethod(entry_point_name.to_owned()))
            })?;

        if system_contract_registry
            .values()
            .any(|value| *value == contract_hash)
        {
            let module = wasm::do_nothing_module(preprocessor)?;
            return Ok(DeployMetadata {
                kind: DeployKind::System,
                account_hash,
                base_key,
                module,
                contract_hash,
                contract,
                contract_package,
                entry_point,
                is_stored: true,
            });
        }

        let contract_wasm = tracking_copy
            .borrow_mut()
            .get_contract_wasm(correlation_id, contract.contract_wasm_hash())?;

        let module = wasm_prep::deserialize(contract_wasm.bytes())?;

        match entry_point.entry_point_type() {
            EntryPointType::Session => {
                let base_key = account.account_hash().into();
                Ok(DeployMetadata {
                    kind: DeployKind::Session,
                    account_hash,
                    base_key,
                    module,
                    contract_hash,
                    contract,
                    contract_package,
                    entry_point,
                    is_stored: true,
                })
            }
            EntryPointType::Contract => Ok(DeployMetadata {
                kind: DeployKind::Contract,
                account_hash,
                base_key,
                module,
                contract_hash,
                contract,
                contract_package,
                entry_point,
                is_stored: true,
            }),
        }
    }
}

impl ToBytes for ExecutableDeployItem {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        match self {
            ExecutableDeployItem::ModuleBytes { module_bytes, args } => {
                buffer.insert(0, MODULE_BYTES_TAG);
                buffer.extend(module_bytes.to_bytes()?);
                buffer.extend(args.to_bytes()?);
            }
            ExecutableDeployItem::StoredContractByHash {
                hash,
                entry_point,
                args,
            } => {
                buffer.insert(0, STORED_CONTRACT_BY_HASH_TAG);
                buffer.extend(hash.to_bytes()?);
                buffer.extend(entry_point.to_bytes()?);
                buffer.extend(args.to_bytes()?)
            }
            ExecutableDeployItem::StoredContractByName {
                name,
                entry_point,
                args,
            } => {
                buffer.insert(0, STORED_CONTRACT_BY_NAME_TAG);
                buffer.extend(name.to_bytes()?);
                buffer.extend(entry_point.to_bytes()?);
                buffer.extend(args.to_bytes()?)
            }
            ExecutableDeployItem::StoredVersionedContractByHash {
                hash,
                version,
                entry_point,
                args,
            } => {
                buffer.insert(0, STORED_VERSIONED_CONTRACT_BY_HASH_TAG);
                buffer.extend(hash.to_bytes()?);
                buffer.extend(version.to_bytes()?);
                buffer.extend(entry_point.to_bytes()?);
                buffer.extend(args.to_bytes()?)
            }
            ExecutableDeployItem::StoredVersionedContractByName {
                name,
                version,
                entry_point,
                args,
            } => {
                buffer.insert(0, STORED_VERSIONED_CONTRACT_BY_NAME_TAG);
                buffer.extend(name.to_bytes()?);
                buffer.extend(version.to_bytes()?);
                buffer.extend(entry_point.to_bytes()?);
                buffer.extend(args.to_bytes()?)
            }
            ExecutableDeployItem::Transfer { args } => {
                buffer.insert(0, TRANSFER_TAG);
                buffer.extend(args.to_bytes()?)
            }
        }
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        TAG_LENGTH
            + match self {
                ExecutableDeployItem::ModuleBytes { module_bytes, args } => {
                    module_bytes.serialized_length() + args.serialized_length()
                }
                ExecutableDeployItem::StoredContractByHash {
                    hash,
                    entry_point,
                    args,
                } => {
                    hash.serialized_length()
                        + entry_point.serialized_length()
                        + args.serialized_length()
                }
                ExecutableDeployItem::StoredContractByName {
                    name,
                    entry_point,
                    args,
                } => {
                    name.serialized_length()
                        + entry_point.serialized_length()
                        + args.serialized_length()
                }
                ExecutableDeployItem::StoredVersionedContractByHash {
                    hash,
                    version,
                    entry_point,
                    args,
                } => {
                    hash.serialized_length()
                        + version.serialized_length()
                        + entry_point.serialized_length()
                        + args.serialized_length()
                }
                ExecutableDeployItem::StoredVersionedContractByName {
                    name,
                    version,
                    entry_point,
                    args,
                } => {
                    name.serialized_length()
                        + version.serialized_length()
                        + entry_point.serialized_length()
                        + args.serialized_length()
                }
                ExecutableDeployItem::Transfer { args } => args.serialized_length(),
            }
    }
}

impl FromBytes for ExecutableDeployItem {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            MODULE_BYTES_TAG => {
                let (module_bytes, remainder) = FromBytes::from_bytes(remainder)?;
                let (args, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((
                    ExecutableDeployItem::ModuleBytes { module_bytes, args },
                    remainder,
                ))
            }
            STORED_CONTRACT_BY_HASH_TAG => {
                let (hash, remainder) = FromBytes::from_bytes(remainder)?;
                let (entry_point, remainder) = String::from_bytes(remainder)?;
                let (args, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((
                    ExecutableDeployItem::StoredContractByHash {
                        hash,
                        entry_point,
                        args,
                    },
                    remainder,
                ))
            }
            STORED_CONTRACT_BY_NAME_TAG => {
                let (name, remainder) = String::from_bytes(remainder)?;
                let (entry_point, remainder) = String::from_bytes(remainder)?;
                let (args, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((
                    ExecutableDeployItem::StoredContractByName {
                        name,
                        entry_point,
                        args,
                    },
                    remainder,
                ))
            }
            STORED_VERSIONED_CONTRACT_BY_HASH_TAG => {
                let (hash, remainder) = FromBytes::from_bytes(remainder)?;
                let (version, remainder) = Option::<ContractVersion>::from_bytes(remainder)?;
                let (entry_point, remainder) = String::from_bytes(remainder)?;
                let (args, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((
                    ExecutableDeployItem::StoredVersionedContractByHash {
                        hash,
                        version,
                        entry_point,
                        args,
                    },
                    remainder,
                ))
            }
            STORED_VERSIONED_CONTRACT_BY_NAME_TAG => {
                let (name, remainder) = String::from_bytes(remainder)?;
                let (version, remainder) = Option::<ContractVersion>::from_bytes(remainder)?;
                let (entry_point, remainder) = String::from_bytes(remainder)?;
                let (args, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((
                    ExecutableDeployItem::StoredVersionedContractByName {
                        name,
                        version,
                        entry_point,
                        args,
                    },
                    remainder,
                ))
            }
            TRANSFER_TAG => {
                let (args, remainder) = FromBytes::from_bytes(remainder)?;
                Ok((ExecutableDeployItem::Transfer { args }, remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

impl Display for ExecutableDeployItem {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ExecutableDeployItem::ModuleBytes { module_bytes, .. } => {
                write!(f, "module-bytes [{} bytes]", module_bytes.len())
            }
            ExecutableDeployItem::StoredContractByHash {
                hash, entry_point, ..
            } => write!(
                f,
                "stored-contract-by-hash: {:10}, entry-point: {}",
                HexFmt(hash),
                entry_point,
            ),
            ExecutableDeployItem::StoredContractByName {
                name, entry_point, ..
            } => write!(
                f,
                "stored-contract-by-name: {}, entry-point: {}",
                name, entry_point,
            ),
            ExecutableDeployItem::StoredVersionedContractByHash {
                hash,
                version: Some(ver),
                entry_point,
                ..
            } => write!(
                f,
                "stored-versioned-contract-by-hash: {:10}, version: {}, entry-point: {}",
                HexFmt(hash),
                ver,
                entry_point,
            ),
            ExecutableDeployItem::StoredVersionedContractByHash {
                hash, entry_point, ..
            } => write!(
                f,
                "stored-versioned-contract-by-hash: {:10}, version: latest, entry-point: {}",
                HexFmt(hash),
                entry_point,
            ),
            ExecutableDeployItem::StoredVersionedContractByName {
                name,
                version: Some(ver),
                entry_point,
                ..
            } => write!(
                f,
                "stored-versioned-contract: {}, version: {}, entry-point: {}",
                name, ver, entry_point,
            ),
            ExecutableDeployItem::StoredVersionedContractByName {
                name, entry_point, ..
            } => write!(
                f,
                "stored-versioned-contract: {}, version: latest, entry-point: {}",
                name, entry_point,
            ),
            ExecutableDeployItem::Transfer { .. } => write!(f, "transfer"),
        }
    }
}

impl Debug for ExecutableDeployItem {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ExecutableDeployItem::ModuleBytes { module_bytes, args } => f
                .debug_struct("ModuleBytes")
                .field("module_bytes", &format!("[{} bytes]", module_bytes.len()))
                .field("args", args)
                .finish(),
            ExecutableDeployItem::StoredContractByHash {
                hash,
                entry_point,
                args,
            } => f
                .debug_struct("StoredContractByHash")
                .field("hash", &checksummed_hex::encode(hash))
                .field("entry_point", &entry_point)
                .field("args", args)
                .finish(),
            ExecutableDeployItem::StoredContractByName {
                name,
                entry_point,
                args,
            } => f
                .debug_struct("StoredContractByName")
                .field("name", &name)
                .field("entry_point", &entry_point)
                .field("args", args)
                .finish(),
            ExecutableDeployItem::StoredVersionedContractByHash {
                hash,
                version,
                entry_point,
                args,
            } => f
                .debug_struct("StoredVersionedContractByHash")
                .field("hash", &checksummed_hex::encode(hash))
                .field("version", version)
                .field("entry_point", &entry_point)
                .field("args", args)
                .finish(),
            ExecutableDeployItem::StoredVersionedContractByName {
                name,
                version,
                entry_point,
                args,
            } => f
                .debug_struct("StoredVersionedContractByName")
                .field("name", &name)
                .field("version", version)
                .field("entry_point", &entry_point)
                .field("args", args)
                .finish(),
            ExecutableDeployItem::Transfer { args } => {
                f.debug_struct("Transfer").field("args", args).finish()
            }
        }
    }
}

impl Distribution<ExecutableDeployItem> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> ExecutableDeployItem {
        fn random_bytes<R: Rng + ?Sized>(rng: &mut R) -> Vec<u8> {
            let mut bytes = vec![0u8; rng.gen_range(0..100)];
            rng.fill_bytes(bytes.as_mut());
            bytes
        }

        fn random_string<R: Rng + ?Sized>(rng: &mut R) -> String {
            rng.sample_iter(&Alphanumeric)
                .take(20)
                .map(char::from)
                .collect()
        }

        let mut args = RuntimeArgs::new();
        let _ = args.insert(random_string(rng), Bytes::from(random_bytes(rng)));

        match rng.gen_range(0..5) {
            0 => ExecutableDeployItem::ModuleBytes {
                module_bytes: random_bytes(rng).into(),
                args,
            },
            1 => ExecutableDeployItem::StoredContractByHash {
                hash: ContractHash::new(rng.gen()),
                entry_point: random_string(rng),
                args,
            },
            2 => ExecutableDeployItem::StoredContractByName {
                name: random_string(rng),
                entry_point: random_string(rng),
                args,
            },
            3 => ExecutableDeployItem::StoredVersionedContractByHash {
                hash: ContractPackageHash::new(rng.gen()),
                version: rng.gen(),
                entry_point: random_string(rng),
                args,
            },
            4 => ExecutableDeployItem::StoredVersionedContractByName {
                name: random_string(rng),
                version: rng.gen(),
                entry_point: random_string(rng),
                args,
            },
            5 => {
                let amount = rng.gen_range(MAX_PAYMENT_AMOUNT..1_000_000_000_000_000);
                let mut transfer_args = RuntimeArgs::new();
                transfer_args.insert_cl_value(
                    ARG_AMOUNT,
                    CLValue::from_t(U512::from(amount)).expect("should get CLValue from U512"),
                );
                ExecutableDeployItem::Transfer {
                    args: transfer_args,
                }
            }
            _ => unreachable!(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct DeployMetadata {
    pub kind: DeployKind,
    pub account_hash: AccountHash,
    pub base_key: Key,
    pub module: Module,
    pub contract_hash: ContractHash,
    pub contract: Contract,
    pub contract_package: ContractPackage,
    pub entry_point: EntryPoint,
    pub is_stored: bool,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DeployKind {
    Session,
    Contract,
    System,
}

impl DeployMetadata {
    pub fn take_module(self) -> Module {
        self.module
    }

    pub fn initial_call_stack(&self) -> Result<Vec<CallStackElement>, Error> {
        match (
            self.kind,
            self.entry_point.entry_point_type(),
            self.is_stored,
        ) {
            (DeployKind::Session, EntryPointType::Contract, _) => {
                Err(Error::InvalidDeployItemVariant(
                    "Contract deploy item has invalid 'Session' kind".to_string(),
                ))
            }
            (DeployKind::Session, EntryPointType::Session, false) => {
                Ok(vec![CallStackElement::session(self.account_hash)])
            }
            (DeployKind::Session, EntryPointType::Session, true)
            | (DeployKind::Contract, EntryPointType::Session, true)
            | (DeployKind::System, EntryPointType::Session, true) => {
                let account = self
                    .base_key
                    .into_account()
                    .ok_or(Error::InvalidKeyVariant)?;
                let contract_package_hash = self.contract.contract_package_hash();
                let contract_hash = self.contract_hash;
                Ok(vec![
                    CallStackElement::session(self.account_hash),
                    CallStackElement::stored_session(account, contract_package_hash, contract_hash),
                ])
            }
            (DeployKind::Contract, EntryPointType::Contract, true)
            | (DeployKind::System, EntryPointType::Contract, true) => {
                let contract_package_hash = self.contract.contract_package_hash();
                let contract_hash = self.contract_hash;
                Ok(vec![
                    CallStackElement::session(self.account_hash),
                    CallStackElement::stored_contract(contract_package_hash, contract_hash),
                ])
            }
            (_, _, _) => Err(Error::Deploy),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialization_roundtrip() {
        let mut rng = rand::thread_rng();
        for _ in 0..10 {
            let executable_deploy_item: ExecutableDeployItem = rng.gen();
            bytesrepr::test_serialization_roundtrip(&executable_deploy_item);
        }
    }
}
