//! Key types.

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};

use core::{
    convert::TryFrom,
    fmt::{self, Debug, Display, Formatter},
    str::FromStr,
};

#[cfg(test)]
use crate::testing::TestRng;

#[cfg(doc)]
use crate::CLValue;
use blake2::{
    digest::{Update, VariableOutput},
    VarBlake2b,
};
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    account::{AccountHash, ACCOUNT_HASH_LENGTH},
    addressable_entity,
    addressable_entity::AddressableEntityHash,
    byte_code::ByteCodeKind,
    bytesrepr::{
        self, Error, FromBytes, ToBytes, U32_SERIALIZED_LENGTH, U64_SERIALIZED_LENGTH,
        U8_SERIALIZED_LENGTH,
    },
    checksummed_hex,
    contract_messages::{self, MessageAddr, TopicNameHash, TOPIC_NAME_HASH_LENGTH},
    contract_wasm::ContractWasmHash,
    contracts::{ContractHash, ContractPackageHash},
    package::{PackageHash, PackageKindTag},
    system::auction::{BidAddr, BidAddrTag},
    uref::{self, URef, URefAddr, UREF_SERIALIZED_LENGTH},
    DeployHash, Digest, EraId, Tagged, TransferAddr, TransferFromStrError, TRANSFER_ADDR_LENGTH,
    UREF_ADDR_LENGTH,
};

const HASH_PREFIX: &str = "hash-";
const DEPLOY_INFO_PREFIX: &str = "deploy-";
const ERA_INFO_PREFIX: &str = "era-";
const BALANCE_PREFIX: &str = "balance-";
const BID_PREFIX: &str = "bid-";
const WITHDRAW_PREFIX: &str = "withdraw-";
const DICTIONARY_PREFIX: &str = "dictionary-";
const UNBOND_PREFIX: &str = "unbond-";
const SYSTEM_CONTRACT_REGISTRY_PREFIX: &str = "system-contract-registry-";
const ERA_SUMMARY_PREFIX: &str = "era-summary-";
const CHAINSPEC_REGISTRY_PREFIX: &str = "chainspec-registry-";
const CHECKSUM_REGISTRY_PREFIX: &str = "checksum-registry-";
const BID_ADDR_PREFIX: &str = "bid-addr-";
const PACKAGE_PREFIX: &str = "package-";
const ENTITY_PREFIX: &str = "addressable-entity-";
const ACCOUNT_ENTITY_PREFIX: &str = "account-";
const CONTRACT_ENTITY_PREFIX: &str = "contract-";
const SYSTEM_ENTITY_PREFIX: &str = "system-";
const BYTE_CODE_PREFIX: &str = "byte-code-";
const V1_WASM_PREFIX: &str = "v1-wasm-";
const EMPTY_PREFIX: &str = "empty-";

/// The number of bytes in a Blake2b hash
pub const BLAKE2B_DIGEST_LENGTH: usize = 32;
/// The number of bytes in a [`Key::Hash`].
pub const KEY_HASH_LENGTH: usize = 32;
/// The number of bytes in a [`Key::Transfer`].
pub const KEY_TRANSFER_LENGTH: usize = TRANSFER_ADDR_LENGTH;
/// The number of bytes in a [`Key::DeployInfo`].
pub const KEY_DEPLOY_INFO_LENGTH: usize = DeployHash::LENGTH;
/// The number of bytes in a [`Key::Dictionary`].
pub const KEY_DICTIONARY_LENGTH: usize = 32;
/// The maximum length for a `dictionary_item_key`.
pub const DICTIONARY_ITEM_KEY_MAX_LENGTH: usize = 128;
/// The maximum length for an `Addr`.
pub const ADDR_LENGTH: usize = 32;
const PADDING_BYTES: [u8; 32] = [0u8; 32];
const KEY_ID_SERIALIZED_LENGTH: usize = 1;
// u8 used to determine the ID
const KEY_HASH_SERIALIZED_LENGTH: usize = KEY_ID_SERIALIZED_LENGTH + KEY_HASH_LENGTH;
const KEY_UREF_SERIALIZED_LENGTH: usize = KEY_ID_SERIALIZED_LENGTH + UREF_SERIALIZED_LENGTH;
const KEY_TRANSFER_SERIALIZED_LENGTH: usize = KEY_ID_SERIALIZED_LENGTH + KEY_TRANSFER_LENGTH;
const KEY_DEPLOY_INFO_SERIALIZED_LENGTH: usize = KEY_ID_SERIALIZED_LENGTH + KEY_DEPLOY_INFO_LENGTH;
const KEY_ERA_INFO_SERIALIZED_LENGTH: usize = KEY_ID_SERIALIZED_LENGTH + U64_SERIALIZED_LENGTH;
const KEY_BALANCE_SERIALIZED_LENGTH: usize = KEY_ID_SERIALIZED_LENGTH + UREF_ADDR_LENGTH;
const KEY_BID_SERIALIZED_LENGTH: usize = KEY_ID_SERIALIZED_LENGTH + KEY_HASH_LENGTH;
const KEY_WITHDRAW_SERIALIZED_LENGTH: usize = KEY_ID_SERIALIZED_LENGTH + KEY_HASH_LENGTH;
const KEY_UNBOND_SERIALIZED_LENGTH: usize = KEY_ID_SERIALIZED_LENGTH + KEY_HASH_LENGTH;
const KEY_DICTIONARY_SERIALIZED_LENGTH: usize = KEY_ID_SERIALIZED_LENGTH + KEY_DICTIONARY_LENGTH;
const KEY_SYSTEM_CONTRACT_REGISTRY_SERIALIZED_LENGTH: usize =
    KEY_ID_SERIALIZED_LENGTH + PADDING_BYTES.len();
const KEY_ERA_SUMMARY_SERIALIZED_LENGTH: usize = KEY_ID_SERIALIZED_LENGTH + PADDING_BYTES.len();
const KEY_CHAINSPEC_REGISTRY_SERIALIZED_LENGTH: usize =
    KEY_ID_SERIALIZED_LENGTH + PADDING_BYTES.len();
const KEY_CHECKSUM_REGISTRY_SERIALIZED_LENGTH: usize =
    KEY_ID_SERIALIZED_LENGTH + PADDING_BYTES.len();
const KEY_PACKAGE_SERIALIZED_LENGTH: usize = KEY_ID_SERIALIZED_LENGTH + 32;
const KEY_MESSAGE_SERIALIZED_LENGTH: usize = KEY_ID_SERIALIZED_LENGTH
    + KEY_HASH_LENGTH
    + TOPIC_NAME_HASH_LENGTH
    + U8_SERIALIZED_LENGTH
    + U32_SERIALIZED_LENGTH;

const MAX_SERIALIZED_LENGTH: usize = KEY_MESSAGE_SERIALIZED_LENGTH;

/// An alias for [`Key`]s hash variant.
pub type HashAddr = [u8; KEY_HASH_LENGTH];

/// An alias for [`Key`]s package variant.
pub type PackageAddr = [u8; ADDR_LENGTH];

/// An alias for [`Key`]s entity variant.
pub type EntityAddr = [u8; ADDR_LENGTH];

/// An alias for [`Key`]s byte code variant.
pub type ByteCodeAddr = [u8; ADDR_LENGTH];

/// An alias for [`Key`]s dictionary variant.
pub type DictionaryAddr = [u8; KEY_DICTIONARY_LENGTH];

#[allow(missing_docs)]
#[derive(Debug, Copy, Clone, PartialOrd, Ord, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum KeyTag {
    Account = 0,
    Hash = 1,
    URef = 2,
    Transfer = 3,
    DeployInfo = 4,
    EraInfo = 5,
    Balance = 6,
    Bid = 7,
    Withdraw = 8,
    Dictionary = 9,
    SystemContractRegistry = 10,
    EraSummary = 11,
    Unbond = 12,
    ChainspecRegistry = 13,
    ChecksumRegistry = 14,
    BidAddr = 15,
    Package = 16,
    AddressableEntity = 17,
    ByteCode = 18,
    Message = 19,
}

impl KeyTag {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..20) {
            0 => KeyTag::Account,
            1 => KeyTag::Hash,
            2 => KeyTag::URef,
            3 => KeyTag::Transfer,
            4 => KeyTag::DeployInfo,
            5 => KeyTag::EraInfo,
            6 => KeyTag::Balance,
            7 => KeyTag::Bid,
            8 => KeyTag::Withdraw,
            9 => KeyTag::Dictionary,
            10 => KeyTag::SystemContractRegistry,
            11 => KeyTag::EraSummary,
            12 => KeyTag::Unbond,
            13 => KeyTag::ChainspecRegistry,
            14 => KeyTag::ChecksumRegistry,
            15 => KeyTag::BidAddr,
            16 => KeyTag::Package,
            17 => KeyTag::AddressableEntity,
            18 => KeyTag::ByteCode,
            19 => KeyTag::Message,
            _ => panic!(),
        }
    }
}

impl Display for KeyTag {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            KeyTag::Account => write!(f, "Account"),
            KeyTag::Hash => write!(f, "Hash"),
            KeyTag::URef => write!(f, "URef"),
            KeyTag::Transfer => write!(f, "Transfer"),
            KeyTag::DeployInfo => write!(f, "DeployInfo"),
            KeyTag::EraInfo => write!(f, "EraInfo"),
            KeyTag::Balance => write!(f, "Balance"),
            KeyTag::Bid => write!(f, "Bid"),
            KeyTag::Withdraw => write!(f, "Withdraw"),
            KeyTag::Dictionary => write!(f, "Dictionary"),
            KeyTag::SystemContractRegistry => write!(f, "SystemContractRegistry"),
            KeyTag::EraSummary => write!(f, "EraSummary"),
            KeyTag::Unbond => write!(f, "Unbond"),
            KeyTag::ChainspecRegistry => write!(f, "ChainspecRegistry"),
            KeyTag::ChecksumRegistry => write!(f, "ChecksumRegistry"),
            KeyTag::BidAddr => write!(f, "BidAddr"),
            KeyTag::Package => write!(f, "Package"),
            KeyTag::AddressableEntity => write!(f, "AddressableEntity"),
            KeyTag::ByteCode => write!(f, "ByteCode"),
            KeyTag::Message => write!(f, "Message"),
        }
    }
}

impl ToBytes for KeyTag {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = bytesrepr::unchecked_allocate_buffer(self);
        self.write_bytes(&mut result)?;
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        KEY_ID_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        writer.push(*self as u8);
        Ok(())
    }
}

