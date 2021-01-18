// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use std::fmt::{self, Debug, Display, Formatter};

use datasize::DataSize;
use hex_buffer_serde::{Hex, HexForm};
use hex_fmt::HexFmt;
use rand::{
    distributions::{Alphanumeric, Distribution, Standard},
    Rng,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use casper_types::{
    bytesrepr::{self, Bytes, Error, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    contracts::{ContractVersion, DEFAULT_ENTRY_POINT_NAME},
    ContractHash, ContractPackageHash, Key, RuntimeArgs,
};

use super::error;
use crate::{core::execution, shared::account::Account};

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
        #[serde(with = "HexForm")]
        #[schemars(
            with = "String",
            description = "Hex-encoded contract args, serialized using ToBytes."
        )]
        args: Bytes,
    },
    StoredContractByHash {
        #[serde(with = "HexForm")]
        #[schemars(with = "String", description = "Hex-encoded hash.")]
        hash: ContractHash,
        entry_point: String,
        #[serde(with = "HexForm")]
        #[schemars(
            with = "String",
            description = "Hex-encoded contract args, serialized using ToBytes."
        )]
        args: Bytes,
    },
    StoredContractByName {
        name: String,
        entry_point: String,
        #[serde(with = "HexForm")]
        #[schemars(
            with = "String",
            description = "Hex-encoded contract args, serialized using ToBytes."
        )]
        args: Bytes,
    },
    StoredVersionedContractByHash {
        #[serde(with = "HexForm")]
        #[schemars(with = "String", description = "Hex-encoded hash.")]
        hash: ContractPackageHash,
        version: Option<ContractVersion>, // defaults to highest enabled version
        entry_point: String,
        #[serde(with = "HexForm")]
        #[schemars(
            with = "String",
            description = "Hex-encoded contract args, serialized using ToBytes."
        )]
        args: Bytes,
    },
    StoredVersionedContractByName {
        name: String,
        version: Option<ContractVersion>, // defaults to highest enabled version
        entry_point: String,
        #[serde(with = "HexForm")]
        #[schemars(
            with = "String",
            description = "Hex-encoded contract args, serialized using ToBytes."
        )]
        args: Bytes,
    },
    Transfer {
        #[serde(with = "HexForm")]
        #[schemars(
            with = "String",
            description = "Hex-encoded contract args, serialized using ToBytes."
        )]
        args: Bytes,
    },
}

impl ExecutableDeployItem {
    pub(crate) fn to_contract_hash_key(
        &self,
        account: &Account,
    ) -> Result<Option<Key>, error::Error> {
        match self {
            ExecutableDeployItem::StoredContractByHash { hash, .. } => Ok(Some(Key::from(*hash))),
            ExecutableDeployItem::StoredVersionedContractByHash { hash, .. } => {
                Ok(Some(Key::from(*hash)))
            }
            ExecutableDeployItem::StoredContractByName { name, .. }
            | ExecutableDeployItem::StoredVersionedContractByName { name, .. } => {
                let key = account.named_keys().get(name).cloned().ok_or_else(|| {
                    error::Error::Exec(execution::Error::NamedKeyNotFound(name.to_string()))
                })?;
                Ok(Some(key))
            }
            ExecutableDeployItem::ModuleBytes { .. } | ExecutableDeployItem::Transfer { .. } => {
                Ok(None)
            }
        }
    }

    pub fn into_runtime_args(self) -> Result<RuntimeArgs, Error> {
        match self {
            ExecutableDeployItem::ModuleBytes { args, .. }
            | ExecutableDeployItem::StoredContractByHash { args, .. }
            | ExecutableDeployItem::StoredContractByName { args, .. }
            | ExecutableDeployItem::StoredVersionedContractByHash { args, .. }
            | ExecutableDeployItem::StoredVersionedContractByName { args, .. }
            | ExecutableDeployItem::Transfer { args } => {
                let runtime_args: RuntimeArgs = bytesrepr::deserialize(args.into())?;
                Ok(runtime_args)
            }
        }
    }

