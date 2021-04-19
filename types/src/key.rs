use alloc::{format, string::String, vec::Vec};
use core::{
    array::TryFromSliceError,
    convert::TryFrom,
    fmt::{self, Debug, Display, Formatter},
    num::ParseIntError,
    str::FromStr,
};

use datasize::DataSize;
use hex_fmt::HexFmt;
use rand::{
    distributions::{Distribution, Standard},
    Rng,
};
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    account::{self, AccountHash, AccountHashBytes, TryFromSliceForAccountHashError},
    bytesrepr::{self, Error, FromBytes, ToBytes, U64_SERIALIZED_LENGTH},
    contract_wasm::ContractWasmHash,
    contracts::{ContractHash, ContractPackageHash},
    uref::{self, URef, URefAddr, UREF_SERIALIZED_LENGTH},
    DeployHash, EraId, Tagged, TransferAddr, DEPLOY_HASH_LENGTH, TRANSFER_ADDR_LENGTH,
    UREF_ADDR_LENGTH,
};

const HASH_PREFIX: &str = "hash-";
const DEPLOY_INFO_PREFIX: &str = "deploy-";
const ERA_INFO_PREFIX: &str = "era-";
const BALANCE_PREFIX: &str = "balance-";
const BID_PREFIX: &str = "bid-";
const WITHDRAW_PREFIX: &str = "withdraw-";

/// The number of bytes in a Blake2b hash
pub const BLAKE2B_DIGEST_LENGTH: usize = 32;
/// The number of bytes in a [`Key::Hash`].
pub const KEY_HASH_LENGTH: usize = 32;
/// The number of bytes in a [`Key::Transfer`].
pub const KEY_TRANSFER_LENGTH: usize = TRANSFER_ADDR_LENGTH;
/// The number of bytes in a [`Key::DeployInfo`].
pub const KEY_DEPLOY_INFO_LENGTH: usize = DEPLOY_HASH_LENGTH;

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

/// An alias for [`Key`]s hash variant.
pub type HashAddr = [u8; KEY_HASH_LENGTH];

impl From<HashAddr> for Key {
    fn from(addr: HashAddr) -> Self {
        Key::Hash(addr)
    }
}

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
}

/// The type under which data (e.g. [`CLValue`](crate::CLValue)s, smart contracts, user accounts)
/// are indexed on the network.
#[repr(C)]
#[derive(PartialEq, Eq, Clone, Copy, PartialOrd, Ord, Hash, DataSize)]
pub enum Key {
    /// A `Key` under which a user account is stored.
    Account(AccountHash),
    /// A `Key` under which a smart contract is stored and which is the pseudo-hash of the
    /// contract.
    Hash(HashAddr),
    /// A `Key` which is a [`URef`], under which most types of data can be stored.
    URef(URef),
    /// A `Key` under which we store a transfer.
    Transfer(TransferAddr),
    /// A `Key` under which we store a deploy info.
    DeployInfo(DeployHash),
    /// A `Key` under which we store an era info.
    EraInfo(EraId),
    /// A `Key` under which we store a purse balance.
    Balance(URefAddr),
    /// A `Key` under which we store bid information
    Bid(AccountHash),
    /// A `Key` under which we store unbond information.
    Withdraw(AccountHash),
}

#[derive(Debug)]
pub enum FromStrError {
    InvalidPrefix,
    Hex(base16::DecodeError),
    Account(TryFromSliceForAccountHashError),
    Hash(TryFromSliceError),
    AccountHash(account::FromStrError),
    URef(uref::FromStrError),
    EraId(ParseIntError),
}

impl From<base16::DecodeError> for FromStrError {
    fn from(error: base16::DecodeError) -> Self {
        FromStrError::Hex(error)
    }
}

impl From<TryFromSliceForAccountHashError> for FromStrError {
    fn from(error: TryFromSliceForAccountHashError) -> Self {
        FromStrError::Account(error)
    }
}

impl From<TryFromSliceError> for FromStrError {
    fn from(error: TryFromSliceError) -> Self {
        FromStrError::Hash(error)
    }
}

impl From<account::FromStrError> for FromStrError {
    fn from(error: account::FromStrError) -> Self {
        FromStrError::AccountHash(error)
    }
}

impl From<uref::FromStrError> for FromStrError {
    fn from(error: uref::FromStrError) -> Self {
        FromStrError::URef(error)
    }
}

