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
use super::TransactionV1;
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    system::mint::ARG_AMOUNT,
    AddressableEntityHash, EntityIdentifier, EntityVersion, Gas, Motes, PackageHash,
    PackageIdentifier, RuntimeArgs, U512,
};

const STORED_CONTRACT_BY_HASH_TAG: u8 = 0;
const STORED_CONTRACT_BY_NAME_TAG: u8 = 1;
const STORED_VERSIONED_CONTRACT_BY_HASH_TAG: u8 = 2;
const STORED_VERSIONED_CONTRACT_BY_NAME_TAG: u8 = 3;

/// A [`TransactionV1`] targeting a stored contract.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "A TransactionV1 targeting a stored contract.")
)]
#[serde(deny_unknown_fields)]
pub enum DirectCallV1 {
    /// Stored contract referenced by its hash.
    StoredContractByHash {
        /// Contract hash.
        hash: AddressableEntityHash,
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
        hash: PackageHash,
        /// Version of the contract to call; defaults to highest enabled version if unspecified.
        version: Option<EntityVersion>,
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
        version: Option<EntityVersion>,
        /// Name of the entry point.
        entry_point: String,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
}

impl DirectCallV1 {
    /// Returns a new `DirectCallV1::StoredContractByHash`.
    pub fn new_stored_contract_by_hash(
        hash: AddressableEntityHash,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Self {
        DirectCallV1::StoredContractByHash {
            hash,
            entry_point,
            args,
        }
    }

    /// Returns a new `DirectCallV1::StoredContractByName`.
    pub fn new_stored_contract_by_name(
        name: String,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Self {
        DirectCallV1::StoredContractByName {
            name,
            entry_point,
            args,
        }
    }

    /// Returns a new `DirectCallV1::StoredVersionedContractByHash`.
    pub fn new_stored_versioned_contract_by_hash(
        hash: PackageHash,
        version: Option<EntityVersion>,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Self {
        DirectCallV1::StoredVersionedContractByHash {
            hash,
            version,
            entry_point,
            args,
        }
    }

    /// Returns a new `DirectCallV1::StoredVersionedContractByName`.
    pub fn new_stored_versioned_contract_by_name(
        name: String,
        version: Option<EntityVersion>,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Self {
        DirectCallV1::StoredVersionedContractByName {
            name,
            version,
            entry_point,
            args,
        }
    }

    /// Returns the entry point name.
    pub fn entry_point_name(&self) -> &str {
        match self {
            DirectCallV1::StoredVersionedContractByName { entry_point, .. }
            | DirectCallV1::StoredVersionedContractByHash { entry_point, .. }
            | DirectCallV1::StoredContractByHash { entry_point, .. }
            | DirectCallV1::StoredContractByName { entry_point, .. } => entry_point,
        }
    }

    /// Returns the identifier of the contract, if present.
    pub fn contract_identifier(&self) -> Option<EntityIdentifier> {
        match self {
            DirectCallV1::StoredVersionedContractByHash { .. }
            | DirectCallV1::StoredVersionedContractByName { .. } => None,
            DirectCallV1::StoredContractByHash { hash, .. } => Some(EntityIdentifier::Hash(*hash)),
            DirectCallV1::StoredContractByName { name, .. } => {
                Some(EntityIdentifier::Name(name.clone()))
            }
        }
    }

    /// Returns the identifier of the contract package, if present.
    pub fn contract_package_identifier(&self) -> Option<PackageIdentifier> {
        match self {
            DirectCallV1::StoredContractByHash { .. }
            | DirectCallV1::StoredContractByName { .. } => None,
            DirectCallV1::StoredVersionedContractByHash { hash, version, .. } => {
                Some(PackageIdentifier::Hash {
                    package_hash: *hash,
                    version: *version,
                })
            }
            DirectCallV1::StoredVersionedContractByName { name, version, .. } => {
                Some(PackageIdentifier::Name {
                    name: name.clone(),
                    version: *version,
                })
            }
        }
    }

    /// Returns the runtime arguments.
    pub fn args(&self) -> &RuntimeArgs {
        match self {
            DirectCallV1::StoredContractByHash { args, .. }
            | DirectCallV1::StoredContractByName { args, .. }
            | DirectCallV1::StoredVersionedContractByHash { args, .. }
            | DirectCallV1::StoredVersionedContractByName { args, .. } => args,
        }
    }

    /// Returns the payment amount from args (if any) as Gas.
    pub fn payment_amount(&self, conv_rate: u64) -> Option<Gas> {
        let cl_value = self.args().get(ARG_AMOUNT)?;
        let motes = cl_value.clone().into_t::<U512>().ok()?;
        Gas::from_motes(Motes::new(motes), conv_rate)
    }

    pub(super) fn args_mut(&mut self) -> &mut RuntimeArgs {
        match self {
            DirectCallV1::StoredContractByHash { args, .. }
            | DirectCallV1::StoredContractByName { args, .. }
            | DirectCallV1::StoredVersionedContractByHash { args, .. }
            | DirectCallV1::StoredVersionedContractByName { args, .. } => args,
        }
    }

    /// Returns a random `DirectCallV1`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        let entry_point = rng.random_string(1..21);
        let args = RuntimeArgs::random(rng);
        match rng.gen_range(0..4) {
            0 => DirectCallV1::StoredContractByHash {
                hash: AddressableEntityHash::new(rng.gen()),
                entry_point,
                args,
            },
            1 => DirectCallV1::StoredContractByName {
                name: rng.random_string(1..21),
                entry_point,
                args,
            },
            2 => DirectCallV1::StoredVersionedContractByHash {
                hash: PackageHash::new(rng.gen()),
                version: rng.gen(),
                entry_point,
                args,
            },
            3 => DirectCallV1::StoredVersionedContractByName {
                name: rng.random_string(1..21),
                version: rng.gen(),
                entry_point,
                args,
            },
            _ => unreachable!(),
        }
    }
}

impl Display for DirectCallV1 {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            DirectCallV1::StoredContractByHash {
                hash, entry_point, ..
            } => write!(
                formatter,
                "stored-contract-by-hash: {:10}, entry-point: {}",
                HexFmt(hash),
                entry_point,
            ),
            DirectCallV1::StoredContractByName {
                name, entry_point, ..
            } => write!(
                formatter,
                "stored-contract-by-name: {}, entry-point: {}",
                name, entry_point,
            ),
            DirectCallV1::StoredVersionedContractByHash {
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
            DirectCallV1::StoredVersionedContractByHash {
                hash, entry_point, ..
            } => write!(
                formatter,
                "stored-versioned-contract-by-hash: {:10}, version: latest, entry-point: {}",
                HexFmt(hash),
                entry_point,
            ),
            DirectCallV1::StoredVersionedContractByName {
                name,
                version: Some(ver),
                entry_point,
                ..
            } => write!(
                formatter,
                "stored-versioned-contract: {}, version: {}, entry-point: {}",
                name, ver, entry_point,
            ),
            DirectCallV1::StoredVersionedContractByName {
                name, entry_point, ..
            } => write!(
                formatter,
                "stored-versioned-contract: {}, version: latest, entry-point: {}",
                name, entry_point,
            ),
        }
    }
}

impl Debug for DirectCallV1 {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            DirectCallV1::StoredContractByHash {
                hash,
                entry_point,
                args,
            } => f
                .debug_struct("StoredContractByHash")
                .field("hash", &base16::encode_lower(hash))
                .field("entry_point", &entry_point)
                .field("args", args)
                .finish(),
            DirectCallV1::StoredContractByName {
                name,
                entry_point,
                args,
            } => f
                .debug_struct("StoredContractByName")
                .field("name", &name)
                .field("entry_point", &entry_point)
                .field("args", args)
                .finish(),
            DirectCallV1::StoredVersionedContractByHash {
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
            DirectCallV1::StoredVersionedContractByName {
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

impl ToBytes for DirectCallV1 {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            DirectCallV1::StoredContractByHash {
                hash,
                entry_point,
                args,
            } => {
                writer.push(STORED_CONTRACT_BY_HASH_TAG);
                hash.write_bytes(writer)?;
                entry_point.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            DirectCallV1::StoredContractByName {
                name,
                entry_point,
                args,
            } => {
                writer.push(STORED_CONTRACT_BY_NAME_TAG);
                name.write_bytes(writer)?;
                entry_point.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            DirectCallV1::StoredVersionedContractByHash {
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
            DirectCallV1::StoredVersionedContractByName {
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
                DirectCallV1::StoredContractByHash {
                    hash,
                    entry_point,
                    args,
                } => {
                    hash.serialized_length()
                        + entry_point.serialized_length()
                        + args.serialized_length()
                }
                DirectCallV1::StoredContractByName {
                    name,
                    entry_point,
                    args,
                } => {
                    name.serialized_length()
                        + entry_point.serialized_length()
                        + args.serialized_length()
                }
                DirectCallV1::StoredVersionedContractByHash {
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
                DirectCallV1::StoredVersionedContractByName {
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

impl FromBytes for DirectCallV1 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            STORED_CONTRACT_BY_HASH_TAG => {
                let (hash, remainder) = AddressableEntityHash::from_bytes(remainder)?;
                let (entry_point, remainder) = String::from_bytes(remainder)?;
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                Ok((
                    DirectCallV1::StoredContractByHash {
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
                    DirectCallV1::StoredContractByName {
                        name,
                        entry_point,
                        args,
                    },
                    remainder,
                ))
            }
            STORED_VERSIONED_CONTRACT_BY_HASH_TAG => {
                let (hash, remainder) = PackageHash::from_bytes(remainder)?;
                let (version, remainder) = Option::<EntityVersion>::from_bytes(remainder)?;
                let (entry_point, remainder) = String::from_bytes(remainder)?;
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                Ok((
                    DirectCallV1::StoredVersionedContractByHash {
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
                let (version, remainder) = Option::<EntityVersion>::from_bytes(remainder)?;
                let (entry_point, remainder) = String::from_bytes(remainder)?;
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                Ok((
                    DirectCallV1::StoredVersionedContractByName {
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
            bytesrepr::test_serialization_roundtrip(&DirectCallV1::random(rng));
        }
    }
}