impl FromBytes for KeyTag {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (id, rem) = u8::from_bytes(bytes)?;
        let tag = match id {
            tag if tag == KeyTag::Account as u8 => KeyTag::Account,
            tag if tag == KeyTag::Hash as u8 => KeyTag::Hash,
            tag if tag == KeyTag::URef as u8 => KeyTag::URef,
            tag if tag == KeyTag::Transfer as u8 => KeyTag::Transfer,
            tag if tag == KeyTag::DeployInfo as u8 => KeyTag::DeployInfo,
            tag if tag == KeyTag::EraInfo as u8 => KeyTag::EraInfo,
            tag if tag == KeyTag::Balance as u8 => KeyTag::Balance,
            tag if tag == KeyTag::Bid as u8 => KeyTag::Bid,
            tag if tag == KeyTag::Withdraw as u8 => KeyTag::Withdraw,
            tag if tag == KeyTag::Dictionary as u8 => KeyTag::Dictionary,
            tag if tag == KeyTag::SystemContractRegistry as u8 => KeyTag::SystemContractRegistry,
            tag if tag == KeyTag::EraSummary as u8 => KeyTag::EraSummary,
            tag if tag == KeyTag::Unbond as u8 => KeyTag::Unbond,
            tag if tag == KeyTag::ChainspecRegistry as u8 => KeyTag::ChainspecRegistry,
            tag if tag == KeyTag::ChecksumRegistry as u8 => KeyTag::ChecksumRegistry,
            tag if tag == KeyTag::BidAddr as u8 => KeyTag::BidAddr,
            tag if tag == KeyTag::Package as u8 => KeyTag::Package,
            tag if tag == KeyTag::AddressableEntity as u8 => KeyTag::AddressableEntity,
            tag if tag == KeyTag::ByteCode as u8 => KeyTag::ByteCode,
            tag if tag == KeyTag::Message as u8 => KeyTag::Message,
            _ => return Err(Error::Formatting),
        };
        Ok((tag, rem))
    }
}

/// The key under which data (e.g. [`CLValue`]s, smart contracts, user accounts) are stored in
/// global state.
#[repr(C)]
#[derive(PartialEq, Eq, Clone, Copy, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub enum Key {
    /// A `Key` under which a user account is stored.
    Account(AccountHash),
    /// A `Key` under which a smart contract is stored and which is the pseudo-hash of the
    /// contract.
    Hash(HashAddr),
    /// A `Key` which is a [`URef`], under which most types of data can be stored.
    URef(URef),
    /// A `Key` under which a transfer is stored.
    Transfer(TransferAddr),
    /// A `Key` under which a deploy info is stored.
    DeployInfo(DeployHash),
    /// A `Key` under which an era info is stored.
    EraInfo(EraId),
    /// A `Key` under which a purse balance is stored.
    Balance(URefAddr),
    /// A `Key` under which bid information is stored.
    Bid(AccountHash),
    /// A `Key` under which withdraw information is stored.
    Withdraw(AccountHash),
    /// A `Key` whose value is derived by hashing a [`URef`] address and arbitrary data, under
    /// which a dictionary is stored.
    Dictionary(DictionaryAddr),
    /// A `Key` under which system contract hashes are stored.
    SystemContractRegistry,
    /// A `Key` under which current era info is stored.
    EraSummary,
    /// A `Key` under which unbond information is stored.
    Unbond(AccountHash),
    /// A `Key` under which chainspec and other hashes are stored.
    ChainspecRegistry,
    /// A `Key` under which a registry of checksums is stored.
    ChecksumRegistry,
    /// A `Key` under which bid information is stored.
    BidAddr(BidAddr),
    /// A `Key` under which package information is stored.
    Package(PackageAddr),
    /// A `Key` under which an addressable entity is stored.
    AddressableEntity(PackageKindTag, EntityAddr),
    /// A `Key` under which a byte code record is stored.
    ByteCode(ByteCodeKind, ByteCodeAddr),
    /// A `Key` under which a message is stored.
    Message(MessageAddr),
}

#[cfg(feature = "json-schema")]
impl JsonSchema for Key {
    fn schema_name() -> String {
        String::from("Key")
    }

    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        let schema = gen.subschema_for::<String>();
        let mut schema_object = schema.into_object();
        schema_object.metadata().description = Some(
            "The key as a formatted string, under which data (e.g. `CLValue`s, smart contracts, \
                user accounts) are stored in global state."
                .to_string(),
        );
        schema_object.into()
    }
}

/// Errors produced when converting a `String` into a `Key`.
#[derive(Debug)]
#[non_exhaustive]
pub enum FromStrError {
    /// Account parse error.
    Account(addressable_entity::FromStrError),
    /// Hash parse error.
    Hash(String),
    /// URef parse error.
    URef(uref::FromStrError),
    /// Transfer parse error.
    Transfer(TransferFromStrError),
    /// DeployInfo parse error.
    DeployInfo(String),
    /// EraInfo parse error.
    EraInfo(String),
    /// Balance parse error.
    Balance(String),
    /// Bid parse error.
    Bid(String),
    /// Withdraw parse error.
    Withdraw(String),
    /// Dictionary parse error.
    Dictionary(String),
    /// System contract registry parse error.
    SystemContractRegistry(String),
    /// Era summary parse error.
    EraSummary(String),
    /// Unbond parse error.
    Unbond(String),
    /// Chainspec registry error.
    ChainspecRegistry(String),
    /// Checksum registry error.
    ChecksumRegistry(String),
    /// Bid parse error.
    BidAddr(String),
    /// Package parse error.
    Package(String),
    /// Entity parse error.
    AddressableEntity(String),
    /// Byte code parse error.
    ByteCode(String),
    /// Message parse error.
    Message(contract_messages::FromStrError),
    /// Unknown prefix.
    UnknownPrefix,
}

impl From<addressable_entity::FromStrError> for FromStrError {
    fn from(error: addressable_entity::FromStrError) -> Self {
        FromStrError::Account(error)
    }
}

impl From<TransferFromStrError> for FromStrError {
    fn from(error: TransferFromStrError) -> Self {
        FromStrError::Transfer(error)
    }
}

impl From<uref::FromStrError> for FromStrError {
    fn from(error: uref::FromStrError) -> Self {
        FromStrError::URef(error)
    }
}

impl From<contract_messages::FromStrError> for FromStrError {
    fn from(error: contract_messages::FromStrError) -> Self {
        FromStrError::Message(error)
    }
}

impl Display for FromStrError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            FromStrError::Account(error) => write!(f, "account-key from string error: {}", error),
            FromStrError::Hash(error) => write!(f, "hash-key from string error: {}", error),
            FromStrError::URef(error) => write!(f, "uref-key from string error: {}", error),
            FromStrError::Transfer(error) => write!(f, "transfer-key from string error: {}", error),
            FromStrError::DeployInfo(error) => {
                write!(f, "deploy-info-key from string error: {}", error)
            }
            FromStrError::EraInfo(error) => write!(f, "era-info-key from string error: {}", error),
            FromStrError::Balance(error) => write!(f, "balance-key from string error: {}", error),
            FromStrError::Bid(error) => write!(f, "bid-key from string error: {}", error),
            FromStrError::Withdraw(error) => write!(f, "withdraw-key from string error: {}", error),
            FromStrError::Dictionary(error) => {
                write!(f, "dictionary-key from string error: {}", error)
            }
            FromStrError::SystemContractRegistry(error) => {
                write!(
                    f,
                    "system-contract-registry-key from string error: {}",
                    error
                )
            }
            FromStrError::EraSummary(error) => {
                write!(f, "era-summary-key from string error: {}", error)
            }
            FromStrError::Unbond(error) => {
                write!(f, "unbond-key from string error: {}", error)
            }
            FromStrError::ChainspecRegistry(error) => {
                write!(f, "chainspec-registry-key from string error: {}", error)
            }
            FromStrError::ChecksumRegistry(error) => {
                write!(f, "checksum-registry-key from string error: {}", error)
            }
            FromStrError::BidAddr(error) => write!(f, "bid-addr-key from string error: {}", error),
            FromStrError::Package(error) => write!(f, "package-key from string error: {}", error),
            FromStrError::AddressableEntity(error) => {
                write!(f, "addressable-entity-key from string error: {}", error)
            }
            FromStrError::ByteCode(error) => {
                write!(f, "byte-code-key from string error: {}", error)
            }
            FromStrError::Message(error) => {
                write!(f, "message-key from string error: {}", error)
            }
            FromStrError::UnknownPrefix => write!(f, "unknown prefix for key"),
        }
    }
}

impl Key {
    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    pub fn type_string(&self) -> String {
        match self {
            Key::Account(_) => String::from("Key::Account"),
            Key::Hash(_) => String::from("Key::Hash"),
            Key::URef(_) => String::from("Key::URef"),
            Key::Transfer(_) => String::from("Key::Transfer"),
            Key::DeployInfo(_) => String::from("Key::DeployInfo"),
            Key::EraInfo(_) => String::from("Key::EraInfo"),
            Key::Balance(_) => String::from("Key::Balance"),
            Key::Bid(_) => String::from("Key::Bid"),
            Key::Withdraw(_) => String::from("Key::Unbond"),
            Key::Dictionary(_) => String::from("Key::Dictionary"),
            Key::SystemContractRegistry => String::from("Key::SystemContractRegistry"),
            Key::EraSummary => String::from("Key::EraSummary"),
            Key::Unbond(_) => String::from("Key::Unbond"),
            Key::ChainspecRegistry => String::from("Key::ChainspecRegistry"),
            Key::ChecksumRegistry => String::from("Key::ChecksumRegistry"),
            Key::BidAddr(_) => String::from("Key::BidAddr"),
            Key::Package(_) => String::from("Key::Package"),
            Key::AddressableEntity(..) => String::from("Key::AddressableEntity"),
            Key::ByteCode(..) => String::from("Key::ByteCode"),
            Key::Message(_) => String::from("Key::Message"),
        }
    }

    /// Returns the maximum size a [`Key`] can be serialized into.
    pub const fn max_serialized_length() -> usize {
        MAX_SERIALIZED_LENGTH
    }

    /// If `self` is of type [`Key::URef`], returns `self` with the
    /// [`AccessRights`](crate::AccessRights) stripped from the wrapped [`URef`], otherwise
    /// returns `self` unmodified.
    #[must_use]
    pub fn normalize(self) -> Key {
        match self {
            Key::URef(uref) => Key::URef(uref.remove_access_rights()),
            other => other,
        }
    }

