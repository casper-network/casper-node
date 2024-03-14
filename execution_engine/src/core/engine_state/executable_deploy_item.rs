//! Units of execution.
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
use rand::{
    distributions::{Alphanumeric, Distribution, Standard},
    Rng,
};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use casper_hashing::Digest;
use casper_types::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    contracts::{ContractVersion, NamedKeys, DEFAULT_ENTRY_POINT_NAME},
    system::mint::ARG_AMOUNT,
    CLValue, ContractHash, ContractPackage, ContractPackageHash, ContractVersionKey, Gas, Key,
    Motes, Phase, ProtocolVersion, RuntimeArgs, StoredValue, U512,
};

use crate::{
    core::{
        engine_state::{Error, ExecError, MAX_PAYMENT_AMOUNT},
        execution,
        tracking_copy::{TrackingCopy, TrackingCopyExt},
    },
    shared::newtypes::CorrelationId,
    storage::global_state::StateReader,
};

const TAG_LENGTH: usize = U8_SERIALIZED_LENGTH;
const MODULE_BYTES_TAG: u8 = 0;
const STORED_CONTRACT_BY_HASH_TAG: u8 = 1;
const STORED_CONTRACT_BY_NAME_TAG: u8 = 2;
const STORED_VERSIONED_CONTRACT_BY_HASH_TAG: u8 = 3;
const STORED_VERSIONED_CONTRACT_BY_NAME_TAG: u8 = 4;
const TRANSFER_TAG: u8 = 5;

/// Possible ways to identify the `ExecutableDeployItem`.
#[derive(
    Clone, DataSize, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum ExecutableDeployItemIdentifier {
    /// The deploy item is of the type [`ExecutableDeployItem::ModuleBytes`]
    Module,
    /// The deploy item is a variation of a stored contract.
    Contract(ContractIdentifier),
    /// The deploy item is a variation of a stored contract package.
    Package(ContractPackageIdentifier),
    /// The deploy item is a native transfer.
    Transfer,
}

/// Possible ways to identify the contract object within an `ExecutableDeployItem`.
#[derive(
    Clone, DataSize, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum ContractIdentifier {
    /// The contract object within the deploy item is identified by name.
    Name(String),
    /// The contract object within the deploy item is identified by its hash.
    Hash(ContractHash),
}

/// Possible ways to identify the contract package object within an `ExecutableDeployItem`.
#[derive(
    Clone, DataSize, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum ContractPackageIdentifier {
    /// The stored contract package within the deploy item is identified by name.
    Name {
        /// Name of the contract package.
        name: String,
        /// The version specified in the deploy item.
        version: Option<ContractVersion>,
    },
    /// The stored contract package within the deploy item is identified by hash.
    Hash {
        /// Hash of the contract package.
        contract_package_hash: ContractPackageHash,
        /// The version specified in the deploy item.
        version: Option<ContractVersion>,
    },
}

impl ContractPackageIdentifier {
    /// Returns the version of the contract package specified in the deploy item.
    pub fn version(&self) -> Option<ContractVersion> {
        match self {
            ContractPackageIdentifier::Name { version, .. } => *version,
            ContractPackageIdentifier::Hash { version, .. } => *version,
        }
    }
}