impl From<ParseIntError> for FromStrError {
    fn from(error: ParseIntError) -> Self {
        FromStrError::EraId(error)
    }
}

impl Display for FromStrError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            FromStrError::InvalidPrefix => write!(f, "invalid prefix"),
            FromStrError::Hex(error) => write!(f, "decode from hex: {}", error),
            FromStrError::Account(error) => write!(f, "account from string error: {:?}", error),
            FromStrError::Hash(error) => write!(f, "hash from string error: {}", error),
            FromStrError::AccountHash(error) => {
                write!(f, "account hash from string error: {:?}", error)
            }
            FromStrError::URef(error) => write!(f, "uref from string error: {:?}", error),
            FromStrError::EraId(error) => write!(f, "era id from string error: {}", error),
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
        }
    }

    /// Returns the maximum size a [`Key`] can be serialized into.
    pub const fn max_serialized_length() -> usize {
        KEY_UREF_SERIALIZED_LENGTH
    }

    /// If `self` is of type [`Key::URef`], returns `self` with the
    /// [`AccessRights`](crate::AccessRights) stripped from the wrapped [`URef`], otherwise
    /// returns `self` unmodified.
    pub fn normalize(self) -> Key {
        match self {
            Key::URef(uref) => Key::URef(uref.remove_access_rights()),
            other => other,
        }
    }

    /// Returns a human-readable version of `self`, with the inner bytes encoded to Base16.
    pub fn to_formatted_string(&self) -> String {
        match self {
            Key::Account(account_hash) => account_hash.to_formatted_string(),
            Key::Hash(addr) => format!("{}{}", HASH_PREFIX, base16::encode_lower(addr)),
            Key::URef(uref) => uref.to_formatted_string(),
            Key::Transfer(transfer_addr) => transfer_addr.to_formatted_string(),
            Key::DeployInfo(addr) => {
                format!(
                    "{}{}",
                    DEPLOY_INFO_PREFIX,
                    base16::encode_lower(addr.as_bytes())
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
        }
    }

    /// Parses a string formatted as per `Self::to_formatted_string()` into a `Key`.
    pub fn from_formatted_str(input: &str) -> Result<Key, FromStrError> {
        if let Ok(account_hash) = AccountHash::from_formatted_str(input) {
            Ok(Key::Account(account_hash))
        } else if let Some(hex) = input.strip_prefix(HASH_PREFIX) {
            Ok(Key::Hash(HashAddr::try_from(
                base16::decode(hex)?.as_ref(),
            )?))
        } else if let Some(hex) = input.strip_prefix(DEPLOY_INFO_PREFIX) {
            Ok(Key::DeployInfo(DeployHash::new(
                <[u8; DEPLOY_HASH_LENGTH]>::try_from(base16::decode(hex)?.as_ref())?,
            )))
        } else if let Ok(transfer_addr) = TransferAddr::from_formatted_str(input) {
            Ok(Key::Transfer(transfer_addr))
        } else if let Ok(uref) = URef::from_formatted_str(input) {
            Ok(Key::URef(uref))
        } else if let Some(era_id_str) = input.strip_prefix(ERA_INFO_PREFIX) {
            Ok(Key::EraInfo(EraId::from_str(era_id_str)?))
        } else if let Some(hex) = input.strip_prefix(BALANCE_PREFIX) {
            Ok(Key::Balance(URefAddr::try_from(
                base16::decode(hex)?.as_ref(),
            )?))
        } else if let Some(hex) = input.strip_prefix(BID_PREFIX) {
            Ok(Key::Bid(AccountHash::new(AccountHashBytes::try_from(
                base16::decode(hex)?.as_ref(),
            )?)))
        } else if let Some(hex) = input.strip_prefix(WITHDRAW_PREFIX) {
            Ok(Key::Withdraw(AccountHash::new(AccountHashBytes::try_from(
                base16::decode(hex)?.as_ref(),
            )?)))
        } else {
            Err(FromStrError::InvalidPrefix)
        }
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
    pub fn into_hash(self) -> Option<HashAddr> {
        match self {
            Key::Hash(hash) => Some(hash),
            _ => None,
        }
    }

    /// Returns a reference to the inner [`URef`] if `self` is of type [`Key::URef`], otherwise
    /// returns `None`.
    pub fn as_uref(&self) -> Option<&URef> {
        match self {
            Key::URef(uref) => Some(uref),
            _ => None,
        }
    }

    /// Returns the inner [`URef`] if `self` is of type [`Key::URef`], otherwise returns `None`.
    pub fn into_uref(self) -> Option<URef> {
        match self {
            Key::URef(uref) => Some(uref),
            _ => None,
        }
    }

    /// Casts a [`Key::URef`] to a [`Key::Hash`]
    pub fn uref_to_hash(&self) -> Option<Key> {
        let uref = self.as_uref()?;
        let addr = uref.addr();
        Some(Key::Hash(addr))
    }
}

impl Display for Key {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Key::Account(account_hash) => write!(f, "Key::Account({})", account_hash),
            Key::Hash(addr) => write!(f, "Key::Hash({})", HexFmt(addr)),
            Key::URef(uref) => write!(f, "Key::{}", uref), /* Display impl for URef will append */
            Key::Transfer(transfer_addr) => write!(f, "Key::Transfer({})", transfer_addr),
            Key::DeployInfo(addr) => write!(f, "Key::DeployInfo({})", HexFmt(addr.as_bytes())),
            Key::EraInfo(era_id) => write!(f, "Key::EraInfo({})", era_id),
            Key::Balance(uref_addr) => write!(f, "Key::Balance({})", HexFmt(uref_addr)),
            Key::Bid(account_hash) => write!(f, "Key::Bid({})", account_hash),
            Key::Withdraw(account_hash) => write!(f, "Key::Withdraw({})", account_hash),
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

impl From<ContractHash> for Key {
    fn from(contract_hash: ContractHash) -> Key {
        Key::Hash(contract_hash.value())
    }
}

impl From<ContractWasmHash> for Key {
    fn from(wasm_hash: ContractWasmHash) -> Key {
        Key::Hash(wasm_hash.value())
    }
}

impl From<ContractPackageHash> for Key {
    fn from(package_hash: ContractPackageHash) -> Key {
        Key::Hash(package_hash.value())
    }
}

impl ToBytes for Key {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = bytesrepr::unchecked_allocate_buffer(self);
        result.push(self.tag());
        match self {
            Key::Account(account_hash) => {
                result.append(&mut account_hash.to_bytes()?);
            }
            Key::Hash(hash) => {
                result.append(&mut hash.to_bytes()?);
            }
            Key::URef(uref) => {
                result.append(&mut uref.to_bytes()?);
            }
            Key::Transfer(addr) => {
                result.append(&mut addr.to_bytes()?);
            }
            Key::DeployInfo(addr) => {
                result.append(&mut addr.to_bytes()?);
            }
            Key::EraInfo(era_id) => {
                result.append(&mut era_id.to_bytes()?);
            }
            Key::Balance(uref_addr) => {
                result.append(&mut uref_addr.to_bytes()?);
            }
            Key::Bid(account_hash) => {
                result.append(&mut account_hash.to_bytes()?);
            }
            Key::Withdraw(account_hash) => {
                result.append(&mut account_hash.to_bytes()?);
            }
        }
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
        }
    }
}

impl FromBytes for Key {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            tag if tag == KeyTag::Account as u8 => {
                let (account_hash, rem) = AccountHash::from_bytes(remainder)?;
                Ok((Key::Account(account_hash), rem))
            }
            tag if tag == KeyTag::Hash as u8 => {
                let (hash, rem) = FromBytes::from_bytes(remainder)?;
                Ok((Key::Hash(hash), rem))
            }
            tag if tag == KeyTag::URef as u8 => {
                let (uref, rem) = URef::from_bytes(remainder)?;
                Ok((Key::URef(uref), rem))
            }
            tag if tag == KeyTag::Transfer as u8 => {
                let (transfer_addr, rem) = TransferAddr::from_bytes(remainder)?;
                Ok((Key::Transfer(transfer_addr), rem))
            }
            tag if tag == KeyTag::DeployInfo as u8 => {
                let (deploy_hash, rem) = FromBytes::from_bytes(remainder)?;
                Ok((Key::DeployInfo(deploy_hash), rem))
            }
            tag if tag == KeyTag::EraInfo as u8 => {
                let (era_id, rem) = FromBytes::from_bytes(remainder)?;
                Ok((Key::EraInfo(era_id), rem))
            }
            tag if tag == KeyTag::Balance as u8 => {
                let (uref_addr, rem) = URefAddr::from_bytes(remainder)?;
                Ok((Key::Balance(uref_addr), rem))
            }
            tag if tag == KeyTag::Bid as u8 => {
                let (account_hash, rem) = AccountHash::from_bytes(remainder)?;
                Ok((Key::Bid(account_hash), rem))
            }
            tag if tag == KeyTag::Withdraw as u8 => {
                let (account_hash, rem) = AccountHash::from_bytes(remainder)?;
                Ok((Key::Withdraw(account_hash), rem))
            }
            _ => Err(Error::Formatting),
        }
    }
}

