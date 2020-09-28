use std::fmt::{self, Debug, Display, Formatter};

use datasize::DataSize;
use hex_buffer_serde::{Hex, HexForm};
use hex_fmt::HexFmt;
use rand::{
    distributions::{Alphanumeric, Distribution, Standard},
    Rng,
};
use serde::{Deserialize, Serialize};

use casper_types::{
    bytesrepr,
    contracts::{ContractVersion, DEFAULT_ENTRY_POINT_NAME},
    ContractHash, ContractPackageHash, Key, RuntimeArgs, KEY_HASH_LENGTH,
};

use super::error;
use crate::{core::execution, shared::account::Account};

#[derive(Clone, DataSize, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ExecutableDeployItem {
    ModuleBytes {
        #[serde(with = "HexForm::<Vec<u8>>")]
        module_bytes: Vec<u8>,
        // assumes implicit `call` noarg entrypoint
        #[serde(with = "HexForm::<Vec<u8>>")]
        args: Vec<u8>,
    },
    StoredContractByHash {
        #[serde(with = "HexForm::<[u8; KEY_HASH_LENGTH]>")]
        hash: ContractHash,
        entry_point: String,
        #[serde(with = "HexForm::<Vec<u8>>")]
        args: Vec<u8>,
    },
    StoredContractByName {
        name: String,
        entry_point: String,
        #[serde(with = "HexForm::<Vec<u8>>")]
        args: Vec<u8>,
    },
    StoredVersionedContractByName {
        name: String,
        version: Option<ContractVersion>, // defaults to highest enabled version
        entry_point: String,
        #[serde(with = "HexForm::<Vec<u8>>")]
        args: Vec<u8>,
    },
    StoredVersionedContractByHash {
        #[serde(with = "HexForm::<[u8; KEY_HASH_LENGTH]>")]
        hash: ContractPackageHash,
        version: Option<ContractVersion>, // defaults to highest enabled version
        entry_point: String,
        #[serde(with = "HexForm::<Vec<u8>>")]
        args: Vec<u8>,
    },
    Transfer {
        #[serde(with = "HexForm::<Vec<u8>>")]
        args: Vec<u8>,
    },
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
                name,
                version: None,
                entry_point,
                ..
            } => write!(
                f,
                "stored-versioned-contract: {}, version: latest, entry-point: {}",
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
                hash,
                version: None,
                entry_point,
                ..
            } => write!(
                f,
                "stored-versioned-contract-by-hash: {:10}, version: latest, entry-point: {}",
                HexFmt(hash),
                entry_point,
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
            ExecutableDeployItem::Transfer { args } => f
                .debug_struct("Transfer")
                .field("args", &HexFmt(&args))
                .finish(),
        }
    }
}

impl ExecutableDeployItem {
    pub(crate) fn to_contract_hash_key(
        &self,
        account: &Account,
    ) -> Result<Option<Key>, error::Error> {
        match self {
            ExecutableDeployItem::StoredContractByHash { hash, .. }
            | ExecutableDeployItem::StoredVersionedContractByHash { hash, .. } => {
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

    pub fn into_runtime_args(self) -> Result<RuntimeArgs, bytesrepr::Error> {
        match self {
            ExecutableDeployItem::ModuleBytes { args, .. }
            | ExecutableDeployItem::StoredContractByHash { args, .. }
            | ExecutableDeployItem::StoredContractByName { args, .. }
            | ExecutableDeployItem::StoredVersionedContractByHash { args, .. }
            | ExecutableDeployItem::StoredVersionedContractByName { args, .. }
            | ExecutableDeployItem::Transfer { args } => {
                let runtime_args: RuntimeArgs = bytesrepr::deserialize(args)?;
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
                module_bytes: random_bytes(rng),
                args,
            },
            1 => ExecutableDeployItem::StoredContractByHash {
                hash: rng.gen(),
                entry_point: random_string(rng),
                args,
            },
            2 => ExecutableDeployItem::StoredContractByName {
                name: random_string(rng),
                entry_point: random_string(rng),
                args,
            },
            3 => ExecutableDeployItem::StoredVersionedContractByName {
                name: random_string(rng),
                version: rng.gen(),
                entry_point: random_string(rng),
                args,
            },
            4 => ExecutableDeployItem::StoredVersionedContractByHash {
                hash: rng.gen(),
                version: rng.gen(),
                entry_point: random_string(rng),
                args,
            },
            5 => ExecutableDeployItem::Transfer { args },
            _ => unreachable!(),
        }
    }
}
