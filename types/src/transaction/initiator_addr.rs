#[cfg(any(feature = "testing", test))]
use super::serialization::initiator_addr::{ACCOUNT_HASH_TAG, PUBLIC_KEY_TAG};
use super::serialization::{
    initiator_addr::{deserialize_initiator_addr, serialize_account_hash, serialize_public_key},
    serialized_length_for_field_sizes,
};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    account::AccountHash,
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    AsymmetricType, PublicKey,
};
use alloc::vec::Vec;
use core::fmt::{self, Debug, Display, Formatter};
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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

impl From<PublicKey> for InitiatorAddr {
    fn from(public_key: PublicKey) -> Self {
        InitiatorAddr::PublicKey(public_key)
    }
}

impl From<AccountHash> for InitiatorAddr {
    fn from(account_hash: AccountHash) -> Self {
        InitiatorAddr::AccountHash(account_hash)
    }
}

impl Display for InitiatorAddr {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            InitiatorAddr::PublicKey(public_key) => {
                write!(formatter, "public key {}", public_key.to_hex())
            }
            InitiatorAddr::AccountHash(account_hash) => {
                write!(formatter, "account hash {}", account_hash)
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
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        match self {
            InitiatorAddr::PublicKey(public_key) => serialize_public_key(public_key),
            InitiatorAddr::AccountHash(account_hash) => serialize_account_hash(account_hash),
        }
    }

    fn serialized_length(&self) -> usize {
        match self {
            InitiatorAddr::PublicKey(public_key) => serialized_length_for_field_sizes(vec![
                U8_SERIALIZED_LENGTH,
                public_key.serialized_length(),
            ]),
            InitiatorAddr::AccountHash(account_hash) => serialized_length_for_field_sizes(vec![
                U8_SERIALIZED_LENGTH,
                account_hash.serialized_length(),
            ]),
        }
    }
}

impl FromBytes for InitiatorAddr {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        deserialize_initiator_addr(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gens::initiator_addr_arb;
    use proptest::prelude::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        for _ in 0..10 {
            bytesrepr::test_serialization_roundtrip(&InitiatorAddr::random(rng));
        }
    }

    proptest! {
        #[test]
        fn generative_bytesrepr_roundtrip(val in initiator_addr_arb()) {
            bytesrepr::test_serialization_roundtrip(&val);
        }
    }
}