impl Distribution<Key> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Key {
        match rng.gen_range(0..8) {
            0 => Key::Account(rng.gen()),
            1 => Key::Hash(rng.gen()),
            2 => Key::URef(rng.gen()),
            3 => Key::Transfer(rng.gen()),
            4 => Key::DeployInfo(rng.gen()),
            5 => Key::EraInfo(rng.gen()),
            6 => Key::Balance(rng.gen()),
            7 => Key::Bid(rng.gen()),
            8 => Key::Withdraw(rng.gen()),
            _ => unreachable!(),
        }
    }
}

mod serde_helpers {
    use super::*;

    #[derive(Serialize, Deserialize)]
    pub(super) enum HumanReadable {
        Account(String),
        Hash(String),
        URef(String),
        Transfer(String),
        DeployInfo(String),
        EraInfo(String),
        Balance(String),
        Bid(String),
        Withdraw(String),
    }

    impl From<&Key> for HumanReadable {
        fn from(key: &Key) -> Self {
            let formatted_string = key.to_formatted_string();
            match key {
                Key::Account(_) => HumanReadable::Account(formatted_string),
                Key::Hash(_) => HumanReadable::Hash(formatted_string),
                Key::URef(_) => HumanReadable::URef(formatted_string),
                Key::Transfer(_) => HumanReadable::Transfer(formatted_string),
                Key::DeployInfo(_) => HumanReadable::DeployInfo(formatted_string),
                Key::EraInfo(_) => HumanReadable::EraInfo(formatted_string),
                Key::Balance(_) => HumanReadable::Balance(formatted_string),
                Key::Bid(_) => HumanReadable::Bid(formatted_string),
                Key::Withdraw(_) => HumanReadable::Withdraw(formatted_string),
            }
        }
    }