    /// Returns a human-readable version of `self`, with the inner bytes encoded to Base16.
    pub fn to_formatted_string(self) -> String {
        match self {
            Key::Account(account_hash) => account_hash.to_formatted_string(),
            Key::Hash(addr) => format!("{}{}", HASH_PREFIX, base16::encode_lower(&addr)),
            Key::URef(uref) => uref.to_formatted_string(),
            Key::Transfer(transfer_addr) => transfer_addr.to_formatted_string(),
            Key::DeployInfo(addr) => {
                format!(
                    "{}{}",
                    DEPLOY_INFO_PREFIX,
                    base16::encode_lower(addr.as_ref())
                )
            }
            Key::EraInfo(era_id) => {
                format!("{}{}", ERA_INFO_PREFIX, era_id.value())
            }
            Key::Balance(uref_addr) => {
                format!("{}{}", BALANCE_PREFIX, base16::encode_lower(&uref_addr))
            }
            Key::Bid(account_hash) => {
                format!("{}{}", BID_PREFIX, base16::encode_lower(&account_hash))
            }
            Key::Withdraw(account_hash) => {
                format!("{}{}", WITHDRAW_PREFIX, base16::encode_lower(&account_hash))
            }
            Key::Dictionary(dictionary_addr) => {
                format!(
                    "{}{}",
                    DICTIONARY_PREFIX,
                    base16::encode_lower(&dictionary_addr)
                )
            }
            Key::SystemContractRegistry => {
                format!(
                    "{}{}",
                    SYSTEM_CONTRACT_REGISTRY_PREFIX,
                    base16::encode_lower(&PADDING_BYTES)
                )
            }
            Key::EraSummary => {
                format!(
                    "{}{}",
                    ERA_SUMMARY_PREFIX,
                    base16::encode_lower(&PADDING_BYTES)
                )
            }
            Key::Unbond(account_hash) => {
                format!("{}{}", UNBOND_PREFIX, base16::encode_lower(&account_hash))
            }
            Key::ChainspecRegistry => {
                format!(
                    "{}{}",
                    CHAINSPEC_REGISTRY_PREFIX,
                    base16::encode_lower(&PADDING_BYTES)
                )
            }
            Key::ChecksumRegistry => {
                format!(
                    "{}{}",
                    CHECKSUM_REGISTRY_PREFIX,
                    base16::encode_lower(&PADDING_BYTES)
                )
            }
            Key::BidAddr(bid_addr) => {
                format!("{}{}", BID_ADDR_PREFIX, bid_addr)
            }
            Key::Message(message_addr) => message_addr.to_formatted_string(),
            Key::Package(package_addr) => {
                format!("{}{}", PACKAGE_PREFIX, base16::encode_lower(&package_addr))
            }
            Key::AddressableEntity(package_tag, entity_addr) => match package_tag {
                PackageKindTag::System => {
                    format!(
                        "{}{}{}",
                        ENTITY_PREFIX,
                        SYSTEM_ENTITY_PREFIX,
                        base16::encode_lower(&entity_addr)
                    )
                }
                PackageKindTag::Account => {
                    format!(
                        "{}{}{}",
                        ENTITY_PREFIX,
                        ACCOUNT_ENTITY_PREFIX,
                        base16::encode_lower(&entity_addr)
                    )
                }
                PackageKindTag::SmartContract => {
                    format!(
                        "{}{}{}",
                        ENTITY_PREFIX,
                        CONTRACT_ENTITY_PREFIX,
                        base16::encode_lower(&entity_addr)
                    )
                }
            },
            Key::ByteCode(byte_code_kind, byte_code_addr) => match byte_code_kind {
                ByteCodeKind::Empty => {
                    format!(
                        "{}{}{}",
                        BYTE_CODE_PREFIX,
                        EMPTY_PREFIX,
                        base16::encode_lower(&byte_code_addr)
                    )
                }
                ByteCodeKind::V1CasperWasm => {
                    format!(
                        "{}{}{}",
                        BYTE_CODE_PREFIX,
                        V1_WASM_PREFIX,
                        base16::encode_lower(&byte_code_addr)
                    )
                }
            },
        }
    }

    /// Parses a string formatted as per `Self::to_formatted_string()` into a `Key`.
    pub fn from_formatted_str(input: &str) -> Result<Key, FromStrError> {
        match AccountHash::from_formatted_str(input) {
            Ok(account_hash) => return Ok(Key::Account(account_hash)),
            Err(addressable_entity::FromStrError::InvalidPrefix) => {}
            Err(error) => return Err(error.into()),
        }

        if let Some(hex) = input.strip_prefix(HASH_PREFIX) {
            let addr = checksummed_hex::decode(hex)
                .map_err(|error| FromStrError::Hash(error.to_string()))?;
            let hash_addr = HashAddr::try_from(addr.as_ref())
                .map_err(|error| FromStrError::Hash(error.to_string()))?;
            return Ok(Key::Hash(hash_addr));
        }

        if let Some(hex) = input.strip_prefix(DEPLOY_INFO_PREFIX) {
            let hash = checksummed_hex::decode(hex)
                .map_err(|error| FromStrError::DeployInfo(error.to_string()))?;
            let hash_array = <[u8; DeployHash::LENGTH]>::try_from(hash.as_ref())
                .map_err(|error| FromStrError::DeployInfo(error.to_string()))?;
            return Ok(Key::DeployInfo(DeployHash::new(Digest::from(hash_array))));
        }

        match TransferAddr::from_formatted_str(input) {
            Ok(transfer_addr) => return Ok(Key::Transfer(transfer_addr)),
            Err(TransferFromStrError::InvalidPrefix) => {}
            Err(error) => return Err(error.into()),
        }

        match URef::from_formatted_str(input) {
            Ok(uref) => return Ok(Key::URef(uref)),
            Err(uref::FromStrError::InvalidPrefix) => {}
            Err(error) => return Err(error.into()),
        }

        if let Some(era_summary_padding) = input.strip_prefix(ERA_SUMMARY_PREFIX) {
            let padded_bytes = checksummed_hex::decode(era_summary_padding)
                .map_err(|error| FromStrError::EraSummary(error.to_string()))?;
            let _padding: [u8; 32] = TryFrom::try_from(padded_bytes.as_ref()).map_err(|_| {
                FromStrError::EraSummary("Failed to deserialize era summary key".to_string())
            })?;
            return Ok(Key::EraSummary);
        }

        if let Some(era_id_str) = input.strip_prefix(ERA_INFO_PREFIX) {
            let era_id = EraId::from_str(era_id_str)
                .map_err(|error| FromStrError::EraInfo(error.to_string()))?;
            return Ok(Key::EraInfo(era_id));
        }

        if let Some(hex) = input.strip_prefix(BALANCE_PREFIX) {
            let addr = checksummed_hex::decode(hex)
                .map_err(|error| FromStrError::Balance(error.to_string()))?;
            let uref_addr = URefAddr::try_from(addr.as_ref())
                .map_err(|error| FromStrError::Balance(error.to_string()))?;
            return Ok(Key::Balance(uref_addr));
        }

        // note: BID_ADDR must come before BID as their heads overlap (bid- / bid-addr-)
        if let Some(hex) = input.strip_prefix(BID_ADDR_PREFIX) {
            let bytes = checksummed_hex::decode(hex)
                .map_err(|error| FromStrError::BidAddr(error.to_string()))?;
            if bytes.is_empty() {
                return Err(FromStrError::BidAddr(
                    "bytes should not be 0 len".to_string(),
                ));
            }
            let tag_bytes = <[u8; BidAddrTag::BID_ADDR_TAG_LENGTH]>::try_from(bytes[0..1].as_ref())
                .map_err(|err| FromStrError::BidAddr(err.to_string()))?;
            let tag = BidAddrTag::try_from_u8(tag_bytes[0])
                .ok_or_else(|| FromStrError::BidAddr("failed to parse bid addr tag".to_string()))?;
            let validator_bytes = <[u8; ACCOUNT_HASH_LENGTH]>::try_from(
                bytes[1..BidAddr::VALIDATOR_BID_ADDR_LENGTH].as_ref(),
            )
            .map_err(|err| FromStrError::BidAddr(err.to_string()))?;

            let bid_addr = {
                if tag == BidAddrTag::Unified {
                    BidAddr::legacy(validator_bytes)
                } else if tag == BidAddrTag::Validator {
                    BidAddr::new_validator_addr(validator_bytes)
                } else if tag == BidAddrTag::Delegator {
                    let delegator_bytes = <[u8; ACCOUNT_HASH_LENGTH]>::try_from(
                        bytes[BidAddr::VALIDATOR_BID_ADDR_LENGTH..].as_ref(),
                    )
                    .map_err(|err| FromStrError::BidAddr(err.to_string()))?;
                    BidAddr::new_delegator_addr((validator_bytes, delegator_bytes))
                } else {
                    return Err(FromStrError::BidAddr("invalid tag".to_string()));
                }
            };
            return Ok(Key::BidAddr(bid_addr));
        }

        if let Some(hex) = input.strip_prefix(BID_PREFIX) {
            let hash = checksummed_hex::decode(hex)
                .map_err(|error| FromStrError::Bid(error.to_string()))?;
            let account_hash = <[u8; ACCOUNT_HASH_LENGTH]>::try_from(hash.as_ref())
                .map_err(|error| FromStrError::Bid(error.to_string()))?;
            return Ok(Key::Bid(AccountHash::new(account_hash)));
        }

        if let Some(hex) = input.strip_prefix(WITHDRAW_PREFIX) {
            let hash = checksummed_hex::decode(hex)
                .map_err(|error| FromStrError::Withdraw(error.to_string()))?;
            let account_hash = <[u8; ACCOUNT_HASH_LENGTH]>::try_from(hash.as_ref())
                .map_err(|error| FromStrError::Withdraw(error.to_string()))?;
            return Ok(Key::Withdraw(AccountHash::new(account_hash)));
        }

        if let Some(hex) = input.strip_prefix(UNBOND_PREFIX) {
            let hash = checksummed_hex::decode(hex)
                .map_err(|error| FromStrError::Unbond(error.to_string()))?;
            let account_hash = <[u8; ACCOUNT_HASH_LENGTH]>::try_from(hash.as_ref())
                .map_err(|error| FromStrError::Unbond(error.to_string()))?;
            return Ok(Key::Unbond(AccountHash::new(account_hash)));
        }

        if let Some(dictionary_addr) = input.strip_prefix(DICTIONARY_PREFIX) {
            let dictionary_addr_bytes = checksummed_hex::decode(dictionary_addr)
                .map_err(|error| FromStrError::Dictionary(error.to_string()))?;
            let addr = DictionaryAddr::try_from(dictionary_addr_bytes.as_ref())
                .map_err(|error| FromStrError::Dictionary(error.to_string()))?;
            return Ok(Key::Dictionary(addr));
        }

        if let Some(registry_address) = input.strip_prefix(SYSTEM_CONTRACT_REGISTRY_PREFIX) {
            let padded_bytes = checksummed_hex::decode(registry_address)
                .map_err(|error| FromStrError::SystemContractRegistry(error.to_string()))?;
            let _padding: [u8; 32] = TryFrom::try_from(padded_bytes.as_ref()).map_err(|_| {
                FromStrError::SystemContractRegistry(
                    "Failed to deserialize system registry key".to_string(),
                )
            })?;
            return Ok(Key::SystemContractRegistry);
        }

        if let Some(registry_address) = input.strip_prefix(CHAINSPEC_REGISTRY_PREFIX) {
            let padded_bytes = checksummed_hex::decode(registry_address)
                .map_err(|error| FromStrError::ChainspecRegistry(error.to_string()))?;
            let _padding: [u8; 32] = TryFrom::try_from(padded_bytes.as_ref()).map_err(|_| {
                FromStrError::ChainspecRegistry(
                    "Failed to deserialize chainspec registry key".to_string(),
                )
            })?;
            return Ok(Key::ChainspecRegistry);
        }

        if let Some(registry_address) = input.strip_prefix(CHECKSUM_REGISTRY_PREFIX) {
            let padded_bytes = checksummed_hex::decode(registry_address)
                .map_err(|error| FromStrError::ChecksumRegistry(error.to_string()))?;
            let _padding: [u8; 32] = TryFrom::try_from(padded_bytes.as_ref()).map_err(|_| {
                FromStrError::ChecksumRegistry(
                    "Failed to deserialize checksum registry key".to_string(),
                )
            })?;
            return Ok(Key::ChecksumRegistry);
        }

        if let Some(package_addr) = input.strip_prefix(PACKAGE_PREFIX) {
            let package_addr_bytes = checksummed_hex::decode(package_addr)
                .map_err(|error| FromStrError::Dictionary(error.to_string()))?;
            let addr = PackageAddr::try_from(package_addr_bytes.as_ref())
                .map_err(|error| FromStrError::Package(error.to_string()))?;
            return Ok(Key::Package(addr));
        }

        if let Some(entity) = input.strip_prefix(ENTITY_PREFIX) {
            let (addr_str, tag) = if let Some(str) = entity.strip_prefix(ACCOUNT_ENTITY_PREFIX) {
                (str, PackageKindTag::Account)
            } else if let Some(str) = entity.strip_prefix(SYSTEM_ENTITY_PREFIX) {
                (str, PackageKindTag::System)
            } else if let Some(str) = entity.strip_prefix(CONTRACT_ENTITY_PREFIX) {
                (str, PackageKindTag::SmartContract)
            } else {
                return Err(FromStrError::UnknownPrefix);
            };
            let addr = checksummed_hex::decode(addr_str)
                .map_err(|error| FromStrError::AddressableEntity(error.to_string()))?;
            let entity_addr = EntityAddr::try_from(addr.as_ref())
                .map_err(|error| FromStrError::AddressableEntity(error.to_string()))?;
            return Ok(Key::AddressableEntity(tag, entity_addr));
        }

        if let Some(byte_code) = input.strip_prefix(BYTE_CODE_PREFIX) {
            let (addr_str, tag) = if let Some(str) = byte_code.strip_prefix(EMPTY_PREFIX) {
                (str, ByteCodeKind::Empty)
            } else if let Some(str) = byte_code.strip_prefix(V1_WASM_PREFIX) {
                (str, ByteCodeKind::V1CasperWasm)
            } else {
                return Err(FromStrError::UnknownPrefix);
            };
            let addr = checksummed_hex::decode(addr_str)
                .map_err(|error| FromStrError::ByteCode(error.to_string()))?;
            let byte_code_addr = ByteCodeAddr::try_from(addr.as_ref())
                .map_err(|error| FromStrError::ByteCode(error.to_string()))?;
            return Ok(Key::ByteCode(tag, byte_code_addr));
        }

        match MessageAddr::from_formatted_str(input) {
            Ok(message_addr) => return Ok(Key::Message(message_addr)),
            Err(contract_messages::FromStrError::InvalidPrefix) => {}
            Err(error) => return Err(error.into()),
        }

        Err(FromStrError::UnknownPrefix)
    }

