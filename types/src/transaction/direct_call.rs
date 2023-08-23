use alloc::{string::String, vec::Vec};
use core::fmt::{self, Debug, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use hex_fmt::HexFmt;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::Transaction;
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    system::mint::ARG_AMOUNT,
    ContractHash, ContractIdentifier, ContractPackageHash, ContractPackageIdentifier,
    ContractVersion, Gas, Motes, RuntimeArgs, U512,
};

const STORED_CONTRACT_BY_HASH_TAG: u8 = 0;
const STORED_CONTRACT_BY_NAME_TAG: u8 = 1;
const STORED_VERSIONED_CONTRACT_BY_HASH_TAG: u8 = 2;
const STORED_VERSIONED_CONTRACT_BY_NAME_TAG: u8 = 3;

/// A [`Transaction`] targeting a stored contract.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "A Transaction targeting a stored contract.")
)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub enum DirectCall {
    /// Stored contract referenced by its hash.
    StoredContractByHash {
        /// Contract hash.
        hash: ContractHash,
        /// Name of the entry point.
        entry_point: String,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
    /// Stored contract referenced by the name of a named key in the signer's account context.
    StoredContractByName {
        /// Name of the named key.
        name: String,
        /// Name of the entry point.
        entry_point: String,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
    /// Stored versioned contract referenced by its hash.
    StoredVersionedContractByHash {
        /// Contract package hash.
        hash: ContractPackageHash,
        /// Version of the contract to call; defaults to highest enabled version if unspecified.
        version: Option<ContractVersion>,
        /// Name of the entry point.
        entry_point: String,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
    /// Stored versioned contract referenced by the name of a named key in the signer's account
    /// context.
    StoredVersionedContractByName {
        /// Name of the named key.
        name: String,
        /// Version of the contract to call; defaults to highest enabled version if unspecified.
        version: Option<ContractVersion>,
        /// Name of the entry point.
        entry_point: String,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
}

impl DirectCall {
    /// Returns a new `DirectCall::StoredContractByHash`.
    pub fn new_stored_contract_by_hash(
        hash: ContractHash,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Self {
        DirectCall::StoredContractByHash {
            hash,
            entry_point,
            args,
        }
    }

    /// Returns a new `DirectCall::StoredContractByName`.
    pub fn new_stored_contract_by_name(
        name: String,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Self {
        DirectCall::StoredContractByName {
            name,
            entry_point,
            args,
        }
    }

    /// Returns a new `DirectCall::StoredVersionedContractByHash`.
    pub fn new_stored_versioned_contract_by_hash(
        hash: ContractPackageHash,
        version: Option<ContractVersion>,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Self {
        DirectCall::StoredVersionedContractByHash {
            hash,
            version,
            entry_point,
            args,
        }
    }

    /// Returns a new `DirectCall::StoredVersionedContractByName`.
    pub fn new_stored_versioned_contract_by_name(
        name: String,
        version: Option<ContractVersion>,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Self {
        DirectCall::StoredVersionedContractByName {
            name,
            version,
            entry_point,
            args,
        }
    }

    /// Returns the entry point name.
    pub fn entry_point_name(&self) -> &str {
        match self {
            DirectCall::StoredVersionedContractByName { entry_point, .. }
            | DirectCall::StoredVersionedContractByHash { entry_point, .. }
            | DirectCall::StoredContractByHash { entry_point, .. }
            | DirectCall::StoredContractByName { entry_point, .. } => entry_point,
        }
    }

    /// Returns the identifier of the contract, if present.
    pub fn contract_identifier(&self) -> Option<ContractIdentifier> {
        match self {
            DirectCall::StoredVersionedContractByHash { .. }
            | DirectCall::StoredVersionedContractByName { .. } => None,
            DirectCall::StoredContractByHash { hash, .. } => Some(ContractIdentifier::Hash(*hash)),
            DirectCall::StoredContractByName { name, .. } => {
                Some(ContractIdentifier::Name(name.clone()))
            }
        }
    }

    /// Returns the identifier of the contract package, if present.
    pub fn contract_package_identifier(&self) -> Option<ContractPackageIdentifier> {
        match self {
            DirectCall::StoredContractByHash { .. } | DirectCall::StoredContractByName { .. } => {
                None
            }
            DirectCall::StoredVersionedContractByHash { hash, version, .. } => {
                Some(ContractPackageIdentifier::Hash {
                    contract_package_hash: *hash,
                    version: *version,
                })
            }
            DirectCall::StoredVersionedContractByName { name, version, .. } => {
                Some(ContractPackageIdentifier::Name {
                    name: name.clone(),
                    version: *version,
                })
            }
        }
    }

    /// Returns the runtime arguments.
    pub fn args(&self) -> &RuntimeArgs {
        match self {
            DirectCall::StoredContractByHash { args, .. }
            | DirectCall::StoredContractByName { args, .. }
            | DirectCall::StoredVersionedContractByHash { args, .. }
            | DirectCall::StoredVersionedContractByName { args, .. } => args,
        }
    }

    /// Returns the payment amount from args (if any) as Gas.
    pub fn payment_amount(&self, conv_rate: u64) -> Option<Gas> {
        let cl_value = self.args().get(ARG_AMOUNT)?;
        let motes = cl_value.clone().into_t::<U512>().ok()?;
        Gas::from_motes(Motes::new(motes), conv_rate)
    }

    /// Returns a random `DirectCall`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let entry_point = rng.random_string(1..21);
        let args = RuntimeArgs::random(rng);
        match rng.gen_range(0..4) {
            0 => DirectCall::StoredContractByHash {
                hash: ContractHash::new(rng.gen()),
                entry_point,
                args,
            },
            1 => DirectCall::StoredContractByName {
                name: rng.random_string(1..21),
                entry_point,
                args,
            },
            2 => DirectCall::StoredVersionedContractByHash {
                hash: ContractPackageHash::new(rng.gen()),
                version: rng.gen(),
                entry_point,
                args,
            },
            3 => DirectCall::StoredVersionedContractByName {
                name: rng.random_string(1..21),
                version: rng.gen(),
                entry_point,
                args,
            },
            _ => unreachable!(),
        }
    }
}

impl Display for DirectCall {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            DirectCall::StoredContractByHash {
                hash, entry_point, ..
            } => write!(
                formatter,
                "stored-contract-by-hash: {:10}, entry-point: {}",
                HexFmt(hash),
                entry_point,
            ),
            DirectCall::StoredContractByName {
                name, entry_point, ..
            } => write!(
                formatter,
                "stored-contract-by-name: {}, entry-point: {}",
                name, entry_point,
            ),
            DirectCall::StoredVersionedContractByHash {
                hash,
                version: Some(ver),
                entry_point,
                ..
            } => write!(
                formatter,
                "stored-versioned-contract-by-hash: {:10}, version: {}, entry-point: {}",
                HexFmt(hash),
                ver,
                entry_point,
            ),
            DirectCall::StoredVersionedContractByHash {
                hash, entry_point, ..
            } => write!(
                formatter,
                "stored-versioned-contract-by-hash: {:10}, version: latest, entry-point: {}",
                HexFmt(hash),
                entry_point,
            ),
            DirectCall::StoredVersionedContractByName {
                name,
                version: Some(ver),
                entry_point,
                ..
            } => write!(
                formatter,
                "stored-versioned-contract: {}, version: {}, entry-point: {}",
                name, ver, entry_point,
            ),
            DirectCall::StoredVersionedContractByName {
                name, entry_point, ..
            } => write!(
                formatter,
                "stored-versioned-contract: {}, version: latest, entry-point: {}",
                name, entry_point,
            ),
        }
    }
}

impl Debug for DirectCall {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            DirectCall::StoredContractByHash {
                hash,
                entry_point,
                args,
            } => f
                .debug_struct("StoredContractByHash")
                .field("hash", &base16::encode_lower(hash))
                .field("entry_point", &entry_point)
                .field("args", args)
                .finish(),
            DirectCall::StoredContractByName {
                name,
                entry_point,
                args,
            } => f
                .debug_struct("StoredContractByName")
                .field("name", &name)
                .field("entry_point", &entry_point)
                .field("args", args)
                .finish(),
            DirectCall::StoredVersionedContractByHash {
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
            DirectCall::StoredVersionedContractByName {
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
        }
    }
}

impl ToBytes for DirectCall {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            DirectCall::StoredContractByHash {
                hash,
                entry_point,
                args,
            } => {
                writer.push(STORED_CONTRACT_BY_HASH_TAG);
                hash.write_bytes(writer)?;
                entry_point.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            DirectCall::StoredContractByName {
                name,
                entry_point,
                args,
            } => {
                writer.push(STORED_CONTRACT_BY_NAME_TAG);
                name.write_bytes(writer)?;
                entry_point.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            DirectCall::StoredVersionedContractByHash {
                hash,
                version,
                entry_point,
                args,
            } => {
                writer.push(STORED_VERSIONED_CONTRACT_BY_HASH_TAG);
                hash.write_bytes(writer)?;
                version.write_bytes(writer)?;
                entry_point.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            DirectCall::StoredVersionedContractByName {
                name,
                version,
                entry_point,
                args,
            } => {
                writer.push(STORED_VERSIONED_CONTRACT_BY_NAME_TAG);
                name.write_bytes(writer)?;
                version.write_bytes(writer)?;
                entry_point.write_bytes(writer)?;
                args.write_bytes(writer)
            }
        }
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                DirectCall::StoredContractByHash {
                    hash,
                    entry_point,
                    args,
                } => {
                    hash.serialized_length()
                        + entry_point.serialized_length()
                        + args.serialized_length()
                }
                DirectCall::StoredContractByName {
                    name,
                    entry_point,
                    args,
                } => {
                    name.serialized_length()
                        + entry_point.serialized_length()
                        + args.serialized_length()
                }
                DirectCall::StoredVersionedContractByHash {
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
                DirectCall::StoredVersionedContractByName {
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
            }
    }
}

impl FromBytes for DirectCall {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            STORED_CONTRACT_BY_HASH_TAG => {
                let (hash, remainder) = ContractHash::from_bytes(remainder)?;
                let (entry_point, remainder) = String::from_bytes(remainder)?;
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                Ok((
                    DirectCall::StoredContractByHash {
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
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                Ok((
                    DirectCall::StoredContractByName {
                        name,
                        entry_point,
                        args,
                    },
                    remainder,
                ))
            }
            STORED_VERSIONED_CONTRACT_BY_HASH_TAG => {
                let (hash, remainder) = ContractPackageHash::from_bytes(remainder)?;
                let (version, remainder) = Option::<ContractVersion>::from_bytes(remainder)?;
                let (entry_point, remainder) = String::from_bytes(remainder)?;
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                Ok((
                    DirectCall::StoredVersionedContractByHash {
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
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                Ok((
                    DirectCall::StoredVersionedContractByName {
                        name,
                        version,
                        entry_point,
                        args,
                    },
                    remainder,
                ))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        for _ in 0..10 {
            bytesrepr::test_serialization_roundtrip(&DirectCall::random(rng));
        }
    }
}
