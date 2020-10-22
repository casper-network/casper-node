use alloc::{format, string::String, vec::Vec};
use core::{
    array::TryFromSliceError,
    convert::TryFrom,
    fmt::{self, Debug, Display, Formatter},
};

use hex_fmt::HexFmt;

use crate::{
    account::{self, AccountHash, TryFromSliceForAccountHashError},
    bytesrepr::{self, Error, FromBytes, ToBytes},
    uref::{self, URef, UREF_SERIALIZED_LENGTH},
    DeployHash, DEPLOY_HASH_LENGTH,
};

const ACCOUNT_ID: u8 = 0;
const HASH_ID: u8 = 1;
const UREF_ID: u8 = 2;
const TRANSFER_ID: u8 = 3;
const DEPLOY_INFO_ID: u8 = 4;

const HASH_PREFIX: &str = "hash-";
const TRANSFER_PREFIX: &str = "transfer-";
const DEPLOY_INFO_PREFIX: &str = "deploy-";

/// The number of bytes in a Blake2b hash
pub const BLAKE2B_DIGEST_LENGTH: usize = 32;
/// The number of bytes in a [`Key::Hash`].
pub const KEY_HASH_LENGTH: usize = 32;
/// The number of bytes in a [`Key::Transfer`].
pub const KEY_TRANSFER_LENGTH: usize = 32;
/// The number of bytes in a [`Key::DeployInfo`].
pub const KEY_DEPLOY_INFO_LENGTH: usize = DEPLOY_HASH_LENGTH;

const KEY_ID_SERIALIZED_LENGTH: usize = 1;
// u8 used to determine the ID
const KEY_HASH_SERIALIZED_LENGTH: usize = KEY_ID_SERIALIZED_LENGTH + KEY_HASH_LENGTH;
const KEY_UREF_SERIALIZED_LENGTH: usize = KEY_ID_SERIALIZED_LENGTH + UREF_SERIALIZED_LENGTH;
const KEY_TRANSFER_SERIALIZED_LENGTH: usize = KEY_ID_SERIALIZED_LENGTH + KEY_TRANSFER_LENGTH;
const KEY_DEPLOY_INFO_SERIALIZED_LENGTH: usize = KEY_ID_SERIALIZED_LENGTH + KEY_DEPLOY_INFO_LENGTH;

/// An alias for [`Key`]s hash variant.
pub type HashAddr = [u8; KEY_HASH_LENGTH];

impl From<HashAddr> for Key {
    fn from(addr: HashAddr) -> Self {
        Key::Hash(addr)
    }
}

/// An alias for [`Key`]s hash variant.
pub type ContractHash = HashAddr;
/// An alias for [`Key`]s hash variant.
pub type ContractWasmHash = HashAddr;
/// An alias for [`Key`]s hash variant.
pub type ContractPackageHash = HashAddr;
/// An alias for [`Key`]s transfer variant.
pub type TransferAddr = HashAddr;

/// The type under which data (e.g. [`CLValue`](crate::CLValue)s, smart contracts, user accounts)
/// are indexed on the network.
#[repr(C)]
#[derive(PartialEq, Eq, Clone, Copy, PartialOrd, Ord, Hash)]
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
}