    /// Returns the inner bytes of `self` if `self` is of type [`Key::Account`], otherwise returns
    /// `None`.
    pub fn into_account(self) -> Option<AccountHash> {
        match self {
            Key::Account(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// Returns the inner bytes of `self` if `self` is of type [`Key::Hash`], otherwise returns
    /// `None`.
    pub fn into_hash_addr(self) -> Option<HashAddr> {
        match self {
            Key::Hash(hash) => Some(hash),
            _ => None,
        }
    }

    /// Returns the inner bytes of `self` if `self` is of type [`Key::AddressableEntity`], otherwise
    /// returns `None`.
    pub fn into_entity_addr(self) -> Option<EntityAddr> {
        match self {
            Key::AddressableEntity(_, hash) => Some(hash),
            _ => None,
        }
    }

    /// Returns the inner bytes of `self` if `self` is of type [`Key::Package`], otherwise returns
    /// `None`.
    pub fn into_package_addr(self) -> Option<PackageAddr> {
        match self {
            Key::Package(package_addr) => Some(package_addr),
            _ => None,
        }
    }

    /// Returns [`AddressableEntityHash`] of `self` if `self` is of type [`Key::AddressableEntity`],
    /// otherwise returns `None`.
    pub fn into_entity_hash(self) -> Option<AddressableEntityHash> {
        let entity_addr = self.into_entity_addr()?;
        Some(AddressableEntityHash::new(entity_addr))
    }

    /// Returns [`PackageHash`] of `self` if `self` is of type [`Key::Package`], otherwise
    /// returns `None`.
    pub fn into_package_hash(self) -> Option<PackageHash> {
        let package_addr = self.into_package_addr()?;
        Some(PackageHash::new(package_addr))
    }

    /// Returns a reference to the inner [`URef`] if `self` is of type [`Key::URef`], otherwise
    /// returns `None`.
    pub fn as_uref(&self) -> Option<&URef> {
        match self {
            Key::URef(uref) => Some(uref),
            _ => None,
        }
    }

    /// Returns a reference to the inner [`URef`] if `self` is of type [`Key::URef`], otherwise
    /// returns `None`.
    pub fn as_uref_mut(&mut self) -> Option<&mut URef> {
        match self {
            Key::URef(uref) => Some(uref),
            _ => None,
        }
    }

    /// Returns a reference to the inner `URefAddr` if `self` is of type [`Key::Balance`],
    /// otherwise returns `None`.
    pub fn as_balance(&self) -> Option<&URefAddr> {
        if let Self::Balance(v) = self {
            Some(v)
        } else {
            None
        }
    }

    /// Returns the inner [`URef`] if `self` is of type [`Key::URef`], otherwise returns `None`.
    pub fn into_uref(self) -> Option<URef> {
        match self {
            Key::URef(uref) => Some(uref),
            _ => None,
        }
    }

    /// Returns a reference to the inner [`DictionaryAddr`] if `self` is of type
    /// [`Key::Dictionary`], otherwise returns `None`.
    pub fn as_dictionary(&self) -> Option<&DictionaryAddr> {
        match self {
            Key::Dictionary(v) => Some(v),
            _ => None,
        }
    }

    /// Casts a [`Key::URef`] to a [`Key::Hash`]
    pub fn uref_to_hash(&self) -> Option<Key> {
        let uref = self.as_uref()?;
        let addr = uref.addr();
        Some(Key::Hash(addr))
    }

    /// Casts a [`Key::Withdraw`] to a [`Key::Unbond`]
    pub fn withdraw_to_unbond(&self) -> Option<Key> {
        if let Key::Withdraw(account_hash) = self {
            return Some(Key::Unbond(*account_hash));
        }
        None
    }

    /// Creates a new [`Key::Dictionary`] variant based on a `seed_uref` and a `dictionary_item_key`
    /// bytes.
    pub fn dictionary(seed_uref: URef, dictionary_item_key: &[u8]) -> Key {
        // NOTE: Expect below is safe because the length passed is supported.
        let mut hasher = VarBlake2b::new(BLAKE2B_DIGEST_LENGTH).expect("should create hasher");
        hasher.update(seed_uref.addr().as_ref());
        hasher.update(dictionary_item_key);
        // NOTE: Assumed safe as size of `HashAddr` equals to the output provided by hasher.
        let mut addr = HashAddr::default();
        hasher.finalize_variable(|hash| addr.clone_from_slice(hash));
        Key::Dictionary(addr)
    }

    /// Creates a new [`Key::AddressableEntity`] variant from a package kind and an entity
    /// hash.
    pub fn addressable_entity_key(
        package_kind_tag: PackageKindTag,
        entity_hash: AddressableEntityHash,
    ) -> Self {
        Key::AddressableEntity(package_kind_tag, entity_hash.value())
    }

    /// Creates a new [`Key::AddressableEntity`] for a Smart contract.
    pub fn contract_entity_key(entity_hash: AddressableEntityHash) -> Key {
        Self::addressable_entity_key(PackageKindTag::SmartContract, entity_hash)
    }

    /// Creates a new [`Key::ByteCode`] variant from a byte code kind and an byte code addr.
    pub fn byte_code_key(byte_code_kind: ByteCodeKind, byte_code_addr: ByteCodeAddr) -> Self {
        Key::ByteCode(byte_code_kind, byte_code_addr)
    }

    /// Creates a new [`Key::Message`] variant that identifies an indexed message based on an
    /// `entity_addr`, `topic_name_hash` and message `index`.
    pub fn message(
        entity_addr: AddressableEntityHash,
        topic_name_hash: TopicNameHash,
        index: u32,
    ) -> Key {
        Key::Message(MessageAddr::new_message_addr(
            entity_addr,
            topic_name_hash,
            index,
        ))
    }

    /// Creates a new [`Key::Message`] variant that identifies a message topic based on an
    /// `entity_addr` and a hash of the topic name.
    pub fn message_topic(
        entity_addr: AddressableEntityHash,
        topic_name_hash: TopicNameHash,
    ) -> Key {
        Key::Message(MessageAddr::new_topic_addr(entity_addr, topic_name_hash))
    }

    /// Returns true if the key is of type [`Key::Dictionary`].
    pub fn is_dictionary_key(&self) -> bool {
        if let Key::Dictionary(_) = self {
            return true;
        }
        false
    }

    /// Returns true if the key is of type [`Key::Bid`].
    pub fn is_balance_key(&self) -> bool {
        if let Key::Balance(_) = self {
            return true;
        }
        false
    }

    /// Returns true if the key is of type [`Key::BidAddr`].
    pub fn is_bid_addr_key(&self) -> bool {
        if let Key::BidAddr(_) = self {
            return true;
        }
        false
    }

    /// Returns a reference to the inner `BidAddr` if `self` is of type [`Key::Bid`],
    /// otherwise returns `None`.
    pub fn as_bid_addr(&self) -> Option<&BidAddr> {
        if let Self::BidAddr(addr) = self {
            Some(addr)
        } else {
            None
        }
    }

    /// Returns if they inner Key is for a system contract entity.
    pub fn is_system_key(&self) -> bool {
        if let Self::AddressableEntity(PackageKindTag::System, _) = self {
            return true;
        }

        false
    }

    /// Return true if the inner Key is of the smart contract type.
    pub fn is_smart_contract_key(&self) -> bool {
        if let Self::AddressableEntity(PackageKindTag::SmartContract, _) = self {
            return true;
        }

        false
    }
}

impl Display for Key {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Key::Account(account_hash) => write!(f, "Key::Account({})", account_hash),
            Key::Hash(addr) => write!(f, "Key::Hash({})", base16::encode_lower(&addr)),
            Key::URef(uref) => write!(f, "Key::{}", uref), /* Display impl for URef will append */
            Key::Transfer(transfer_addr) => write!(f, "Key::Transfer({})", transfer_addr),
            Key::DeployInfo(addr) => write!(
                f,
                "Key::DeployInfo({})",
                base16::encode_lower(addr.as_ref())
            ),
            Key::EraInfo(era_id) => write!(f, "Key::EraInfo({})", era_id),
            Key::Balance(uref_addr) => {
                write!(f, "Key::Balance({})", base16::encode_lower(uref_addr))
            }
            Key::Bid(account_hash) => write!(f, "Key::Bid({})", account_hash),
            Key::Withdraw(account_hash) => write!(f, "Key::Withdraw({})", account_hash),
            Key::Dictionary(addr) => {
                write!(f, "Key::Dictionary({})", base16::encode_lower(addr))
            }
            Key::SystemContractRegistry => write!(
                f,
                "Key::SystemContractRegistry({})",
                base16::encode_lower(&PADDING_BYTES)
            ),
            Key::EraSummary => write!(
                f,
                "Key::EraSummary({})",
                base16::encode_lower(&PADDING_BYTES),
            ),
            Key::Unbond(account_hash) => write!(f, "Key::Unbond({})", account_hash),
            Key::ChainspecRegistry => write!(
                f,
                "Key::ChainspecRegistry({})",
                base16::encode_lower(&PADDING_BYTES)
            ),
            Key::ChecksumRegistry => {
                write!(
                    f,
                    "Key::ChecksumRegistry({})",
                    base16::encode_lower(&PADDING_BYTES)
                )
            }
            Key::BidAddr(bid_addr) => write!(f, "Key::BidAddr({})", bid_addr),
            Key::Message(message_addr) => {
                write!(f, "Key::Message({})", message_addr)
            }
            Key::Package(package_addr) => {
                write!(f, "Key::Package({})", base16::encode_lower(package_addr))
            }
            Key::AddressableEntity(kind_tag, entity_addr) => write!(
                f,
                "Key::AddressableEntity({}-{})",
                kind_tag,
                base16::encode_lower(entity_addr)
            ),
            Key::ByteCode(kind, byte_code_addr) => {
                write!(
                    f,
                    "Key::ByteCode({}-{})",
                    kind,
                    base16::encode_lower(byte_code_addr)
                )
            }
        }
    }
}

impl Debug for Key {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl Tagged<KeyTag> for Key {
    fn tag(&self) -> KeyTag {
        match self {
            Key::Account(_) => KeyTag::Account,
            Key::Hash(_) => KeyTag::Hash,
            Key::URef(_) => KeyTag::URef,
            Key::Transfer(_) => KeyTag::Transfer,
            Key::DeployInfo(_) => KeyTag::DeployInfo,
            Key::EraInfo(_) => KeyTag::EraInfo,
            Key::Balance(_) => KeyTag::Balance,
            Key::Bid(_) => KeyTag::Bid,
            Key::Withdraw(_) => KeyTag::Withdraw,
            Key::Dictionary(_) => KeyTag::Dictionary,
            Key::SystemContractRegistry => KeyTag::SystemContractRegistry,
            Key::EraSummary => KeyTag::EraSummary,
            Key::Unbond(_) => KeyTag::Unbond,
            Key::ChainspecRegistry => KeyTag::ChainspecRegistry,
            Key::ChecksumRegistry => KeyTag::ChecksumRegistry,
            Key::BidAddr(_) => KeyTag::BidAddr,
            Key::Package(_) => KeyTag::Package,
            Key::AddressableEntity(..) => KeyTag::AddressableEntity,
            Key::ByteCode(..) => KeyTag::ByteCode,
            Key::Message(_) => KeyTag::Message,
        }
    }
}

impl Tagged<u8> for Key {
    fn tag(&self) -> u8 {
        let key_tag: KeyTag = self.tag();
        key_tag as u8
    }
}

impl From<URef> for Key {
    fn from(uref: URef) -> Key {
        Key::URef(uref)
    }
}

impl From<AccountHash> for Key {
    fn from(account_hash: AccountHash) -> Key {
        Key::Account(account_hash)
    }
}

impl From<TransferAddr> for Key {
    fn from(transfer_addr: TransferAddr) -> Key {
        Key::Transfer(transfer_addr)
    }
}

impl From<PackageHash> for Key {
    fn from(package_hash: PackageHash) -> Key {
        Key::Package(package_hash.value())
    }
}

impl From<ContractWasmHash> for Key {
    fn from(wasm_hash: ContractWasmHash) -> Self {
        Key::Hash(wasm_hash.value())
    }
}

impl From<ContractPackageHash> for Key {
    fn from(contract_package_hash: ContractPackageHash) -> Self {
        Key::Hash(contract_package_hash.value())
    }
}

impl From<ContractHash> for Key {
    fn from(contract_hash: ContractHash) -> Self {
        Key::Hash(contract_hash.value())
    }
}

impl ToBytes for Key {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = bytesrepr::unchecked_allocate_buffer(self);
        self.write_bytes(&mut result)?;
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        match self {
            Key::Account(account_hash) => {
                KEY_ID_SERIALIZED_LENGTH + account_hash.serialized_length()
            }
            Key::Hash(_) => KEY_HASH_SERIALIZED_LENGTH,
            Key::URef(_) => KEY_UREF_SERIALIZED_LENGTH,
            Key::Transfer(_) => KEY_TRANSFER_SERIALIZED_LENGTH,
            Key::DeployInfo(_) => KEY_DEPLOY_INFO_SERIALIZED_LENGTH,
            Key::EraInfo(_) => KEY_ERA_INFO_SERIALIZED_LENGTH,
            Key::Balance(_) => KEY_BALANCE_SERIALIZED_LENGTH,
            Key::Bid(_) => KEY_BID_SERIALIZED_LENGTH,
            Key::Withdraw(_) => KEY_WITHDRAW_SERIALIZED_LENGTH,
            Key::Dictionary(_) => KEY_DICTIONARY_SERIALIZED_LENGTH,
            Key::SystemContractRegistry => KEY_SYSTEM_CONTRACT_REGISTRY_SERIALIZED_LENGTH,
            Key::EraSummary => KEY_ERA_SUMMARY_SERIALIZED_LENGTH,
            Key::Unbond(_) => KEY_UNBOND_SERIALIZED_LENGTH,
            Key::ChainspecRegistry => KEY_CHAINSPEC_REGISTRY_SERIALIZED_LENGTH,
            Key::ChecksumRegistry => KEY_CHECKSUM_REGISTRY_SERIALIZED_LENGTH,
            Key::BidAddr(bid_addr) => match bid_addr.tag() {
                BidAddrTag::Unified => KEY_ID_SERIALIZED_LENGTH + bid_addr.serialized_length() - 1,
                BidAddrTag::Validator | BidAddrTag::Delegator => {
                    KEY_ID_SERIALIZED_LENGTH + bid_addr.serialized_length()
                }
            },
            Key::Package(_) => KEY_PACKAGE_SERIALIZED_LENGTH,
            Key::AddressableEntity(..) => {
                U8_SERIALIZED_LENGTH + KEY_ID_SERIALIZED_LENGTH + ADDR_LENGTH
            }
            Key::ByteCode(..) => U8_SERIALIZED_LENGTH + KEY_ID_SERIALIZED_LENGTH + ADDR_LENGTH,
            Key::Message(message_addr) => {
                KEY_ID_SERIALIZED_LENGTH + message_addr.serialized_length()
            }
        }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        writer.push(self.tag());
        match self {
            Key::Account(account_hash) => account_hash.write_bytes(writer),
            Key::Hash(hash) => hash.write_bytes(writer),
            Key::URef(uref) => uref.write_bytes(writer),
            Key::Transfer(addr) => addr.write_bytes(writer),
            Key::DeployInfo(deploy_hash) => deploy_hash.write_bytes(writer),
            Key::EraInfo(era_id) => era_id.write_bytes(writer),
            Key::Balance(uref_addr) => uref_addr.write_bytes(writer),
            Key::Bid(account_hash) => account_hash.write_bytes(writer),
            Key::Withdraw(account_hash) => account_hash.write_bytes(writer),
            Key::Dictionary(addr) => addr.write_bytes(writer),
            Key::Unbond(account_hash) => account_hash.write_bytes(writer),
            Key::SystemContractRegistry
            | Key::EraSummary
            | Key::ChainspecRegistry
            | Key::ChecksumRegistry => PADDING_BYTES.write_bytes(writer),
            Key::BidAddr(bid_addr) => match bid_addr.tag() {
                BidAddrTag::Unified => {
                    let bytes = bid_addr.to_bytes()?;
                    writer.extend(&bytes[1..]);
                    Ok(())
                }
                BidAddrTag::Validator | BidAddrTag::Delegator => bid_addr.write_bytes(writer),
            },
            Key::Package(package_addr) => package_addr.write_bytes(writer),
            Key::AddressableEntity(package_kind_tag, entity_addr) => {
                package_kind_tag.write_bytes(writer)?;
                entity_addr.write_bytes(writer)
            }
            Key::ByteCode(byte_code_kind, byte_code_addr) => {
                byte_code_kind.write_bytes(writer)?;
                byte_code_addr.write_bytes(writer)
            }
            Key::Message(message_addr) => message_addr.write_bytes(writer),
        }
    }
}

impl FromBytes for Key {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (tag, remainder) = KeyTag::from_bytes(bytes)?;
        match tag {
            KeyTag::Account => {
                let (account_hash, rem) = AccountHash::from_bytes(remainder)?;
                Ok((Key::Account(account_hash), rem))
            }
            KeyTag::Hash => {
                let (hash, rem) = HashAddr::from_bytes(remainder)?;
                Ok((Key::Hash(hash), rem))
            }
            KeyTag::URef => {
                let (uref, rem) = URef::from_bytes(remainder)?;
                Ok((Key::URef(uref), rem))
            }
            KeyTag::Transfer => {
                let (transfer_addr, rem) = TransferAddr::from_bytes(remainder)?;
                Ok((Key::Transfer(transfer_addr), rem))
            }
            KeyTag::DeployInfo => {
                let (deploy_hash, rem) = DeployHash::from_bytes(remainder)?;
                Ok((Key::DeployInfo(deploy_hash), rem))
            }
            KeyTag::EraInfo => {
                let (era_id, rem) = EraId::from_bytes(remainder)?;
                Ok((Key::EraInfo(era_id), rem))
            }
            KeyTag::Balance => {
                let (uref_addr, rem) = URefAddr::from_bytes(remainder)?;
                Ok((Key::Balance(uref_addr), rem))
            }
            KeyTag::Bid => {
                let (account_hash, rem) = AccountHash::from_bytes(remainder)?;
                Ok((Key::Bid(account_hash), rem))
            }
            KeyTag::Withdraw => {
                let (account_hash, rem) = AccountHash::from_bytes(remainder)?;
                Ok((Key::Withdraw(account_hash), rem))
            }
            KeyTag::Dictionary => {
                let (addr, rem) = DictionaryAddr::from_bytes(remainder)?;
                Ok((Key::Dictionary(addr), rem))
            }
            KeyTag::SystemContractRegistry => {
                let (_, rem) = <[u8; 32]>::from_bytes(remainder)?;
                Ok((Key::SystemContractRegistry, rem))
            }
            KeyTag::EraSummary => {
                let (_, rem) = <[u8; 32]>::from_bytes(remainder)?;
                Ok((Key::EraSummary, rem))
            }
            KeyTag::Unbond => {
                let (account_hash, rem) = AccountHash::from_bytes(remainder)?;
                Ok((Key::Unbond(account_hash), rem))
            }
            KeyTag::ChainspecRegistry => {
                let (_, rem) = <[u8; 32]>::from_bytes(remainder)?;
                Ok((Key::ChainspecRegistry, rem))
            }
            KeyTag::ChecksumRegistry => {
                let (_, rem) = <[u8; 32]>::from_bytes(remainder)?;
                Ok((Key::ChecksumRegistry, rem))
            }
            KeyTag::BidAddr => {
                let (bid_addr, rem) = BidAddr::from_bytes(remainder)?;
                Ok((Key::BidAddr(bid_addr), rem))
            }
            KeyTag::Package => {
                let (package_addr, rem) = PackageAddr::from_bytes(remainder)?;
                Ok((Key::Package(package_addr), rem))
            }
            KeyTag::AddressableEntity => {
                let (package_kind_tag, rem) = PackageKindTag::from_bytes(remainder)?;
                let (entity_addr, rem) = EntityAddr::from_bytes(rem)?;
                Ok((Key::AddressableEntity(package_kind_tag, entity_addr), rem))
            }
            KeyTag::ByteCode => {
                let (byte_code_kind, rem) = ByteCodeKind::from_bytes(remainder)?;
                let (byte_code_addr, rem) = ByteCodeAddr::from_bytes(rem)?;
                Ok((Key::ByteCode(byte_code_kind, byte_code_addr), rem))
            }
            KeyTag::Message => {
                let (message_addr, rem) = MessageAddr::from_bytes(remainder)?;
                Ok((Key::Message(message_addr), rem))
            }
        }
    }
}