    impl TryFrom<HumanReadable> for Key {
        type Error = FromStrError;

        fn try_from(helper: HumanReadable) -> Result<Self, Self::Error> {
            match helper {
                HumanReadable::Account(formatted_string)
                | HumanReadable::Hash(formatted_string)
                | HumanReadable::URef(formatted_string)
                | HumanReadable::Transfer(formatted_string)
                | HumanReadable::DeployInfo(formatted_string)
                | HumanReadable::EraInfo(formatted_string)
                | HumanReadable::Balance(formatted_string)
                | HumanReadable::Bid(formatted_string)
                | HumanReadable::Withdraw(formatted_string) => {
                    Key::from_formatted_str(&formatted_string)
                }
            }
        }
    }

    #[derive(Serialize)]
    pub(super) enum BinarySerHelper<'a> {
        Account(&'a AccountHash),
        Hash(&'a HashAddr),
        URef(&'a URef),
        Transfer(&'a TransferAddr),
        DeployInfo(&'a DeployHash),
        EraInfo(&'a EraId),
        Balance(&'a URefAddr),
        Bid(&'a AccountHash),
        Withdraw(&'a AccountHash),
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
            }
        }
    }

    #[derive(Deserialize)]
    pub(super) enum BinaryDeserHelper {
        Account(AccountHash),
        Hash(HashAddr),
        URef(URef),
        Transfer(TransferAddr),
        DeployInfo(DeployHash),
        EraInfo(EraId),
        Balance(URefAddr),
        Bid(AccountHash),
        Withdraw(AccountHash),
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
            }
        }
    }
}

impl Serialize for Key {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serde_helpers::HumanReadable::from(self).serialize(serializer)
        } else {
            serde_helpers::BinarySerHelper::from(self).serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let human_readable = serde_helpers::HumanReadable::deserialize(deserializer)?;
            Key::try_from(human_readable).map_err(SerdeError::custom)
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
        bytesrepr::{Error, FromBytes},
        AccessRights, URef,
    };

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
        let expected_hash = core::iter::repeat("0").take(64).collect::<String>();
        let addr_array = [0u8; 32];
        let account_hash = AccountHash::new(addr_array);
        let account_key = Key::Account(account_hash);
        assert_eq!(
            format!("{}", account_key),
            format!("Key::Account({})", expected_hash)
        );
        let uref_key = Key::URef(URef::new(addr_array, AccessRights::READ));
        assert_eq!(
            format!("{}", uref_key),
            format!("Key::URef({}, READ)", expected_hash)
        );
        let hash_key = Key::Hash(addr_array);
        assert_eq!(
            format!("{}", hash_key),
            format!("Key::Hash({})", expected_hash)
        );
        let transfer_key = Key::Transfer(TransferAddr::new(addr_array));
        assert_eq!(
            format!("{}", transfer_key),
            format!("Key::Transfer({})", expected_hash)
        );
        let deploy_info_key = Key::DeployInfo(DeployHash::new(addr_array));
        assert_eq!(
            format!("{}", deploy_info_key),
            format!("Key::DeployInfo({})", expected_hash)
        );
        let era_info_key = Key::EraInfo(EraId::from(42));
        assert_eq!(
            format!("{}", era_info_key),
            "Key::EraInfo(era 42)".to_string()
        );
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
        assert!(key1.into_hash().is_none());
        assert!(key1.as_uref().is_none());
    }

    #[test]
    fn check_key_hash_getters() {
        let hash = [42; KEY_HASH_LENGTH];
        let key1 = Key::Hash(hash);
        assert!(key1.into_account().is_none());
        assert_eq!(key1.into_hash(), Some(hash));
        assert!(key1.as_uref().is_none());
    }

    #[test]
    fn check_key_uref_getters() {
        let uref = URef::new([42; 32], AccessRights::READ_ADD_WRITE);
        let key1 = Key::URef(uref);
        assert!(key1.into_account().is_none());
        assert!(key1.into_hash().is_none());
        assert_eq!(key1.as_uref(), Some(&uref));
    }

    #[test]
    fn key_max_serialized_length() {
        let key_account = Key::Account(AccountHash::new([42; BLAKE2B_DIGEST_LENGTH]));
        assert!(key_account.serialized_length() <= Key::max_serialized_length());

        let key_hash = Key::Hash([42; KEY_HASH_LENGTH]);
        assert!(key_hash.serialized_length() <= Key::max_serialized_length());

        let key_uref = Key::URef(URef::new([42; BLAKE2B_DIGEST_LENGTH], AccessRights::READ));
        assert!(key_uref.serialized_length() <= Key::max_serialized_length());

        let key_transfer = Key::Transfer(TransferAddr::new([42; BLAKE2B_DIGEST_LENGTH]));
        assert!(key_transfer.serialized_length() <= Key::max_serialized_length());

        let key_deploy_info = Key::DeployInfo(DeployHash::new([42; BLAKE2B_DIGEST_LENGTH]));
        assert!(key_deploy_info.serialized_length() <= Key::max_serialized_length());

        let key_era_info = Key::EraInfo(EraId::from(42));
        assert!(key_era_info.serialized_length() <= Key::max_serialized_length());
    }

    fn to_string_round_trip(key: Key) {
        let string = key.to_formatted_string();
        let parsed_key = Key::from_formatted_str(&string).unwrap();
        assert_eq!(key, parsed_key);
    }

    #[test]
    fn key_from_str() {
        to_string_round_trip(Key::Account(AccountHash::new([42; BLAKE2B_DIGEST_LENGTH])));
        to_string_round_trip(Key::Hash([42; KEY_HASH_LENGTH]));
        to_string_round_trip(Key::URef(URef::new(
            [255; BLAKE2B_DIGEST_LENGTH],
            AccessRights::READ,
        )));
        to_string_round_trip(Key::Transfer(TransferAddr::new([42; KEY_HASH_LENGTH])));
        to_string_round_trip(Key::DeployInfo(DeployHash::new([42; KEY_HASH_LENGTH])));
        to_string_round_trip(Key::EraInfo(EraId::from(42)));

        let invalid_prefix = "a-0000000000000000000000000000000000000000000000000000000000000000";
        assert!(Key::from_formatted_str(invalid_prefix).is_err());

        let invalid_prefix = "hash0000000000000000000000000000000000000000000000000000000000000000";
        assert!(Key::from_formatted_str(invalid_prefix).is_err());

        let short_addr = "00000000000000000000000000000000000000000000000000000000000000";
        assert!(Key::from_formatted_str(&format!("{}{}", HASH_PREFIX, short_addr)).is_err());

        let long_addr = "000000000000000000000000000000000000000000000000000000000000000000";
        assert!(Key::from_formatted_str(&format!("{}{}", HASH_PREFIX, long_addr)).is_err());

        let invalid_hex = "000000000000000000000000000000000000000000000000000000000000000g";
        assert!(Key::from_formatted_str(&format!("{}{}", HASH_PREFIX, invalid_hex)).is_err());
    }

    #[test]
    fn key_to_json() {
        let array = [42; BLAKE2B_DIGEST_LENGTH];
        let hex_bytes = "2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a";

        let key_account = Key::Account(AccountHash::new(array));
        assert_eq!(
            serde_json::to_string(&key_account).unwrap(),
            format!(r#"{{"Account":"account-hash-{}"}}"#, hex_bytes)
        );

        let key_hash = Key::Hash(array);
        assert_eq!(
            serde_json::to_string(&key_hash).unwrap(),
            format!(r#"{{"Hash":"hash-{}"}}"#, hex_bytes)
        );

        let key_uref = Key::URef(URef::new(array, AccessRights::READ));
        assert_eq!(
            serde_json::to_string(&key_uref).unwrap(),
            format!(r#"{{"URef":"uref-{}-001"}}"#, hex_bytes)
        );

        let key_transfer = Key::Transfer(TransferAddr::new(array));
        assert_eq!(
            serde_json::to_string(&key_transfer).unwrap(),
            format!(r#"{{"Transfer":"transfer-{}"}}"#, hex_bytes)
        );

        let key_deploy_info = Key::DeployInfo(DeployHash::new(array));
        assert_eq!(
            serde_json::to_string(&key_deploy_info).unwrap(),
            format!(r#"{{"DeployInfo":"deploy-{}"}}"#, hex_bytes)
        );

        let key_era_info = Key::EraInfo(EraId::from(42));
        assert_eq!(
            serde_json::to_string(&key_era_info).unwrap(),
            r#"{"EraInfo":"era-42"}"#.to_string()
        );
    }

    #[test]
    fn serialization_roundtrip_bincode() {
        let round_trip = |key: &Key| {
            let encoded = bincode::serialize(key).unwrap();
            let decoded = bincode::deserialize(&encoded).unwrap();
            assert_eq!(key, &decoded);
        };

        let array = [42; BLAKE2B_DIGEST_LENGTH];

        round_trip(&Key::Account(AccountHash::new(array)));
        round_trip(&Key::Hash(array));
        round_trip(&Key::URef(URef::new(array, AccessRights::READ)));
        round_trip(&Key::Transfer(TransferAddr::new(array)));
        round_trip(&Key::DeployInfo(DeployHash::new(array)));
        round_trip(&Key::EraInfo(EraId::from(42)));
        round_trip(&Key::Balance(URef::new(array, AccessRights::READ).addr()));
        round_trip(&Key::Bid(AccountHash::new(array)));
        round_trip(&Key::Withdraw(AccountHash::new(array)));
    }

    #[test]
    fn serialization_roundtrip_json() {
        let round_trip = |key: &Key| {
            let encoded = serde_json::to_string_pretty(key).unwrap();
            let decoded = serde_json::from_str(&encoded).unwrap();
            assert_eq!(key, &decoded);
        };

        let array = [42; BLAKE2B_DIGEST_LENGTH];

        round_trip(&Key::Account(AccountHash::new(array)));
        round_trip(&Key::Hash(array));
        round_trip(&Key::URef(URef::new(array, AccessRights::READ)));
        round_trip(&Key::Transfer(TransferAddr::new(array)));
        round_trip(&Key::DeployInfo(DeployHash::new(array)));
        round_trip(&Key::EraInfo(EraId::from(42)));
        round_trip(&Key::Balance(URef::new(array, AccessRights::READ).addr()));
        round_trip(&Key::Withdraw(AccountHash::new(array)));

        let zeros = [0; BLAKE2B_DIGEST_LENGTH];

        round_trip(&Key::Account(AccountHash::new(zeros)));
        round_trip(&Key::Hash(zeros));
        round_trip(&Key::URef(URef::new(zeros, AccessRights::READ)));
        round_trip(&Key::Transfer(TransferAddr::new(zeros)));
        round_trip(&Key::DeployInfo(DeployHash::new(zeros)));
        round_trip(&Key::EraInfo(EraId::from(0)));
        round_trip(&Key::Balance(URef::new(zeros, AccessRights::READ).addr()));
        round_trip(&Key::Bid(AccountHash::new(zeros)));
        round_trip(&Key::Withdraw(AccountHash::new(zeros)));
    }
}
