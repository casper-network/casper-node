use alloc::vec::Vec;
use core::fmt::{self, Debug, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
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
    account::AccountHash,
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    PublicKey,
};

const PUBLIC_KEY_TAG: u8 = 0;
const ACCOUNT_HASH_TAG: u8 = 1;

/// The address of the initiator of a [`TransactionV1`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "The address of the initiator of a TransactionV1.")
)]
#[serde(deny_unknown_fields)]
pub enum InitiatorAddr {
    /// The public key of the initiator.
    PublicKey(PublicKey),
    /// The account hash derived from the public key of the initiator.
    AccountHash(AccountHash),
}

impl InitiatorAddr {
    /// Gets the account hash.
    pub fn account_hash(&self) -> AccountHash {
        match self {
            InitiatorAddr::PublicKey(public_key) => public_key.to_account_hash(),
            InitiatorAddr::AccountHash(hash) => *hash,
        }
    }

    /// Returns a random `InitiatorAddr`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..=1) {
            PUBLIC_KEY_TAG => InitiatorAddr::PublicKey(PublicKey::random(rng)),
            ACCOUNT_HASH_TAG => InitiatorAddr::AccountHash(rng.gen()),
            _ => unreachable!(),
        }
    }
}

impl Display for InitiatorAddr {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            InitiatorAddr::PublicKey(public_key) => write!(formatter, "{}", public_key),
            InitiatorAddr::AccountHash(account_hash) => {
                write!(formatter, "account-hash({})", account_hash)
            }
        }
    }
}

impl Debug for InitiatorAddr {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            InitiatorAddr::PublicKey(public_key) => formatter
                .debug_tuple("PublicKey")
                .field(public_key)
                .finish(),
            InitiatorAddr::AccountHash(account_hash) => formatter
                .debug_tuple("AccountHash")
                .field(account_hash)
                .finish(),
        }
    }
}

impl ToBytes for InitiatorAddr {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            InitiatorAddr::PublicKey(public_key) => {
                PUBLIC_KEY_TAG.write_bytes(writer)?;
                public_key.write_bytes(writer)
            }
            InitiatorAddr::AccountHash(account_hash) => {
                ACCOUNT_HASH_TAG.write_bytes(writer)?;
                account_hash.write_bytes(writer)
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
                InitiatorAddr::PublicKey(public_key) => public_key.serialized_length(),
                InitiatorAddr::AccountHash(account_hash) => account_hash.serialized_length(),
            }
    }
}

impl FromBytes for InitiatorAddr {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            PUBLIC_KEY_TAG => {
                let (public_key, remainder) = PublicKey::from_bytes(remainder)?;
                Ok((InitiatorAddr::PublicKey(public_key), remainder))
            }
            ACCOUNT_HASH_TAG => {
                let (account_hash, remainder) = AccountHash::from_bytes(remainder)?;
                Ok((InitiatorAddr::AccountHash(account_hash), remainder))
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
            bytesrepr::test_serialization_roundtrip(&InitiatorAddr::random(rng));
        }
    }
}