/// Represents possible variants of an executable deploy.
#[derive(
    Clone, DataSize, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub enum ExecutableDeployItem {
    /// Executable specified as raw bytes that represent WASM code and an instance of
    /// [`RuntimeArgs`].
    ModuleBytes {
        /// Raw WASM module bytes with assumed "call" export as an entrypoint.
        #[serde(with = "HexForm")]
        #[schemars(with = "String", description = "Hex-encoded raw Wasm bytes.")]
        module_bytes: Bytes,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
    /// Stored contract referenced by its [`ContractHash`], entry point and an instance of
    /// [`RuntimeArgs`].
    StoredContractByHash {
        /// Contract hash.
        #[serde(with = "contract_hash_as_digest")]
        #[schemars(with = "String", description = "Hex-encoded hash.")]
        hash: ContractHash,
        /// Name of an entry point.
        entry_point: String,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
    /// Stored contract referenced by a named key existing in the signer's account context, entry
    /// point and an instance of [`RuntimeArgs`].
    StoredContractByName {
        /// Named key.
        name: String,
        /// Name of an entry point.
        entry_point: String,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
    /// Stored versioned contract referenced by its [`ContractPackageHash`], entry point and an
    /// instance of [`RuntimeArgs`].
    StoredVersionedContractByHash {
        /// Contract package hash
        #[serde(with = "contract_package_hash_as_digest")]
        #[schemars(with = "String", description = "Hex-encoded hash.")]
        hash: ContractPackageHash,
        /// An optional version of the contract to call. It will default to the highest enabled
        /// version if no value is specified.
        version: Option<ContractVersion>,
        /// Entry point name.
        entry_point: String,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
    /// Stored versioned contract referenced by a named key existing in the signer's account
    /// context, entry point and an instance of [`RuntimeArgs`].
    StoredVersionedContractByName {
        /// Named key.
        name: String,
        /// An optional version of the contract to call. It will default to the highest enabled
        /// version if no value is specified.
        version: Option<ContractVersion>,
        /// Entry point name.
        entry_point: String,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
    /// A native transfer which does not contain or reference a WASM code.
    Transfer {
        /// Runtime arguments.
        args: RuntimeArgs,
    },
}

mod contract_hash_as_digest {
    use super::*;

    pub(super) fn serialize<S: Serializer>(
        contract_hash: &ContractHash,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        Digest::from(contract_hash.value()).serialize(serializer)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<ContractHash, D::Error> {
        let digest = Digest::deserialize(deserializer)?;
        Ok(ContractHash::new(digest.value()))
    }
}

mod contract_package_hash_as_digest {
    use super::*;

    pub(super) fn serialize<S: Serializer>(
        contract_package_hash: &ContractPackageHash,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        Digest::from(contract_package_hash.value()).serialize(serializer)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<ContractPackageHash, D::Error> {
        let digest = Digest::deserialize(deserializer)?;
        Ok(ContractPackageHash::new(digest.value()))
    }
}

impl ExecutableDeployItem {
    /// Returns the entry point name.
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

    /// Returns the identifier of the ExecutableDeployItem.
    pub fn identifier(&self) -> ExecutableDeployItemIdentifier {
        match self {
            ExecutableDeployItem::ModuleBytes { .. } => ExecutableDeployItemIdentifier::Module,
            ExecutableDeployItem::StoredContractByHash { hash, .. } => {
                ExecutableDeployItemIdentifier::Contract(ContractIdentifier::Hash(*hash))
            }
            ExecutableDeployItem::StoredContractByName { name, .. } => {
                ExecutableDeployItemIdentifier::Contract(ContractIdentifier::Name(name.to_string()))
            }
            ExecutableDeployItem::StoredVersionedContractByHash { hash, version, .. } => {
                ExecutableDeployItemIdentifier::Package(ContractPackageIdentifier::Hash {
                    contract_package_hash: *hash,
                    version: *version,
                })
            }
            ExecutableDeployItem::StoredVersionedContractByName { name, version, .. } => {
                ExecutableDeployItemIdentifier::Package(ContractPackageIdentifier::Name {
                    name: name.to_string(),
                    version: *version,
                })
            }
            ExecutableDeployItem::Transfer { .. } => ExecutableDeployItemIdentifier::Transfer,
        }
    }

    /// Returns the identifier of the contract present in the deploy item, if present.
    pub fn contract_identifier(&self) -> Option<ContractIdentifier> {
        match self {
            ExecutableDeployItem::ModuleBytes { .. }
            | ExecutableDeployItem::StoredVersionedContractByHash { .. }
            | ExecutableDeployItem::StoredVersionedContractByName { .. }
            | ExecutableDeployItem::Transfer { .. } => None,

            ExecutableDeployItem::StoredContractByName { name, .. } => {
                Some(ContractIdentifier::Name(name.to_string()))
            }
            ExecutableDeployItem::StoredContractByHash { hash, .. } => {
                Some(ContractIdentifier::Hash(*hash))
            }
        }
    }

    /// Returns the identifier of the contract package present in the deploy item, if present.
    pub fn contract_package_identifier(&self) -> Option<ContractPackageIdentifier> {
        match self {
            ExecutableDeployItem::ModuleBytes { .. }
            | ExecutableDeployItem::StoredContractByHash { .. }
            | ExecutableDeployItem::StoredContractByName { .. }
            | ExecutableDeployItem::Transfer { .. } => None,

            ExecutableDeployItem::StoredVersionedContractByName { name, version, .. } => {
                Some(ContractPackageIdentifier::Name {
                    name: name.clone(),
                    version: *version,
                })
            }
            ExecutableDeployItem::StoredVersionedContractByHash { hash, version, .. } => {
                Some(ContractPackageIdentifier::Hash {
                    contract_package_hash: *hash,
                    version: *version,
                })
            }
        }
    }

    /// Returns the runtime arguments.
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

    /// Returns the amount from args (if any) as Gas.
    pub fn payment_amount(&self, conv_rate: u64) -> Option<Gas> {
        match self.args().get(ARG_AMOUNT) {
            Some(val) => Gas::from_motes(Motes::new(val.clone().into_t::<U512>().ok()?), conv_rate),
            None => None,
        }
    }

    /// Checks if this deploy item is a native transfer.
    pub fn is_transfer(&self) -> bool {
        matches!(self, ExecutableDeployItem::Transfer { .. })
    }

    /// Checks if this deploy is a standard payment.
    pub fn is_standard_payment(&self, phase: Phase) -> bool {
        if phase != Phase::Payment {
            return false;
        }

        if let ExecutableDeployItem::ModuleBytes { module_bytes, .. } = self {
            return module_bytes.is_empty();
        }

        false
    }

    /// Checks if the deploy item is a contract identified by its name.
    pub fn is_by_name(&self) -> bool {
        matches!(
            self,
            ExecutableDeployItem::StoredVersionedContractByName { .. }
        ) || matches!(self, ExecutableDeployItem::StoredContractByName { .. })
    }

    /// Returns the name of the contract or contract package,
    /// if the deploy item is identified by name.
    pub fn by_name(&self) -> Option<String> {
        match self {
            ExecutableDeployItem::StoredContractByName { name, .. }
            | ExecutableDeployItem::StoredVersionedContractByName { name, .. } => {
                Some(name.clone())
            }
            ExecutableDeployItem::ModuleBytes { .. }
            | ExecutableDeployItem::StoredContractByHash { .. }
            | ExecutableDeployItem::StoredVersionedContractByHash { .. }
            | ExecutableDeployItem::Transfer { .. } => None,
        }
    }

    /// Checks if the deploy item is a stored contract.
    pub fn is_stored_contract(&self) -> bool {
        matches!(self, ExecutableDeployItem::StoredContractByHash { .. })
            || matches!(self, ExecutableDeployItem::StoredContractByName { .. })
    }

    /// Checks if the deploy item is a stored contract package.
    pub fn is_stored_contract_package(&self) -> bool {
        matches!(
            self,
            ExecutableDeployItem::StoredVersionedContractByHash { .. }
        ) || matches!(
            self,
            ExecutableDeployItem::StoredVersionedContractByName { .. }
        )
    }

    /// Returns `true` if the executable deploy item is [`ModuleBytes`].
    ///
    /// [`ModuleBytes`]: ExecutableDeployItem::ModuleBytes
    pub fn is_module_bytes(&self) -> bool {
        matches!(self, Self::ModuleBytes { .. })
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
                .field("hash", &base16::encode_lower(hash))
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
                .field("hash", &base16::encode_lower(hash))
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

/// Represents a kind of a deploy.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DeployKind {
    /// Session code.
    Session,
    /// Contract code.
    Contract,
    /// System contract.
    System,
}

/// The type of execution about to be performed.
#[derive(Clone, Debug)]
pub enum ExecutionKind {
    /// Wasm bytes.
    Module(Bytes),
    /// Stored contract.
    Contract {
        /// Contract's hash.
        contract_hash: ContractHash,
        /// Entry point's name.
        entry_point_name: String,
    },
}

impl ExecutionKind {
    /// Returns a new module variant of `ExecutionKind`.
    pub fn new_module(module_bytes: Bytes) -> Self {
        ExecutionKind::Module(module_bytes)
    }

    /// Returns a new contract variant of `ExecutionKind`.
    pub fn new_contract(contract_hash: ContractHash, entry_point_name: String) -> Self {
        ExecutionKind::Contract {
            contract_hash,
            entry_point_name,
        }
    }

    /// Returns all the details necessary for execution.
    ///
    /// This object is generated based on information provided by [`ExecutableDeployItem`].
    pub fn new<R>(
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
        named_keys: &NamedKeys,
        executable_deploy_item: ExecutableDeployItem,
        correlation_id: CorrelationId,
        protocol_version: &ProtocolVersion,
        phase: Phase,
    ) -> Result<ExecutionKind, Error>
    where
        R: StateReader<Key, StoredValue>,
        R::Error: Into<ExecError>,
    {
        let contract_hash: ContractHash;
        let contract_package: ContractPackage;

        let is_payment_phase = phase == Phase::Payment;

        match executable_deploy_item {
            ExecutableDeployItem::Transfer { .. } => {
                Err(Error::InvalidDeployItemVariant("Transfer".into()))
            }
            ExecutableDeployItem::ModuleBytes { module_bytes, .. }
                if module_bytes.is_empty() && is_payment_phase =>
            {
                Err(Error::InvalidDeployItemVariant(
                    "Empty module bytes for custom payment".into(),
                ))
            }
            ExecutableDeployItem::ModuleBytes { module_bytes, .. } => {
                Ok(ExecutionKind::new_module(module_bytes))
            }
            ExecutableDeployItem::StoredContractByHash {
                hash, entry_point, ..
            } => Ok(ExecutionKind::new_contract(hash, entry_point)),
            ExecutableDeployItem::StoredContractByName {
                name, entry_point, ..
            } => {
                let contract_key = named_keys.get(&name).cloned().ok_or_else(|| {
                    Error::Exec(execution::Error::NamedKeyNotFound(name.to_string()))
                })?;

                contract_hash =
                    ContractHash::new(contract_key.into_hash().ok_or(Error::InvalidKeyVariant)?);

                Ok(ExecutionKind::new_contract(contract_hash, entry_point))
            }
            ExecutableDeployItem::StoredVersionedContractByName {
                name,
                version,
                entry_point,
                ..
            } => {
                let contract_package_hash: ContractPackageHash = {
                    named_keys
                        .get(&name)
                        .cloned()
                        .ok_or_else(|| {
                            Error::Exec(execution::Error::NamedKeyNotFound(name.to_string()))
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
                    .ok_or(Error::Exec(execution::Error::NoActiveContractVersions(
                        contract_package_hash,
                    )))?;

                if !contract_package.is_version_enabled(contract_version_key) {
                    return Err(Error::Exec(execution::Error::InvalidContractVersion(
                        contract_version_key,
                    )));
                }

                let looked_up_contract_hash: ContractHash = contract_package
                    .lookup_contract_hash(contract_version_key)
                    .ok_or(Error::Exec(execution::Error::InvalidContractVersion(
                        contract_version_key,
                    )))?
                    .to_owned();

                Ok(ExecutionKind::new_contract(
                    looked_up_contract_hash,
                    entry_point,
                ))
            }
            ExecutableDeployItem::StoredVersionedContractByHash {
                hash: contract_package_hash,
                version,
                entry_point,
                ..
            } => {
                contract_package = tracking_copy
                    .borrow_mut()
                    .get_contract_package(correlation_id, contract_package_hash)?;

                let maybe_version_key =
                    version.map(|ver| ContractVersionKey::new(protocol_version.value().major, ver));

                let contract_version_key = maybe_version_key
                    .or_else(|| contract_package.current_contract_version())
                    .ok_or(Error::Exec(execution::Error::NoActiveContractVersions(
                        contract_package_hash,
                    )))?;

                if !contract_package.is_version_enabled(contract_version_key) {
                    return Err(Error::Exec(execution::Error::InvalidContractVersion(
                        contract_version_key,
                    )));
                }

                let looked_up_contract_hash = *contract_package
                    .lookup_contract_hash(contract_version_key)
                    .ok_or(Error::Exec(execution::Error::InvalidContractVersion(
                        contract_version_key,
                    )))?;

                Ok(ExecutionKind::new_contract(
                    looked_up_contract_hash,
                    entry_point,
                ))
            }
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