#[allow(dead_code)]
fn please_add_to_distribution_impl(key: Key) {
    // If you've been forced to come here, you likely need to add your variant to the
    // `Distribution` impl for `Key`.
    match key {
        Key::Account(_) => unimplemented!(),
        Key::Hash(_) => unimplemented!(),
        Key::URef(_) => unimplemented!(),
        Key::Transfer(_) => unimplemented!(),
        Key::DeployInfo(_) => unimplemented!(),
        Key::EraInfo(_) => unimplemented!(),
        Key::Balance(_) => unimplemented!(),
        Key::Bid(_) => unimplemented!(),
        Key::Withdraw(_) => unimplemented!(),
        Key::Dictionary(_) => unimplemented!(),
        Key::SystemContractRegistry => unimplemented!(),
        Key::EraSummary => unimplemented!(),
        Key::Unbond(_) => unimplemented!(),
        Key::ChainspecRegistry => unimplemented!(),
        Key::ChecksumRegistry => unimplemented!(),
        Key::BidAddr(_) => unimplemented!(),
        Key::Package(_) => unimplemented!(),
        Key::AddressableEntity(..) => unimplemented!(),
        Key::ByteCode(..) => unimplemented!(),
        Key::Message(_) => unimplemented!(),
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<Key> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Key {
        match rng.gen_range(0..=18) {
            0 => Key::Account(rng.gen()),
            1 => Key::Hash(rng.gen()),
            2 => Key::URef(rng.gen()),
            3 => Key::Transfer(rng.gen()),
            4 => Key::DeployInfo(DeployHash::from_raw(rng.gen())),
            5 => Key::EraInfo(EraId::new(rng.gen())),
            6 => Key::Balance(rng.gen()),
            7 => Key::Bid(rng.gen()),
            8 => Key::Withdraw(rng.gen()),
            9 => Key::Dictionary(rng.gen()),
            10 => Key::SystemContractRegistry,
            11 => Key::EraSummary,
            12 => Key::Unbond(rng.gen()),
            13 => Key::ChainspecRegistry,
            14 => Key::ChecksumRegistry,
            15 => Key::BidAddr(rng.gen()),
            16 => Key::Package(rng.gen()),
            17 => Key::AddressableEntity(rng.gen(), rng.gen()),
            18 => Key::ByteCode(rng.gen(), rng.gen()),
            19 => Key::Message(rng.gen()),
            _ => unreachable!(),
        }
    }
}

mod serde_helpers {
    use super::*;

    #[derive(Serialize)]
    pub(super) enum BinarySerHelper<'a> {
        Account(&'a AccountHash),
        Hash(&'a HashAddr),
        URef(&'a URef),
        Transfer(&'a TransferAddr),
        #[serde(with = "crate::serde_helpers::deploy_hash_as_array")]
        DeployInfo(&'a DeployHash),
        EraInfo(&'a EraId),
        Balance(&'a URefAddr),
        Bid(&'a AccountHash),
        Withdraw(&'a AccountHash),
        Dictionary(&'a HashAddr),
        SystemContractRegistry,
        EraSummary,
        Unbond(&'a AccountHash),
        ChainspecRegistry,
        ChecksumRegistry,
        BidAddr(&'a BidAddr),
        Package(&'a PackageAddr),
        AddressableEntity(&'a PackageKindTag, &'a EntityAddr),
        ByteCode(&'a ByteCodeKind, &'a ByteCodeAddr),
        Message(&'a MessageAddr),
    }

    #[derive(Deserialize)]
    pub(super) enum BinaryDeserHelper {
        Account(AccountHash),
        Hash(HashAddr),
        URef(URef),
        Transfer(TransferAddr),
        #[serde(with = "crate::serde_helpers::deploy_hash_as_array")]
        DeployInfo(DeployHash),
        EraInfo(EraId),
        Balance(URefAddr),
        Bid(AccountHash),
        Withdraw(AccountHash),
        Dictionary(DictionaryAddr),
        SystemContractRegistry,
        EraSummary,
        Unbond(AccountHash),
        ChainspecRegistry,
        ChecksumRegistry,
        BidAddr(BidAddr),
        Package(PackageAddr),
        AddressableEntity(PackageKindTag, EntityAddr),
        ByteCode(ByteCodeKind, ByteCodeAddr),
        Message(MessageAddr),
    }

    impl<'a> From<&'a Key> for BinarySerHelper<'a> {
        fn from(key: &'a Key) -> Self {
            match key {
                Key::Account(account_hash) => BinarySerHelper::Account(account_hash),
                Key::Hash(hash_addr) => BinarySerHelper::Hash(hash_addr),
                Key::URef(uref) => BinarySerHelper::URef(uref),
                Key::Transfer(transfer_addr) => BinarySerHelper::Transfer(transfer_addr),
                Key::DeployInfo(deploy_hash) => BinarySerHelper::DeployInfo(deploy_hash),
                Key::EraInfo(era_id) => BinarySerHelper::EraInfo(era_id),
                Key::Balance(uref_addr) => BinarySerHelper::Balance(uref_addr),
                Key::Bid(account_hash) => BinarySerHelper::Bid(account_hash),
                Key::Withdraw(account_hash) => BinarySerHelper::Withdraw(account_hash),
                Key::Dictionary(addr) => BinarySerHelper::Dictionary(addr),
                Key::SystemContractRegistry => BinarySerHelper::SystemContractRegistry,
                Key::EraSummary => BinarySerHelper::EraSummary,
                Key::Unbond(account_hash) => BinarySerHelper::Unbond(account_hash),
                Key::ChainspecRegistry => BinarySerHelper::ChainspecRegistry,
                Key::ChecksumRegistry => BinarySerHelper::ChecksumRegistry,
                Key::BidAddr(bid_addr) => BinarySerHelper::BidAddr(bid_addr),
                Key::Message(message_addr) => BinarySerHelper::Message(message_addr),
                Key::Package(package_addr) => BinarySerHelper::Package(package_addr),
                Key::AddressableEntity(package_kind, entity_addr) => {
                    BinarySerHelper::AddressableEntity(package_kind, entity_addr)
                }
                Key::ByteCode(byte_code_kind, byte_code_addr) => {
                    BinarySerHelper::ByteCode(byte_code_kind, byte_code_addr)
                }
            }
        }
    }

    impl From<BinaryDeserHelper> for Key {
        fn from(helper: BinaryDeserHelper) -> Self {
            match helper {
                BinaryDeserHelper::Account(account_hash) => Key::Account(account_hash),
                BinaryDeserHelper::Hash(hash_addr) => Key::Hash(hash_addr),
                BinaryDeserHelper::URef(uref) => Key::URef(uref),
                BinaryDeserHelper::Transfer(transfer_addr) => Key::Transfer(transfer_addr),
                BinaryDeserHelper::DeployInfo(deploy_hash) => Key::DeployInfo(deploy_hash),
                BinaryDeserHelper::EraInfo(era_id) => Key::EraInfo(era_id),
                BinaryDeserHelper::Balance(uref_addr) => Key::Balance(uref_addr),
                BinaryDeserHelper::Bid(account_hash) => Key::Bid(account_hash),
                BinaryDeserHelper::Withdraw(account_hash) => Key::Withdraw(account_hash),
                BinaryDeserHelper::Dictionary(addr) => Key::Dictionary(addr),
                BinaryDeserHelper::SystemContractRegistry => Key::SystemContractRegistry,
                BinaryDeserHelper::EraSummary => Key::EraSummary,
                BinaryDeserHelper::Unbond(account_hash) => Key::Unbond(account_hash),
                BinaryDeserHelper::ChainspecRegistry => Key::ChainspecRegistry,
                BinaryDeserHelper::ChecksumRegistry => Key::ChecksumRegistry,
                BinaryDeserHelper::BidAddr(bid_addr) => Key::BidAddr(bid_addr),
                BinaryDeserHelper::Message(message_addr) => Key::Message(message_addr),
                BinaryDeserHelper::Package(package_addr) => Key::Package(package_addr),
                BinaryDeserHelper::AddressableEntity(package_kind, entity_addr) => {
                    Key::AddressableEntity(package_kind, entity_addr)
                }
                BinaryDeserHelper::ByteCode(byte_kind, byte_code_addr) => {
                    Key::ByteCode(byte_kind, byte_code_addr)
                }
            }
        }
    }
}

impl Serialize for Key {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            self.to_formatted_string().serialize(serializer)
        } else {
            serde_helpers::BinarySerHelper::from(self).serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let formatted_key = String::deserialize(deserializer)?;
            Key::from_formatted_str(&formatted_key).map_err(SerdeError::custom)
        } else {
            let binary_helper = serde_helpers::BinaryDeserHelper::deserialize(deserializer)?;
            Ok(Key::from(binary_helper))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::string::ToString;

    use super::*;
    use crate::{
        account::ACCOUNT_HASH_FORMATTED_STRING_PREFIX,
        bytesrepr::{Error, FromBytes},
        transfer::TRANSFER_ADDR_FORMATTED_STRING_PREFIX,
        uref::UREF_FORMATTED_STRING_PREFIX,
        AccessRights, URef,
    };

    const ACCOUNT_KEY: Key = Key::Account(AccountHash::new([42; 32]));
    const HASH_KEY: Key = Key::Hash([42; 32]);
    const UREF_KEY: Key = Key::URef(URef::new([42; 32], AccessRights::READ));
    const TRANSFER_KEY: Key = Key::Transfer(TransferAddr::new([42; 32]));
    const DEPLOY_INFO_KEY: Key = Key::DeployInfo(DeployHash::from_raw([42; 32]));
    const ERA_INFO_KEY: Key = Key::EraInfo(EraId::new(42));
    const BALANCE_KEY: Key = Key::Balance([42; 32]);
    const BID_KEY: Key = Key::Bid(AccountHash::new([42; 32]));
    const UNIFIED_BID_KEY: Key = Key::BidAddr(BidAddr::legacy([42; 32]));
    const VALIDATOR_BID_KEY: Key = Key::BidAddr(BidAddr::new_validator_addr([2; 32]));
    const DELEGATOR_BID_KEY: Key = Key::BidAddr(BidAddr::new_delegator_addr(([2; 32], [9; 32])));
    const WITHDRAW_KEY: Key = Key::Withdraw(AccountHash::new([42; 32]));
    const DICTIONARY_KEY: Key = Key::Dictionary([42; 32]);
    const SYSTEM_CONTRACT_REGISTRY_KEY: Key = Key::SystemContractRegistry;
    const ERA_SUMMARY_KEY: Key = Key::EraSummary;
    const UNBOND_KEY: Key = Key::Unbond(AccountHash::new([42; 32]));
    const CHAINSPEC_REGISTRY_KEY: Key = Key::ChainspecRegistry;
    const CHECKSUM_REGISTRY_KEY: Key = Key::ChecksumRegistry;
    const PACKAGE_KEY: Key = Key::Package([42; 32]);
    const ADDRESSABLE_ENTITY_SYSTEM_KEY: Key =
        Key::AddressableEntity(PackageKindTag::System, [42; 32]);
    const ADDRESSABLE_ENTITY_ACCOUNT_KEY: Key =
        Key::AddressableEntity(PackageKindTag::Account, [42; 32]);
    const ADDRESSABLE_ENTITY_SMART_CONTRACT_KEY: Key =
        Key::AddressableEntity(PackageKindTag::SmartContract, [42; 32]);
    const BYTE_CODE_EMPTY_KEY: Key = Key::ByteCode(ByteCodeKind::Empty, [42; 32]);
    const BYTE_CODE_V1_WASM_KEY: Key = Key::ByteCode(ByteCodeKind::V1CasperWasm, [42; 32]);
    const MESSAGE_TOPIC_KEY: Key = Key::Message(MessageAddr::new_topic_addr(
        AddressableEntityHash::new([42u8; 32]),
        TopicNameHash::new([42; 32]),
    ));
    const MESSAGE_KEY: Key = Key::Message(MessageAddr::new_message_addr(
        AddressableEntityHash::new([42u8; 32]),
        TopicNameHash::new([2; 32]),
        15,
    ));
    const KEYS: &[Key] = &[
        ACCOUNT_KEY,
        HASH_KEY,
        UREF_KEY,
        TRANSFER_KEY,
        DEPLOY_INFO_KEY,
        ERA_INFO_KEY,
        BALANCE_KEY,
        BID_KEY,
        WITHDRAW_KEY,
        DICTIONARY_KEY,
        SYSTEM_CONTRACT_REGISTRY_KEY,
        ERA_SUMMARY_KEY,
        UNBOND_KEY,
        CHAINSPEC_REGISTRY_KEY,
        CHECKSUM_REGISTRY_KEY,
        UNIFIED_BID_KEY,
        VALIDATOR_BID_KEY,
        DELEGATOR_BID_KEY,
        PACKAGE_KEY,
        ADDRESSABLE_ENTITY_SYSTEM_KEY,
        ADDRESSABLE_ENTITY_ACCOUNT_KEY,
        ADDRESSABLE_ENTITY_SMART_CONTRACT_KEY,
        BYTE_CODE_EMPTY_KEY,
        BYTE_CODE_V1_WASM_KEY,
        MESSAGE_TOPIC_KEY,
        MESSAGE_KEY,
    ];
    const HEX_STRING: &str = "2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a";
    const TOPIC_NAME_HEX_STRING: &str =
        "0202020202020202020202020202020202020202020202020202020202020202";
    const MESSAGE_INDEX_HEX_STRING: &str = "f";
    const UNIFIED_HEX_STRING: &str =
        "002a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a";
    const VALIDATOR_HEX_STRING: &str =
        "010202020202020202020202020202020202020202020202020202020202020202";
    const DELEGATOR_HEX_STRING: &str =
        "0202020202020202020202020202020202020202020202020202020202020202020909090909090909090909090909090909090909090909090909090909090909";

    fn test_readable(right: AccessRights, is_true: bool) {
        assert_eq!(right.is_readable(), is_true)
    }

    #[test]
    fn test_is_readable() {
        test_readable(AccessRights::READ, true);
        test_readable(AccessRights::READ_ADD, true);
        test_readable(AccessRights::READ_WRITE, true);
        test_readable(AccessRights::READ_ADD_WRITE, true);
        test_readable(AccessRights::ADD, false);
        test_readable(AccessRights::ADD_WRITE, false);
        test_readable(AccessRights::WRITE, false);
    }

    fn test_writable(right: AccessRights, is_true: bool) {
        assert_eq!(right.is_writeable(), is_true)
    }

    #[test]
    fn test_is_writable() {
        test_writable(AccessRights::WRITE, true);
        test_writable(AccessRights::READ_WRITE, true);
        test_writable(AccessRights::ADD_WRITE, true);
        test_writable(AccessRights::READ, false);
        test_writable(AccessRights::ADD, false);
        test_writable(AccessRights::READ_ADD, false);
        test_writable(AccessRights::READ_ADD_WRITE, true);
    }

    fn test_addable(right: AccessRights, is_true: bool) {
        assert_eq!(right.is_addable(), is_true)
    }

    #[test]
    fn test_is_addable() {
        test_addable(AccessRights::ADD, true);
        test_addable(AccessRights::READ_ADD, true);
        test_addable(AccessRights::READ_WRITE, false);
        test_addable(AccessRights::ADD_WRITE, true);
        test_addable(AccessRights::READ, false);
        test_addable(AccessRights::WRITE, false);
        test_addable(AccessRights::READ_ADD_WRITE, true);
    }

    #[test]
    fn should_display_key() {
        assert_eq!(
            format!("{}", ACCOUNT_KEY),
            format!("Key::Account({})", HEX_STRING)
        );
        assert_eq!(
            format!("{}", HASH_KEY),
            format!("Key::Hash({})", HEX_STRING)
        );
        assert_eq!(
            format!("{}", UREF_KEY),
            format!("Key::URef({}, READ)", HEX_STRING)
        );
        assert_eq!(
            format!("{}", TRANSFER_KEY),
            format!("Key::Transfer({})", HEX_STRING)
        );
        assert_eq!(
            format!("{}", DEPLOY_INFO_KEY),
            format!("Key::DeployInfo({})", HEX_STRING)
        );
        assert_eq!(
            format!("{}", ERA_INFO_KEY),
            "Key::EraInfo(era 42)".to_string()
        );
        assert_eq!(
            format!("{}", BALANCE_KEY),
            format!("Key::Balance({})", HEX_STRING)
        );
        assert_eq!(format!("{}", BID_KEY), format!("Key::Bid({})", HEX_STRING));
        assert_eq!(
            format!("{}", UNIFIED_BID_KEY),
            format!("Key::BidAddr({})", UNIFIED_HEX_STRING)
        );
        assert_eq!(
            format!("{}", VALIDATOR_BID_KEY),
            format!("Key::BidAddr({})", VALIDATOR_HEX_STRING)
        );
        assert_eq!(
            format!("{}", DELEGATOR_BID_KEY),
            format!("Key::BidAddr({})", DELEGATOR_HEX_STRING)
        );
        assert_eq!(
            format!("{}", WITHDRAW_KEY),
            format!("Key::Withdraw({})", HEX_STRING)
        );
        assert_eq!(
            format!("{}", DICTIONARY_KEY),
            format!("Key::Dictionary({})", HEX_STRING)
        );
        assert_eq!(
            format!("{}", SYSTEM_CONTRACT_REGISTRY_KEY),
            format!(
                "Key::SystemContractRegistry({})",
                base16::encode_lower(&PADDING_BYTES)
            )
        );
        assert_eq!(
            format!("{}", ERA_SUMMARY_KEY),
            format!("Key::EraSummary({})", base16::encode_lower(&PADDING_BYTES))
        );
        assert_eq!(
            format!("{}", UNBOND_KEY),
            format!("Key::Unbond({})", HEX_STRING)
        );
        assert_eq!(
            format!("{}", CHAINSPEC_REGISTRY_KEY),
            format!(
                "Key::ChainspecRegistry({})",
                base16::encode_lower(&PADDING_BYTES)
            )
        );
        assert_eq!(
            format!("{}", CHECKSUM_REGISTRY_KEY),
            format!(
                "Key::ChecksumRegistry({})",
                base16::encode_lower(&PADDING_BYTES),
            )
        );
        assert_eq!(
            format!("{}", PACKAGE_KEY),
            format!("Key::Package({})", HEX_STRING)
        );
        assert_eq!(
            format!("{}", ADDRESSABLE_ENTITY_SYSTEM_KEY),
            format!("Key::AddressableEntity(system-{})", HEX_STRING)
        );
        assert_eq!(
            format!("{}", ADDRESSABLE_ENTITY_ACCOUNT_KEY),
            format!("Key::AddressableEntity(account-{})", HEX_STRING)
        );
        assert_eq!(
            format!("{}", ADDRESSABLE_ENTITY_SMART_CONTRACT_KEY),
            format!("Key::AddressableEntity(smart-contract-{})", HEX_STRING)
        );
        assert_eq!(
            format!("{}", BYTE_CODE_EMPTY_KEY),
            format!("Key::ByteCode(empty-{})", HEX_STRING)
        );
        assert_eq!(
            format!("{}", BYTE_CODE_V1_WASM_KEY),
            format!("Key::ByteCode(v1-casper-wasm-{})", HEX_STRING)
        );
        assert_eq!(
            format!("{}", MESSAGE_TOPIC_KEY),
            format!("Key::Message({}-{})", HEX_STRING, HEX_STRING)
        );

        assert_eq!(
            format!("{}", MESSAGE_KEY),
            format!(
                "Key::Message({}-{}-{})",
                HEX_STRING, TOPIC_NAME_HEX_STRING, MESSAGE_INDEX_HEX_STRING
            )
        )
    }

    #[test]
    fn abuse_vec_key() {
        // Prefix is 2^32-1 = shouldn't allocate that much
        let bytes: Vec<u8> = vec![255, 255, 255, 255, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        let res: Result<(Vec<Key>, &[u8]), _> = FromBytes::from_bytes(&bytes);
        #[cfg(target_os = "linux")]
        assert_eq!(res.expect_err("should fail"), Error::OutOfMemory);
        #[cfg(target_os = "macos")]
        assert_eq!(res.expect_err("should fail"), Error::EarlyEndOfStream);
    }

    #[test]
    fn check_key_account_getters() {
        let account = [42; 32];
        let account_hash = AccountHash::new(account);
        let key1 = Key::Account(account_hash);
        assert_eq!(key1.into_account(), Some(account_hash));
        assert!(key1.into_entity_addr().is_none());
        assert!(key1.as_uref().is_none());
    }

    #[test]
    fn check_key_hash_getters() {
        let hash = [42; KEY_HASH_LENGTH];
        let key1 = Key::Hash(hash);
        assert!(key1.into_account().is_none());
        assert_eq!(key1.into_hash_addr(), Some(hash));
        assert!(key1.as_uref().is_none());
    }

    #[test]
    fn check_entity_key_getters() {
        let hash = [42; KEY_HASH_LENGTH];
        let key1 = Key::contract_entity_key(AddressableEntityHash::new(hash));
        assert!(key1.into_account().is_none());
        assert_eq!(key1.into_entity_addr(), Some(hash));
        assert!(key1.as_uref().is_none());
    }

    #[test]
    fn check_package_key_getters() {
        let hash = [42; KEY_HASH_LENGTH];
        let key1 = Key::Package(hash);
        assert!(key1.into_account().is_none());
        assert_eq!(key1.into_package_addr(), Some(hash));
        assert!(key1.as_uref().is_none());
    }

    #[test]
    fn check_key_uref_getters() {
        let uref = URef::new([42; 32], AccessRights::READ_ADD_WRITE);
        let key1 = Key::URef(uref);
        assert!(key1.into_account().is_none());
        assert!(key1.into_entity_addr().is_none());
        assert_eq!(key1.as_uref(), Some(&uref));
    }

    #[test]
    fn key_max_serialized_length() {
        let mut got_max = false;
        for key in KEYS {
            let expected = Key::max_serialized_length();
            let actual = key.serialized_length();
            assert!(
                actual <= expected,
                "key too long {} expected {} actual {}",
                key,
                expected,
                actual
            );
            if actual == Key::max_serialized_length() {
                got_max = true;
            }
        }
        assert!(
            got_max,
            "None of the Key variants has a serialized_length equal to \
            Key::max_serialized_length(), so Key::max_serialized_length() should be reduced"
        );
    }

    #[test]
    fn should_parse_legacy_bid_key_from_string() {
        let account_hash = AccountHash([1; 32]);
        let legacy_bid_key = Key::Bid(account_hash);
        let original_string = legacy_bid_key.to_formatted_string();

        let parsed_bid_key =
            Key::from_formatted_str(&original_string).expect("{string} (key = {key:?})");
        if let Key::Bid(parsed_account_hash) = parsed_bid_key {
            assert_eq!(parsed_account_hash, account_hash,);
            assert_eq!(legacy_bid_key, parsed_bid_key);

            let translated_string = parsed_bid_key.to_formatted_string();
            assert_eq!(original_string, translated_string);
        } else {
            panic!("should have account hash");
        }
    }

    #[test]
    fn should_parse_legacy_unified_bid_key_from_string() {
        let legacy_bid_addr = BidAddr::legacy([1; 32]);
        let legacy_bid_key = Key::BidAddr(legacy_bid_addr);
        assert_eq!(legacy_bid_addr.tag(), BidAddrTag::Unified,);

        let original_string = legacy_bid_key.to_formatted_string();
        let parsed_key =
            Key::from_formatted_str(&original_string).expect("{string} (key = {key:?})");
        let parsed_bid_addr = parsed_key.as_bid_addr().expect("must have bid addr");
        assert!(parsed_key.is_bid_addr_key());
        assert_eq!(parsed_bid_addr.tag(), legacy_bid_addr.tag(),);
        assert_eq!(*parsed_bid_addr, legacy_bid_addr);

        let translated_string = parsed_key.to_formatted_string();
        assert_eq!(original_string, translated_string);
        assert_eq!(parsed_key.as_bid_addr(), legacy_bid_key.as_bid_addr(),);
    }

    #[test]
    fn should_parse_validator_bid_key_from_string() {
        let validator_bid_addr = BidAddr::new_validator_addr([1; 32]);
        let validator_bid_key = Key::BidAddr(validator_bid_addr);
        assert_eq!(validator_bid_addr.tag(), BidAddrTag::Validator,);

        let original_string = validator_bid_key.to_formatted_string();
        let parsed_key =
            Key::from_formatted_str(&original_string).expect("{string} (key = {key:?})");
        let parsed_bid_addr = parsed_key.as_bid_addr().expect("must have bid addr");
        assert!(parsed_key.is_bid_addr_key());
        assert_eq!(parsed_bid_addr.tag(), validator_bid_addr.tag(),);
        assert_eq!(*parsed_bid_addr, validator_bid_addr,);

        let translated_string = parsed_key.to_formatted_string();
        assert_eq!(original_string, translated_string);
        assert_eq!(parsed_key.as_bid_addr(), validator_bid_key.as_bid_addr(),);
    }

    #[test]
    fn should_parse_delegator_bid_key_from_string() {
        let delegator_bid_addr = BidAddr::new_delegator_addr(([1; 32], [9; 32]));
        let delegator_bid_key = Key::BidAddr(delegator_bid_addr);
        assert_eq!(delegator_bid_addr.tag(), BidAddrTag::Delegator,);

        let original_string = delegator_bid_key.to_formatted_string();

        let parsed_key =
            Key::from_formatted_str(&original_string).expect("{string} (key = {key:?})");
        let parsed_bid_addr = parsed_key.as_bid_addr().expect("must have bid addr");
        assert!(parsed_key.is_bid_addr_key());
        assert_eq!(parsed_bid_addr.tag(), delegator_bid_addr.tag(),);
        assert_eq!(*parsed_bid_addr, delegator_bid_addr,);

        let translated_string = parsed_key.to_formatted_string();
        assert_eq!(original_string, translated_string);
        assert_eq!(parsed_key.as_bid_addr(), delegator_bid_key.as_bid_addr(),);
    }

    #[test]
    fn should_parse_key_from_str() {
        for key in KEYS {
            let string = key.to_formatted_string();
            let parsed_key = Key::from_formatted_str(&string).expect("{string} (key = {key:?})");
            assert_eq!(parsed_key, *key, "{string} (key = {key:?})");
        }
    }

    #[test]
    fn should_fail_to_parse_key_from_str() {
        assert!(
            Key::from_formatted_str(ACCOUNT_HASH_FORMATTED_STRING_PREFIX)
                .unwrap_err()
                .to_string()
                .starts_with("account-key from string error: ")
        );
        assert!(Key::from_formatted_str(HASH_PREFIX)
            .unwrap_err()
            .to_string()
            .starts_with("hash-key from string error: "));
        assert!(Key::from_formatted_str(UREF_FORMATTED_STRING_PREFIX)
            .unwrap_err()
            .to_string()
            .starts_with("uref-key from string error: "));
        assert!(
            Key::from_formatted_str(TRANSFER_ADDR_FORMATTED_STRING_PREFIX)
                .unwrap_err()
                .to_string()
                .starts_with("transfer-key from string error: ")
        );
        assert!(Key::from_formatted_str(DEPLOY_INFO_PREFIX)
            .unwrap_err()
            .to_string()
            .starts_with("deploy-info-key from string error: "));
        assert!(Key::from_formatted_str(ERA_INFO_PREFIX)
            .unwrap_err()
            .to_string()
            .starts_with("era-info-key from string error: "));
        assert!(Key::from_formatted_str(BALANCE_PREFIX)
            .unwrap_err()
            .to_string()
            .starts_with("balance-key from string error: "));
        assert!(Key::from_formatted_str(BID_PREFIX)
            .unwrap_err()
            .to_string()
            .starts_with("bid-key from string error: "));
        assert!(Key::from_formatted_str(WITHDRAW_PREFIX)
            .unwrap_err()
            .to_string()
            .starts_with("withdraw-key from string error: "));
        assert!(Key::from_formatted_str(DICTIONARY_PREFIX)
            .unwrap_err()
            .to_string()
            .starts_with("dictionary-key from string error: "));
        assert!(Key::from_formatted_str(SYSTEM_CONTRACT_REGISTRY_PREFIX)
            .unwrap_err()
            .to_string()
            .starts_with("system-contract-registry-key from string error: "));
        assert!(Key::from_formatted_str(ERA_SUMMARY_PREFIX)
            .unwrap_err()
            .to_string()
            .starts_with("era-summary-key from string error"));
        assert!(Key::from_formatted_str(UNBOND_PREFIX)
            .unwrap_err()
            .to_string()
            .starts_with("unbond-key from string error: "));
        assert!(Key::from_formatted_str(CHAINSPEC_REGISTRY_PREFIX)
            .unwrap_err()
            .to_string()
            .starts_with("chainspec-registry-key from string error: "));
        assert!(Key::from_formatted_str(CHECKSUM_REGISTRY_PREFIX)
            .unwrap_err()
            .to_string()
            .starts_with("checksum-registry-key from string error: "));
        let bid_addr_err = Key::from_formatted_str(BID_ADDR_PREFIX)
            .unwrap_err()
            .to_string();
        assert!(
            bid_addr_err.starts_with("bid-addr-key from string error: "),
            "{}",
            bid_addr_err
        );
        assert!(Key::from_formatted_str(PACKAGE_PREFIX)
            .unwrap_err()
            .to_string()
            .starts_with("package-key from string error: "));
        assert!(
            Key::from_formatted_str(&format!("{}{}", ENTITY_PREFIX, ACCOUNT_ENTITY_PREFIX))
                .unwrap_err()
                .to_string()
                .starts_with("addressable-entity-key from string error: ")
        );
        assert!(
            Key::from_formatted_str(&format!("{}{}", BYTE_CODE_PREFIX, EMPTY_PREFIX))
                .unwrap_err()
                .to_string()
                .starts_with("byte-code-key from string error: ")
        );
        let invalid_prefix = "a-0000000000000000000000000000000000000000000000000000000000000000";
        assert_eq!(
            Key::from_formatted_str(invalid_prefix)
                .unwrap_err()
                .to_string(),
            "unknown prefix for key"
        );

        let missing_hyphen_prefix =
            "hash0000000000000000000000000000000000000000000000000000000000000000";
        assert_eq!(
            Key::from_formatted_str(missing_hyphen_prefix)
                .unwrap_err()
                .to_string(),
            "unknown prefix for key"
        );

        let no_prefix = "0000000000000000000000000000000000000000000000000000000000000000";
        assert_eq!(
            Key::from_formatted_str(no_prefix).unwrap_err().to_string(),
            "unknown prefix for key"
        );
    }

    #[test]
    fn key_to_json() {
        for key in KEYS.iter() {
            assert_eq!(
                serde_json::to_string(key).unwrap(),
                format!("\"{}\"", key.to_formatted_string())
            );
        }
    }

    #[test]
    fn serialization_roundtrip_bincode() {
        for key in KEYS {
            let encoded = bincode::serialize(key).unwrap();
            let decoded = bincode::deserialize(&encoded).unwrap();
            assert_eq!(key, &decoded);
        }
    }

    #[test]
    fn key_tag_bytes_roundtrip() {
        for key in KEYS {
            let tag: KeyTag = key.tag();
            bytesrepr::test_serialization_roundtrip(&tag);
        }
    }

    #[test]
    fn serialization_roundtrip_json() {
        let round_trip = |key: &Key| {
            let encoded = serde_json::to_value(key).unwrap();
            let decoded = serde_json::from_value(encoded.clone())
                .unwrap_or_else(|_| panic!("{} {}", key, encoded));
            assert_eq!(key, &decoded);
        };

        for key in KEYS {
            round_trip(key);
        }

        let zeros = [0; BLAKE2B_DIGEST_LENGTH];
        let nines = [9; BLAKE2B_DIGEST_LENGTH];

        round_trip(&Key::Account(AccountHash::new(zeros)));
        round_trip(&Key::Hash(zeros));
        round_trip(&Key::URef(URef::new(zeros, AccessRights::READ)));
        round_trip(&Key::Transfer(TransferAddr::new(zeros)));
        round_trip(&Key::DeployInfo(DeployHash::from_raw(zeros)));
        round_trip(&Key::EraInfo(EraId::from(0)));
        round_trip(&Key::Balance(URef::new(zeros, AccessRights::READ).addr()));
        round_trip(&Key::Bid(AccountHash::new(zeros)));
        round_trip(&Key::BidAddr(BidAddr::legacy(zeros)));
        round_trip(&Key::BidAddr(BidAddr::new_validator_addr(zeros)));
        round_trip(&Key::BidAddr(BidAddr::new_delegator_addr((zeros, nines))));
        round_trip(&Key::Withdraw(AccountHash::new(zeros)));
        round_trip(&Key::Dictionary(zeros));
        round_trip(&Key::Unbond(AccountHash::new(zeros)));
        round_trip(&Key::Package(zeros));
        round_trip(&Key::AddressableEntity(PackageKindTag::System, zeros));
        round_trip(&Key::AddressableEntity(PackageKindTag::Account, zeros));
        round_trip(&Key::AddressableEntity(
            PackageKindTag::SmartContract,
            zeros,
        ));
        round_trip(&Key::ByteCode(ByteCodeKind::Empty, zeros));
        round_trip(&Key::ByteCode(ByteCodeKind::V1CasperWasm, zeros));
        round_trip(&Key::Message(MessageAddr::new_topic_addr(
            zeros.into(),
            nines.into(),
        )));
        round_trip(&Key::Message(MessageAddr::new_message_addr(
            zeros.into(),
            nines.into(),
            1,
        )));
    }
}