#[derive(Debug)]
pub enum FromStrError {
    InvalidPrefix,
    Hex(base16::DecodeError),
    Account(TryFromSliceForAccountHashError),
    Hash(TryFromSliceError),
    AccountHash(account::FromStrError),
    URef(uref::FromStrError),
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
            Key::Transfer(addr) => format!("{}{}", TRANSFER_PREFIX, base16::encode_lower(addr)),
            Key::DeployInfo(addr) => {
                format!("{}{}", DEPLOY_INFO_PREFIX, base16::encode_lower(addr))
            }
        }
    }

    /// Parses a string formatted as per `Self::as_string()` into a `Key`.
    pub fn from_formatted_str(input: &str) -> Result<Key, FromStrError> {
        if let Ok(account_hash) = AccountHash::from_formatted_str(input) {
            Ok(Key::Account(account_hash))
        } else if let Some(hex) = input.strip_prefix(HASH_PREFIX) {
            Ok(Key::Hash(HashAddr::try_from(
                base16::decode(hex)?.as_ref(),
            )?))
        } else if let Some(hex) = input.strip_prefix(DEPLOY_INFO_PREFIX) {
            Ok(Key::DeployInfo(DeployHash::try_from(
                base16::decode(hex)?.as_ref(),
            )?))
        } else if let Some(hex) = input.strip_prefix(TRANSFER_PREFIX) {
            Ok(Key::Transfer(TransferAddr::try_from(
                base16::decode(hex)?.as_ref(),
            )?))
        } else {
            Ok(Key::URef(URef::from_formatted_str(input)?))
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

    /// Creates the seed of a local key for a context with the given base key.
    pub fn into_seed(self) -> [u8; BLAKE2B_DIGEST_LENGTH] {
        match self {
            Key::Account(account_hash) => account_hash.value(),
            Key::Hash(bytes) => bytes,
            Key::URef(uref) => uref.addr(),
            Key::Transfer(addr) => addr,
            Key::DeployInfo(addr) => addr,
        }
    }
}

impl Display for Key {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Key::Account(account_hash) => write!(f, "Key::Account({})", account_hash),
            Key::Hash(addr) => write!(f, "Key::Hash({})", HexFmt(addr)),
            Key::URef(uref) => write!(f, "Key::{}", uref), /* Display impl for URef will append */
            Key::Transfer(addr) => write!(f, "Key::Transfer({})", HexFmt(addr)),
            Key::DeployInfo(addr) => write!(f, "Key::DeployInfo({})", HexFmt(addr)),
        }
    }
}

impl Debug for Key {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", self)
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

impl ToBytes for Key {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = bytesrepr::unchecked_allocate_buffer(self);
        match self {
            Key::Account(account_hash) => {
                result.push(ACCOUNT_ID);
                result.append(&mut account_hash.to_bytes()?);
            }
            Key::Hash(hash) => {
                result.push(HASH_ID);
                result.append(&mut hash.to_bytes()?);
            }
            Key::URef(uref) => {
                result.push(UREF_ID);
                result.append(&mut uref.to_bytes()?);
            }
            Key::Transfer(addr) => {
                result.push(TRANSFER_ID);
                result.append(&mut addr.to_bytes()?);
            }
            Key::DeployInfo(addr) => {
                result.push(DEPLOY_INFO_ID);
                result.append(&mut addr.to_bytes()?);
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
        }
    }
}

impl FromBytes for Key {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (id, remainder) = u8::from_bytes(bytes)?;
        match id {
            ACCOUNT_ID => {
                let (account_hash, rem) = AccountHash::from_bytes(remainder)?;
                Ok((Key::Account(account_hash), rem))
            }
            HASH_ID => {
                let (hash, rem) = <[u8; KEY_HASH_LENGTH]>::from_bytes(remainder)?;
                Ok((Key::Hash(hash), rem))
            }
            UREF_ID => {
                let (uref, rem) = URef::from_bytes(remainder)?;
                Ok((Key::URef(uref), rem))
            }
            TRANSFER_ID => {
                let (transfer_addr, rem) = TransferAddr::from_bytes(remainder)?;
                Ok((Key::Transfer(transfer_addr), rem))
            }
            DEPLOY_INFO_ID => {
                let (deploy_hash, rem) = DeployHash::from_bytes(remainder)?;
                Ok((Key::DeployInfo(deploy_hash), rem))
            }
            _ => Err(Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
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
    }

    fn round_trip(key: Key) {
        let string = key.to_formatted_string();
        let parsed_key = Key::from_formatted_str(&string).unwrap();
        assert_eq!(key, parsed_key);
    }

    #[test]
    fn key_from_str() {
        round_trip(Key::Account(AccountHash::new([0; BLAKE2B_DIGEST_LENGTH])));
        round_trip(Key::Hash([42; KEY_HASH_LENGTH]));
        round_trip(Key::URef(URef::new(
            [255; BLAKE2B_DIGEST_LENGTH],
            AccessRights::READ,
        )));

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
}
