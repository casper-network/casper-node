use alloc::{string::String, vec::Vec};
use core::fmt::{self, Debug, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use hex_fmt::HexFmt;
#[cfg(any(feature = "testing", test))]
use rand::{
    distributions::{Alphanumeric, Distribution, Standard},
    Rng,
};
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::Deploy;
use crate::{
    account::AccountHash,
    addressable_entity::DEFAULT_ENTRY_POINT_NAME,
    bytesrepr::{self, Bytes, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    package::{EntityVersion, PackageHash},
    runtime_args, serde_helpers,
    system::mint::ARG_AMOUNT,
    AddressableEntityHash, AddressableEntityIdentifier, Gas, Motes, PackageIdentifier, Phase,
    PublicKey, RuntimeArgs, URef, U512,
};
#[cfg(any(feature = "testing", test))]
use crate::{testing::TestRng, CLValue};

const TAG_LENGTH: usize = U8_SERIALIZED_LENGTH;
const MODULE_BYTES_TAG: u8 = 0;
const STORED_CONTRACT_BY_HASH_TAG: u8 = 1;
const STORED_CONTRACT_BY_NAME_TAG: u8 = 2;
const STORED_VERSIONED_CONTRACT_BY_HASH_TAG: u8 = 3;
const STORED_VERSIONED_CONTRACT_BY_NAME_TAG: u8 = 4;
const TRANSFER_TAG: u8 = 5;
const TRANSFER_ARG_AMOUNT: &str = "amount";
const TRANSFER_ARG_SOURCE: &str = "source";
const TRANSFER_ARG_TARGET: &str = "target";
const TRANSFER_ARG_ID: &str = "id";

/// Identifier for an [`ExecutableDeployItem`].
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ExecutableDeployItemIdentifier {
    /// The deploy item is of the type [`ExecutableDeployItem::ModuleBytes`]
    Module,
    /// The deploy item is a variation of a stored contract.
    AddressableEntity(AddressableEntityIdentifier),
    /// The deploy item is a variation of a stored contract package.
    Package(PackageIdentifier),
    /// The deploy item is a native transfer.
    Transfer,
}

/// The executable component of a [`Deploy`].
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum ExecutableDeployItem {
    /// Executable specified as raw bytes that represent Wasm code and an instance of
    /// [`RuntimeArgs`].
    ModuleBytes {
        /// Raw Wasm module bytes with 'call' exported as an entrypoint.
        #[cfg_attr(
            feature = "json-schema",
            schemars(description = "Hex-encoded raw Wasm bytes.")
        )]
        module_bytes: Bytes,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
    /// Stored contract referenced by its [`AddressableEntityHash`], entry point and an instance of
    /// [`RuntimeArgs`].
    StoredContractByHash {
        /// Contract hash.
        #[serde(with = "serde_helpers::contract_hash_as_digest")]
        #[cfg_attr(
            feature = "json-schema",
            schemars(
                // this attribute is necessary due to a bug: https://github.com/GREsau/schemars/issues/89
                with = "AddressableEntityHash",
                description = "Hex-encoded contract hash."
            )
        )]
        hash: AddressableEntityHash,
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
    /// Stored versioned contract referenced by its [`PackageHash`], entry point and an
    /// instance of [`RuntimeArgs`].
    StoredVersionedContractByHash {
        /// Contract package hash
        #[serde(with = "serde_helpers::contract_package_hash_as_digest")]
        #[cfg_attr(
            feature = "json-schema",
            schemars(
                // this attribute is necessary due to a bug: https://github.com/GREsau/schemars/issues/89
                with = "PackageHash",
                description = "Hex-encoded contract package hash."
            )
        )]
        hash: PackageHash,
        /// An optional version of the contract to call. It will default to the highest enabled
        /// version if no value is specified.
        version: Option<EntityVersion>,
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
        version: Option<EntityVersion>,
        /// Entry point name.
        entry_point: String,
        /// Runtime arguments.
        args: RuntimeArgs,
    },
    /// A native transfer which does not contain or reference a Wasm code.
    Transfer {
        /// Runtime arguments.
        args: RuntimeArgs,
    },
}

impl ExecutableDeployItem {
    /// Returns a new `ExecutableDeployItem::ModuleBytes`.
    pub fn new_module_bytes(module_bytes: Bytes, args: RuntimeArgs) -> Self {
        ExecutableDeployItem::ModuleBytes { module_bytes, args }
    }

    /// Returns a new `ExecutableDeployItem::ModuleBytes` suitable for use as standard payment code
    /// of a `Deploy`.
    pub fn new_standard_payment<A: Into<U512>>(amount: A) -> Self {
        ExecutableDeployItem::ModuleBytes {
            module_bytes: Bytes::new(),
            args: runtime_args! {
                ARG_AMOUNT => amount.into(),
            },
        }
    }

