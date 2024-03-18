//! Code supporting an executable item.
use core::fmt::{self, Debug, Display, Formatter};
use std::convert::TryFrom;

#[cfg(feature = "datasize")]
use datasize::DataSize;

use hex_fmt::HexFmt;

use serde::{Deserialize, Serialize};

use casper_types::{
    account::AccountHash,
    addressable_entity::DEFAULT_ENTRY_POINT_NAME,
    bytesrepr::{self, Bytes, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    runtime_args,
    system::mint::ARG_AMOUNT,
    AddressableEntityHash, AddressableEntityIdentifier, EntityAddr, EntityVersion,
    ExecutableDeployItem, Gas, Motes, PackageHash, PackageIdentifier, Phase, PublicKey,
    RuntimeArgs, Transaction, TransactionInvocationTarget, TransactionRuntime, TransactionTarget,
    URef, U512,
};

#[cfg(test)]
use casper_types::testing::TestRng;
#[cfg(test)]
use rand::distributions::Alphanumeric;
#[cfg(test)]
use rand::distributions::Distribution;
#[cfg(test)]
use rand::distributions::Standard;
#[cfg(test)]
use rand::Rng;

const TAG_LENGTH: usize = U8_SERIALIZED_LENGTH;
const MODULE_BYTES_TAG: u8 = 0;
const BY_HASH_TAG: u8 = 1;
const BY_NAME_TAG: u8 = 2;
const BY_PACKAGE_HASH: u8 = 3;
const BY_PACKAGE_NAME: u8 = 4;
const BY_ADDR_TAG: u8 = 5;

/// Identifier for an [`ExecutableItem`].
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ExecutableItemIdentifier {
    /// The item is of the type [`ExecutableItem::Wasm`]
    Module,
    /// The item is a variation of a stored contract.
    AddressableEntity(AddressableEntityIdentifier),
    /// The item is a variation of a stored contract package.
    Package(PackageIdentifier),
    /// The item is a native transfer.
    Transfer,
}

/// The executable component of a [``].
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum ExecutableItem {
    /// Executable specified as raw bytes that represent Wasm code and an instance of
    /// [`RuntimeArgs`].
    Wasm {
        /// Raw Wasm module bytes with 'call' exported as an entrypoint.
        #[cfg_attr(
            feature = "json-schema",
            schemars(description = "Hex-encoded raw Wasm bytes.")
        )]
        module_bytes: Bytes,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
    /// Entity referenced by its [`AddressableEntityHash`], entry point and an instance of
    /// [`RuntimeArgs`].
    ByHash {
        /// entity hash.
        hash: AddressableEntityHash,
        /// Name of an entry point.
        entry_point: String,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
    /// Entity referenced by its [`EntityAddr`], entry point and an instance of
    /// [`RuntimeArgs`].
    ByAddress {
        /// Entity address.
        address: EntityAddr,
        /// Name of an entry point.
        entry_point: String,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
    /// Entity referenced by a named key existing in the signer's account context, entry
    /// point and an instance of [`RuntimeArgs`].
    ByName {
        /// Named key.
        name: String,
        /// Name of an entry point.
        entry_point: String,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
    /// Entity referenced by its [`PackageHash`], entry point and an
    /// instance of [`RuntimeArgs`].
    ByPackageHash {
        /// Contract package hash
        hash: PackageHash,
        /// An optional version of the contract to call. It will default to the highest enabled
        /// version if no value is specified.
        version: Option<EntityVersion>,
        /// Entry point name.
        entry_point: String,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
    /// Entity referenced by a named key existing in the signer's account
    /// context, entry point and an instance of [`RuntimeArgs`].
    ByPackageName {
        /// Named key.
        name: String,
        /// An optional version of the contract to call. It will default to the highest enabled
        /// version if no value is specified.
        version: Option<EntityVersion>,
        /// Entry point name.
        entry_point: String,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
}

impl ExecutableItem {
    /// Returns a new `ExecutableItem::Wasm`.
    pub fn new_wasm(module_bytes: Bytes, args: RuntimeArgs) -> Self {
        ExecutableItem::Wasm { module_bytes, args }
    }

    /// Returns a new `ExecutableItem::Wasm` suitable for use as standard payment code
    /// of a ``.
    pub fn new_standard_payment<A: Into<U512>>(amount: A) -> Self {
        ExecutableItem::Wasm {
            module_bytes: Bytes::new(),
            args: runtime_args! {
                ARG_AMOUNT => amount.into(),
            },
        }
    }

    /// Returns a new `ExecutableItem::ByHash`.
    pub fn new_by_hash(
        hash: AddressableEntityHash,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Self {
        ExecutableItem::ByHash {
            hash,
            entry_point,
            args,
        }
    }

    /// Returns a new `ExecutableItem::ByAddress`.
    pub fn new_by_addr(addr: EntityAddr, entry_point: String, args: RuntimeArgs) -> Self {
        ExecutableItem::ByAddress {
            address: addr,
            entry_point,
            args,
        }
    }

    /// Returns a new `ExecutableItem::ByName`.
    pub fn new_by_name(name: String, entry_point: String, args: RuntimeArgs) -> Self {
        ExecutableItem::ByName {
            name,
            entry_point,
            args,
        }
    }

    /// Returns a new `ExecutableItem::ByPackageHash`.
    pub fn new_by_package_hash(
        hash: PackageHash,
        version: Option<EntityVersion>,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Self {
        ExecutableItem::ByPackageHash {
            hash,
            version,
            entry_point,
            args,
        }
    }

    /// Returns a new `ExecutableItem::ByPackageName`.
    pub fn new_by_package(
        name: String,
        version: Option<EntityVersion>,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Self {
        ExecutableItem::ByPackageName {
            name,
            version,
            entry_point,
            args,
        }
    }

    /// Returns the entry point name.
    pub fn entry_point_name(&self) -> &str {
        match self {
            ExecutableItem::Wasm { .. } => DEFAULT_ENTRY_POINT_NAME,
            ExecutableItem::ByPackageName { entry_point, .. }
            | ExecutableItem::ByPackageHash { entry_point, .. }
            | ExecutableItem::ByAddress { entry_point, .. }
            | ExecutableItem::ByName { entry_point, .. }
            | ExecutableItem::ByHash { entry_point, .. } => entry_point,
        }
    }

    /// Returns the identifier of the `ExecutableItem`.
    pub fn identifier(&self) -> ExecutableItemIdentifier {
        match self {
            ExecutableItem::Wasm { .. } => ExecutableItemIdentifier::Module,
            ExecutableItem::ByHash { hash, .. } => ExecutableItemIdentifier::AddressableEntity(
                AddressableEntityIdentifier::Hash(*hash),
            ),
            ExecutableItem::ByAddress { address, .. } => {
                ExecutableItemIdentifier::AddressableEntity(AddressableEntityIdentifier::Addr(
                    *address,
                ))
            }
            ExecutableItem::ByName { name, .. } => ExecutableItemIdentifier::AddressableEntity(
                AddressableEntityIdentifier::Name(name.clone()),
            ),
            ExecutableItem::ByPackageHash { hash, version, .. } => {
                ExecutableItemIdentifier::Package(PackageIdentifier::Hash {
                    package_hash: *hash,
                    version: *version,
                })
            }
            ExecutableItem::ByPackageName { name, version, .. } => {
                ExecutableItemIdentifier::Package(PackageIdentifier::Name {
                    name: name.clone(),
                    version: *version,
                })
            }
        }
    }

    /// Returns the identifier of the contract in the item, if present.
    pub fn contract_identifier(&self) -> Option<AddressableEntityIdentifier> {
        match self {
            ExecutableItem::Wasm { .. }
            | ExecutableItem::ByPackageHash { .. }
            | ExecutableItem::ByPackageName { .. } => None,
            ExecutableItem::ByHash { hash, .. } => Some(AddressableEntityIdentifier::Hash(*hash)),
            ExecutableItem::ByAddress { address, .. } => {
                Some(AddressableEntityIdentifier::Addr(*address))
            }
            ExecutableItem::ByName { name, .. } => {
                Some(AddressableEntityIdentifier::Name(name.clone()))
            }
        }
    }

    /// Returns the identifier of the contract package in the item, if present.
    pub fn contract_package_identifier(&self) -> Option<PackageIdentifier> {
        match self {
            ExecutableItem::Wasm { .. }
            | ExecutableItem::ByAddress { .. }
            | ExecutableItem::ByName { .. }
            | ExecutableItem::ByHash { .. } => None,

            ExecutableItem::ByPackageHash { hash, version, .. } => Some(PackageIdentifier::Hash {
                package_hash: *hash,
                version: *version,
            }),
            ExecutableItem::ByPackageName { name, version, .. } => Some(PackageIdentifier::Name {
                name: name.clone(),
                version: *version,
            }),
        }
    }

    /// Returns the runtime arguments.
    pub fn args(&self) -> &RuntimeArgs {
        match self {
            ExecutableItem::Wasm { args, .. }
            | ExecutableItem::ByAddress { args, .. }
            | ExecutableItem::ByName { args, .. }
            | ExecutableItem::ByHash { args, .. }
            | ExecutableItem::ByPackageHash { args, .. }
            | ExecutableItem::ByPackageName { args, .. } => args,
        }
    }

    /// Returns the payment amount from args (if any) as Gas.
    pub fn payment_amount(&self, conv_rate: u64) -> Option<Gas> {
        let cl_value = self.args().get(ARG_AMOUNT)?;
        let motes = cl_value.clone().into_t::<U512>().ok()?;
        Gas::from_motes(Motes::new(motes), conv_rate)
    }

    /// Returns `true` if this item is a native transfer.
    pub fn is_transfer(&self) -> bool {
        false
    }

    /// Returns `true` if this item is a standard payment.
    pub fn is_standard_payment(&self, phase: Phase) -> bool {
        if phase != Phase::Payment {
            return false;
        }

        if let ExecutableItem::Wasm { module_bytes, .. } = self {
            return module_bytes.is_empty();
        }

        false
    }

    /// Returns `true` if the item is a contract identified by its name.
    pub fn is_by_name(&self) -> bool {
        matches!(self, ExecutableItem::ByPackageName { .. })
            || matches!(self, ExecutableItem::ByName { .. })
    }

    /// Returns the name of the contract or contract package, if the item is identified by
    /// name.
    pub fn by_name(&self) -> Option<String> {
        match self {
            ExecutableItem::ByName { name, .. } | ExecutableItem::ByPackageName { name, .. } => {
                Some(name.clone())
            }
            ExecutableItem::Wasm { .. }
            | ExecutableItem::ByAddress { .. }
            | ExecutableItem::ByPackageHash { .. }
            | ExecutableItem::ByHash { .. } => None,
        }
    }

    /// Returns `true` if the item is a stored contract.
    pub fn is_addressable_entity(&self) -> bool {
        matches!(self, ExecutableItem::ByAddress { .. })
            || matches!(self, ExecutableItem::ByName { .. })
            || matches!(self, ExecutableItem::ByAddress { .. })
    }

    /// Returns `true` if the item is a stored contract package.
    pub fn is_package(&self) -> bool {
        matches!(self, ExecutableItem::ByPackageHash { .. })
            || matches!(self, ExecutableItem::ByPackageName { .. })
    }

    /// Returns `true` if the item is [`ModuleBytes`].
    ///
    /// [`ModuleBytes`]: ExecutableItem::Wasm
    pub fn is_module_bytes(&self) -> bool {
        matches!(self, Self::Wasm { .. })
    }

    /// Returns a random `ExecutableItem`.
    #[cfg(test)]
    pub fn random(rng: &mut TestRng) -> Self {
        rng.gen()
    }
}

impl ToBytes for ExecutableItem {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        TAG_LENGTH
            + match self {
                ExecutableItem::Wasm { module_bytes, args } => {
                    module_bytes.serialized_length() + args.serialized_length()
                }
                ExecutableItem::ByHash {
                    hash,
                    entry_point,
                    args,
                } => {
                    hash.serialized_length()
                        + entry_point.serialized_length()
                        + args.serialized_length()
                }
                ExecutableItem::ByAddress {
                    address,
                    entry_point,
                    args,
                } => {
                    address.serialized_length()
                        + entry_point.serialized_length()
                        + args.serialized_length()
                }
                ExecutableItem::ByName {
                    name,
                    entry_point,
                    args,
                } => {
                    name.serialized_length()
                        + entry_point.serialized_length()
                        + args.serialized_length()
                }
                ExecutableItem::ByPackageHash {
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
                ExecutableItem::ByPackageName {
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

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            ExecutableItem::Wasm { module_bytes, args } => {
                writer.push(MODULE_BYTES_TAG);
                module_bytes.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            ExecutableItem::ByHash {
                hash,
                entry_point,
                args,
            } => {
                writer.push(BY_HASH_TAG);
                hash.write_bytes(writer)?;
                entry_point.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            ExecutableItem::ByAddress {
                address,
                entry_point,
                args,
            } => {
                writer.push(BY_ADDR_TAG);
                address.write_bytes(writer)?;
                entry_point.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            ExecutableItem::ByName {
                name,
                entry_point,
                args,
            } => {
                writer.push(BY_NAME_TAG);
                name.write_bytes(writer)?;
                entry_point.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            ExecutableItem::ByPackageHash {
                hash,
                version,
                entry_point,
                args,
            } => {
                writer.push(BY_PACKAGE_HASH);
                hash.write_bytes(writer)?;
                version.write_bytes(writer)?;
                entry_point.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            ExecutableItem::ByPackageName {
                name,
                version,
                entry_point,
                args,
            } => {
                writer.push(BY_PACKAGE_NAME);
                name.write_bytes(writer)?;
                version.write_bytes(writer)?;
                entry_point.write_bytes(writer)?;
                args.write_bytes(writer)
            }
        }
    }
}

impl FromBytes for ExecutableItem {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            MODULE_BYTES_TAG => {
                let (module_bytes, remainder) = Bytes::from_bytes(remainder)?;
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                Ok((ExecutableItem::Wasm { module_bytes, args }, remainder))
            }
            BY_ADDR_TAG => {
                let (address, remainder) = EntityAddr::from_bytes(remainder)?;
                let (entry_point, remainder) = String::from_bytes(remainder)?;
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                Ok((
                    ExecutableItem::ByAddress {
                        address,
                        entry_point,
                        args,
                    },
                    remainder,
                ))
            }
            BY_HASH_TAG => {
                let (hash, remainder) = AddressableEntityHash::from_bytes(remainder)?;
                let (entry_point, remainder) = String::from_bytes(remainder)?;
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                Ok((
                    ExecutableItem::ByHash {
                        hash,
                        entry_point,
                        args,
                    },
                    remainder,
                ))
            }
            BY_NAME_TAG => {
                let (name, remainder) = String::from_bytes(remainder)?;
                let (entry_point, remainder) = String::from_bytes(remainder)?;
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                Ok((
                    ExecutableItem::ByName {
                        name,
                        entry_point,
                        args,
                    },
                    remainder,
                ))
            }
            BY_PACKAGE_HASH => {
                let (hash, remainder) = PackageHash::from_bytes(remainder)?;
                let (version, remainder) = Option::<EntityVersion>::from_bytes(remainder)?;
                let (entry_point, remainder) = String::from_bytes(remainder)?;
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                Ok((
                    ExecutableItem::ByPackageHash {
                        hash,
                        version,
                        entry_point,
                        args,
                    },
                    remainder,
                ))
            }
            BY_PACKAGE_NAME => {
                let (name, remainder) = String::from_bytes(remainder)?;
                let (version, remainder) = Option::<EntityVersion>::from_bytes(remainder)?;
                let (entry_point, remainder) = String::from_bytes(remainder)?;
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                Ok((
                    ExecutableItem::ByPackageName {
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

impl Display for ExecutableItem {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ExecutableItem::Wasm { module_bytes, .. } => {
                write!(f, "module-bytes [{} bytes]", module_bytes.len())
            }
            ExecutableItem::ByHash {
                hash, entry_point, ..
            } => write!(
                f,
                "by-hash: {:10}, entry-point: {}",
                HexFmt(hash),
                entry_point,
            ),
            ExecutableItem::ByAddress {
                address,
                entry_point,
                ..
            } => write!(f, "by-address: {}, entry-point: {}", address, entry_point,),
            ExecutableItem::ByName {
                name, entry_point, ..
            } => write!(f, "by-name: {}, entry-point: {}", name, entry_point,),
            ExecutableItem::ByPackageHash {
                hash,
                version: Some(ver),
                entry_point,
                ..
            } => write!(
                f,
                "by-package-hash-and-version: {:10}, version: {}, entry-point: {}",
                HexFmt(hash),
                ver,
                entry_point,
            ),
            ExecutableItem::ByPackageHash {
                hash, entry_point, ..
            } => write!(
                f,
                "by-package-hash: {:10}, version: latest, entry-point: {}",
                HexFmt(hash),
                entry_point,
            ),
            ExecutableItem::ByPackageName {
                name,
                version: Some(ver),
                entry_point,
                ..
            } => write!(
                f,
                "by-package-name-and-version: {}, version: {}, entry-point: {}",
                name, ver, entry_point,
            ),
            ExecutableItem::ByPackageName {
                name, entry_point, ..
            } => write!(
                f,
                "by-package-name: {}, version: latest, entry-point: {}",
                name, entry_point,
            ),
        }
    }
}

impl Debug for ExecutableItem {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ExecutableItem::Wasm { module_bytes, args } => f
                .debug_struct("Wasm")
                .field("module_bytes", &format!("[{} bytes]", module_bytes.len()))
                .field("args", args)
                .finish(),
            ExecutableItem::ByHash {
                hash,
                entry_point,
                args,
            } => f
                .debug_struct("ByHash")
                .field("hash", &base16::encode_lower(hash))
                .field("entry_point", &entry_point)
                .field("args", args)
                .finish(),
            ExecutableItem::ByAddress {
                address,
                entry_point,
                args,
            } => f
                .debug_struct("ByAddress")
                .field("address", &address)
                .field("entry_point", &entry_point)
                .field("args", args)
                .finish(),
            ExecutableItem::ByName {
                name,
                entry_point,
                args,
            } => f
                .debug_struct("StoredContractByName")
                .field("name", &name)
                .field("entry_point", &entry_point)
                .field("args", args)
                .finish(),
            ExecutableItem::ByPackageHash {
                hash,
                version,
                entry_point,
                args,
            } => f
                .debug_struct("ByPackageHash")
                .field("hash", &base16::encode_lower(hash))
                .field("version", version)
                .field("entry_point", &entry_point)
                .field("args", args)
                .finish(),
            ExecutableItem::ByPackageName {
                name,
                version,
                entry_point,
                args,
            } => f
                .debug_struct("ByPackageName")
                .field("name", &name)
                .field("version", version)
                .field("entry_point", &entry_point)
                .field("args", args)
                .finish(),
        }
    }
}

impl TryFrom<ExecutableDeployItem> for ExecutableItem {
    type Error = ();

    fn try_from(deploy_item: ExecutableDeployItem) -> Result<Self, Self::Error> {
        let ret = match deploy_item {
            ExecutableDeployItem::ModuleBytes { module_bytes, args } => {
                ExecutableItem::Wasm { module_bytes, args }
            }
            ExecutableDeployItem::StoredContractByHash {
                hash,
                entry_point,
                args,
            } => ExecutableItem::ByHash {
                hash,
                entry_point,
                args,
            },
            ExecutableDeployItem::StoredContractByName {
                name,
                entry_point,
                args,
            } => ExecutableItem::ByName {
                name,
                entry_point,
                args,
            },
            ExecutableDeployItem::StoredVersionedContractByHash {
                hash,
                version,
                entry_point,
                args,
            } => ExecutableItem::ByPackageHash {
                hash,
                version,
                entry_point,
                args,
            },
            ExecutableDeployItem::StoredVersionedContractByName {
                name,
                version,
                entry_point,
                args,
            } => ExecutableItem::ByPackageName {
                name,
                version,
                entry_point,
                args,
            },
            _ => return Err(()),
        };

        Ok(ret)
    }
}

impl TryFrom<Transaction> for ExecutableItem {
    type Error = ();

    fn try_from(transaction: Transaction) -> Result<Self, Self::Error> {
        match transaction {
            Transaction::Deploy(deploy) => Ok(ExecutableItem::try_from(deploy.session().clone())?),
            Transaction::V1(v1) => match v1.body().target() {
                TransactionTarget::Native => Err(()),
                TransactionTarget::Stored { id, runtime } => {
                    if &TransactionRuntime::VmCasperV1 != runtime {
                        return Err(());
                    }
                    match id {
                        TransactionInvocationTarget::InvocableEntity(hash_addr) => {
                            Ok(ExecutableItem::ByHash {
                                hash: AddressableEntityHash::new(*hash_addr),
                                args: v1.body().args().clone(),
                                entry_point: v1.body().entry_point().to_string(),
                            })
                        }
                        TransactionInvocationTarget::InvocableEntityAlias(name) => {
                            Ok(ExecutableItem::ByName {
                                name: name.clone(),
                                args: v1.body().args().clone(),
                                entry_point: v1.body().entry_point().to_string(),
                            })
                        }
                        TransactionInvocationTarget::Package { addr, version } => {
                            Ok(ExecutableItem::ByPackageHash {
                                hash: PackageHash::new(*addr),
                                args: v1.body().args().clone(),
                                entry_point: v1.body().entry_point().to_string(),
                                version: *version,
                            })
                        }
                        TransactionInvocationTarget::PackageAlias { alias, version } => {
                            Ok(ExecutableItem::ByPackageName {
                                name: alias.clone(),
                                args: v1.body().args().clone(),
                                entry_point: v1.body().entry_point().to_string(),
                                version: *version,
                            })
                        }
                    }
                }
                TransactionTarget::Session {
                    module_bytes,
                    runtime,
                    ..
                } => {
                    if &TransactionRuntime::VmCasperV1 != runtime {
                        return Err(());
                    }
                    Ok(ExecutableItem::Wasm {
                        module_bytes: module_bytes.clone(),
                        args: v1.body().args().clone(),
                    })
                }
            },
        }
    }
}

#[cfg(test)]
impl Distribution<ExecutableItem> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> ExecutableItem {
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
            0 => ExecutableItem::Wasm {
                module_bytes: random_bytes(rng).into(),
                args,
            },
            1 => ExecutableItem::ByHash {
                hash: AddressableEntityHash::new(rng.gen()),
                entry_point: random_string(rng),
                args,
            },
            2 => ExecutableItem::ByName {
                name: random_string(rng),
                entry_point: random_string(rng),
                args,
            },
            3 => ExecutableItem::ByPackageHash {
                hash: PackageHash::new(rng.gen()),
                version: rng.gen(),
                entry_point: random_string(rng),
                args,
            },
            4 => ExecutableItem::ByPackageName {
                name: random_string(rng),
                version: rng.gen(),
                entry_point: random_string(rng),
                args,
            },
            5 => ExecutableItem::ByAddress {
                address: EntityAddr::new_of_kind(rng.gen(), rng.gen()),
                entry_point: random_string(rng),
                args,
            },
            _ => unreachable!(),
        }
    }
}

/// The various types which can be used as the `target` runtime argument of a native transfer.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq)]
pub enum TransferTarget {
    /// A public key.
    PublicKey(PublicKey),
    /// An account hash.
    AccountHash(AccountHash),
    /// A URef.
    URef(URef),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialization_roundtrip() {
        let rng = &mut TestRng::new();
        for _ in 0..10 {
            let executable_deploy_item = ExecutableItem::random(rng);
            bytesrepr::test_serialization_roundtrip(&executable_deploy_item);
        }
    }
}