    pub fn entry_point_name(&self) -> &str {
        match self {
            ExecutableDeployItem::ModuleBytes { .. } | ExecutableDeployItem::Transfer { .. } => {
                DEFAULT_ENTRY_POINT_NAME
            }
            ExecutableDeployItem::StoredVersionedContractByName { entry_point, .. }
            | ExecutableDeployItem::StoredVersionedContractByHash { entry_point, .. }
            | ExecutableDeployItem::StoredContractByHash { entry_point, .. }
            | ExecutableDeployItem::StoredContractByName { entry_point, .. } => &entry_point,
        }
    }

    pub fn args(&self) -> &Bytes {
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
}

impl ToBytes for ExecutableDeployItem {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
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
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
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
            _ => Err(Error::Formatting),
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
            ExecutableDeployItem::Transfer { args } => write!(f, "transfer-args {}", HexFmt(&args)),
        }
    }
}

impl Debug for ExecutableDeployItem {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ExecutableDeployItem::ModuleBytes { module_bytes, args } => f
                .debug_struct("ModuleBytes")
                .field("module_bytes", &format!("[{} bytes]", module_bytes.len()))
                .field("args", &HexFmt(&args))
                .finish(),
            ExecutableDeployItem::StoredContractByHash {
                hash,
                entry_point,
                args,
            } => f
                .debug_struct("StoredContractByHash")
                .field("hash", &HexFmt(hash))
                .field("entry_point", &entry_point)
                .field("args", &HexFmt(&args))
                .finish(),
            ExecutableDeployItem::StoredContractByName {
                name,
                entry_point,
                args,
            } => f
                .debug_struct("StoredContractByName")
                .field("name", &name)
                .field("entry_point", &entry_point)
                .field("args", &HexFmt(&args))
                .finish(),
            ExecutableDeployItem::StoredVersionedContractByHash {
                hash,
                version,
                entry_point,
                args,
            } => f
                .debug_struct("StoredVersionedContractByHash")
                .field("hash", &HexFmt(hash))
                .field("version", version)
                .field("entry_point", &entry_point)
                .field("args", &HexFmt(&args))
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
                .field("args", &HexFmt(&args))
                .finish(),
            ExecutableDeployItem::Transfer { args } => f
                .debug_struct("Transfer")
                .field("args", &HexFmt(&args))
                .finish(),
        }
    }
}

impl Distribution<ExecutableDeployItem> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> ExecutableDeployItem {
        fn random_bytes<R: Rng + ?Sized>(rng: &mut R) -> Vec<u8> {
            let mut bytes = vec![0u8; rng.gen_range(0, 100)];
            rng.fill_bytes(bytes.as_mut());
            bytes
        }

        fn random_string<R: Rng + ?Sized>(rng: &mut R) -> String {
            rng.sample_iter(&Alphanumeric).take(20).collect()
        }

        let args = random_bytes(rng);

        match rng.gen_range(0, 6) {
            0 => ExecutableDeployItem::ModuleBytes {
                module_bytes: random_bytes(rng).into(),
                args: args.into(),
            },
            1 => ExecutableDeployItem::StoredContractByHash {
                hash: ContractHash::new(rng.gen()),
                entry_point: random_string(rng),
                args: args.into(),
            },
            2 => ExecutableDeployItem::StoredContractByName {
                name: random_string(rng),
                entry_point: random_string(rng),
                args: args.into(),
            },
            3 => ExecutableDeployItem::StoredVersionedContractByHash {
                hash: ContractPackageHash::new(rng.gen()),
                version: rng.gen(),
                entry_point: random_string(rng),
                args: args.into(),
            },
            4 => ExecutableDeployItem::StoredVersionedContractByName {
                name: random_string(rng),
                version: rng.gen(),
                entry_point: random_string(rng),
                args: args.into(),
            },
            5 => ExecutableDeployItem::Transfer { args: args.into() },
            _ => unreachable!(),
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