    /// Returns a new `ExecutableDeployItem::StoredContractByHash`.
    pub fn new_stored_contract_by_hash(
        hash: AddressableEntityHash,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Self {
        ExecutableDeployItem::StoredContractByHash {
            hash,
            entry_point,
            args,
        }
    }

    /// Returns a new `ExecutableDeployItem::StoredContractByName`.
    pub fn new_stored_contract_by_name(
        name: String,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Self {
        ExecutableDeployItem::StoredContractByName {
            name,
            entry_point,
            args,
        }
    }

    /// Returns a new `ExecutableDeployItem::StoredVersionedContractByHash`.
    pub fn new_stored_versioned_contract_by_hash(
        hash: PackageHash,
        version: Option<EntityVersion>,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Self {
        ExecutableDeployItem::StoredVersionedContractByHash {
            hash,
            version,
            entry_point,
            args,
        }
    }

    /// Returns a new `ExecutableDeployItem::StoredVersionedContractByName`.
    pub fn new_stored_versioned_contract_by_name(
        name: String,
        version: Option<EntityVersion>,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Self {
        ExecutableDeployItem::StoredVersionedContractByName {
            name,
            version,
            entry_point,
            args,
        }
    }

    /// Returns a new `ExecutableDeployItem` suitable for use as session code for a transfer.
    ///
    /// If `maybe_source` is None, the account's main purse is used as the source.
    pub fn new_transfer<A: Into<U512>>(
        amount: A,
        maybe_source: Option<URef>,
        target: TransferTarget,
        maybe_transfer_id: Option<u64>,
    ) -> Self {
        let mut args = RuntimeArgs::new();
        args.insert(TRANSFER_ARG_AMOUNT, amount.into())
            .expect("should serialize amount arg");

        if let Some(source) = maybe_source {
            args.insert(TRANSFER_ARG_SOURCE, source)
                .expect("should serialize source arg");
        }

        match target {
            TransferTarget::PublicKey(public_key) => args
                .insert(TRANSFER_ARG_TARGET, public_key)
                .expect("should serialize public key target arg"),
            TransferTarget::AccountHash(account_hash) => args
                .insert(TRANSFER_ARG_TARGET, account_hash)
                .expect("should serialize account hash target arg"),
            TransferTarget::URef(uref) => args
                .insert(TRANSFER_ARG_TARGET, uref)
                .expect("should serialize uref target arg"),
        }

        args.insert(TRANSFER_ARG_ID, maybe_transfer_id)
            .expect("should serialize transfer id arg");

        ExecutableDeployItem::Transfer { args }
    }

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

    /// Returns the identifier of the `ExecutableDeployItem`.
    pub fn identifier(&self) -> ExecutableDeployItemIdentifier {
        match self {
            ExecutableDeployItem::ModuleBytes { .. } => ExecutableDeployItemIdentifier::Module,
            ExecutableDeployItem::StoredContractByHash { hash, .. } => {
                ExecutableDeployItemIdentifier::AddressableEntity(
                    AddressableEntityIdentifier::Hash(*hash),
                )
            }
            ExecutableDeployItem::StoredContractByName { name, .. } => {
                ExecutableDeployItemIdentifier::AddressableEntity(
                    AddressableEntityIdentifier::Name(name.clone()),
                )
            }
            ExecutableDeployItem::StoredVersionedContractByHash { hash, version, .. } => {
                ExecutableDeployItemIdentifier::Package(PackageIdentifier::Hash {
                    package_hash: *hash,
                    version: *version,
                })
            }
            ExecutableDeployItem::StoredVersionedContractByName { name, version, .. } => {
                ExecutableDeployItemIdentifier::Package(PackageIdentifier::Name {
                    name: name.clone(),
                    version: *version,
                })
            }
            ExecutableDeployItem::Transfer { .. } => ExecutableDeployItemIdentifier::Transfer,
        }
    }

    /// Returns the identifier of the contract in the deploy item, if present.
    pub fn contract_identifier(&self) -> Option<AddressableEntityIdentifier> {
        match self {
            ExecutableDeployItem::ModuleBytes { .. }
            | ExecutableDeployItem::StoredVersionedContractByHash { .. }
            | ExecutableDeployItem::StoredVersionedContractByName { .. }
            | ExecutableDeployItem::Transfer { .. } => None,
            ExecutableDeployItem::StoredContractByHash { hash, .. } => {
                Some(AddressableEntityIdentifier::Hash(*hash))
            }
            ExecutableDeployItem::StoredContractByName { name, .. } => {
                Some(AddressableEntityIdentifier::Name(name.clone()))
            }
        }
    }

    /// Returns the identifier of the contract package in the deploy item, if present.
    pub fn contract_package_identifier(&self) -> Option<PackageIdentifier> {
        match self {
            ExecutableDeployItem::ModuleBytes { .. }
            | ExecutableDeployItem::StoredContractByHash { .. }
            | ExecutableDeployItem::StoredContractByName { .. }
            | ExecutableDeployItem::Transfer { .. } => None,

            ExecutableDeployItem::StoredVersionedContractByHash { hash, version, .. } => {
                Some(PackageIdentifier::Hash {
                    package_hash: *hash,
                    version: *version,
                })
            }
            ExecutableDeployItem::StoredVersionedContractByName { name, version, .. } => {
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
            ExecutableDeployItem::ModuleBytes { args, .. }
            | ExecutableDeployItem::StoredContractByHash { args, .. }
            | ExecutableDeployItem::StoredContractByName { args, .. }
            | ExecutableDeployItem::StoredVersionedContractByHash { args, .. }
            | ExecutableDeployItem::StoredVersionedContractByName { args, .. }
            | ExecutableDeployItem::Transfer { args } => args,
        }
    }

    /// Returns the payment amount from args (if any) as Gas.
    pub fn payment_amount(&self, conv_rate: u64) -> Option<Gas> {
        let cl_value = self.args().get(ARG_AMOUNT)?;
        let motes = cl_value.clone().into_t::<U512>().ok()?;
        Gas::from_motes(Motes::new(motes), conv_rate)
    }

    /// Returns `true` if this deploy item is a native transfer.
    pub fn is_transfer(&self) -> bool {
        matches!(self, ExecutableDeployItem::Transfer { .. })
    }

    /// Returns `true` if this deploy item is a standard payment.
    pub fn is_standard_payment(&self, phase: Phase) -> bool {
        if phase != Phase::Payment {
            return false;
        }

        if let ExecutableDeployItem::ModuleBytes { module_bytes, .. } = self {
            return module_bytes.is_empty();
        }

        false
    }

    /// Returns `true` if the deploy item is a contract identified by its name.
    pub fn is_by_name(&self) -> bool {
        matches!(
            self,
            ExecutableDeployItem::StoredVersionedContractByName { .. }
        ) || matches!(self, ExecutableDeployItem::StoredContractByName { .. })
    }

    /// Returns the name of the contract or contract package, if the deploy item is identified by
    /// name.
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

    /// Returns `true` if the deploy item is a stored contract.
    pub fn is_stored_contract(&self) -> bool {
        matches!(self, ExecutableDeployItem::StoredContractByHash { .. })
            || matches!(self, ExecutableDeployItem::StoredContractByName { .. })
    }

    /// Returns `true` if the deploy item is a stored contract package.
    pub fn is_stored_contract_package(&self) -> bool {
        matches!(
            self,
            ExecutableDeployItem::StoredVersionedContractByHash { .. }
        ) || matches!(
            self,
            ExecutableDeployItem::StoredVersionedContractByName { .. }
        )
    }

    /// Returns `true` if the deploy item is [`ModuleBytes`].
    ///
    /// [`ModuleBytes`]: ExecutableDeployItem::ModuleBytes
    pub fn is_module_bytes(&self) -> bool {
        matches!(self, Self::ModuleBytes { .. })
    }

    /// Returns a random `ExecutableDeployItem`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        rng.gen()
    }
}

impl ToBytes for ExecutableDeployItem {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            ExecutableDeployItem::ModuleBytes { module_bytes, args } => {
                writer.push(MODULE_BYTES_TAG);
                module_bytes.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            ExecutableDeployItem::StoredContractByHash {
                hash,
                entry_point,
                args,
            } => {
                writer.push(STORED_CONTRACT_BY_HASH_TAG);
                hash.write_bytes(writer)?;
                entry_point.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            ExecutableDeployItem::StoredContractByName {
                name,
                entry_point,
                args,
            } => {
                writer.push(STORED_CONTRACT_BY_NAME_TAG);
                name.write_bytes(writer)?;
                entry_point.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            ExecutableDeployItem::StoredVersionedContractByHash {
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
            ExecutableDeployItem::StoredVersionedContractByName {
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
            ExecutableDeployItem::Transfer { args } => {
                writer.push(TRANSFER_TAG);
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
                let (module_bytes, remainder) = Bytes::from_bytes(remainder)?;
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                Ok((
                    ExecutableDeployItem::ModuleBytes { module_bytes, args },
                    remainder,
                ))
            }
            STORED_CONTRACT_BY_HASH_TAG => {
                let (hash, remainder) = AddressableEntityHash::from_bytes(remainder)?;
                let (entry_point, remainder) = String::from_bytes(remainder)?;
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
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
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
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
                let (hash, remainder) = PackageHash::from_bytes(remainder)?;
                let (version, remainder) = Option::<EntityVersion>::from_bytes(remainder)?;
                let (entry_point, remainder) = String::from_bytes(remainder)?;
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
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
                let (version, remainder) = Option::<EntityVersion>::from_bytes(remainder)?;
                let (entry_point, remainder) = String::from_bytes(remainder)?;
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
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
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
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

#[cfg(any(feature = "testing", test))]
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
                hash: AddressableEntityHash::new(rng.gen()),
                entry_point: random_string(rng),
                args,
            },
            2 => ExecutableDeployItem::StoredContractByName {
                name: random_string(rng),
                entry_point: random_string(rng),
                args,
            },
            3 => ExecutableDeployItem::StoredVersionedContractByHash {
                hash: PackageHash::new(rng.gen()),
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
                let amount = rng.gen_range(2_500_000_000_u64..1_000_000_000_000_000);
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
            let executable_deploy_item = ExecutableDeployItem::random(rng);
            bytesrepr::test_serialization_roundtrip(&executable_deploy_item);
        }
    }
}
