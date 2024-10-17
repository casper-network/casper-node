use super::serialization::CalltableSerializationEnvelope;
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    account::AccountHash,
    bytesrepr::{
        Error::{self, Formatting},
        FromBytes, ToBytes,
    },
    transaction::serialization::CalltableSerializationEnvelopeBuilder,
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

const TAG_FIELD_INDEX: u16 = 0;

const PUBLIC_KEY_VARIANT_TAG: u8 = 0;
const PUBLIC_KEY_FIELD_INDEX: u16 = 1;

const ACCOUNT_HASH_VARIANT_TAG: u8 = 1;
const ACCOUNT_HASH_FIELD_INDEX: u16 = 1;

/// The address of the initiator of a [`crate::Transaction`].
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
            0 => InitiatorAddr::PublicKey(PublicKey::random(rng)),
            1 => InitiatorAddr::AccountHash(rng.gen()),
            _ => unreachable!(),
        }
    }

    fn serialized_field_lengths(&self) -> Vec<usize> {
        match self {
            InitiatorAddr::PublicKey(pub_key) => {
                vec![
                    crate::bytesrepr::U8_SERIALIZED_LENGTH,
                    pub_key.serialized_length(),
                ]
            }
            InitiatorAddr::AccountHash(hash) => {
                vec![
                    crate::bytesrepr::U8_SERIALIZED_LENGTH,
                    hash.serialized_length(),
                ]
            }
        }
    }
}

impl ToBytes for InitiatorAddr {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        match self {
            InitiatorAddr::PublicKey(pub_key) => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &PUBLIC_KEY_VARIANT_TAG)?
                    .add_field(PUBLIC_KEY_FIELD_INDEX, &pub_key)?
                    .binary_payload_bytes()
            }
            InitiatorAddr::AccountHash(hash) => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &ACCOUNT_HASH_VARIANT_TAG)?
                    .add_field(ACCOUNT_HASH_FIELD_INDEX, &hash)?
                    .binary_payload_bytes()
            }
        }
    }
    fn serialized_length(&self) -> usize {
        CalltableSerializationEnvelope::estimate_size(self.serialized_field_lengths())
    }
}

impl FromBytes for InitiatorAddr {
    fn from_bytes(bytes: &[u8]) -> Result<(InitiatorAddr, &[u8]), Error> {
        let (binary_payload, remainder) = CalltableSerializationEnvelope::from_bytes(2, bytes)?;
        let window = binary_payload.start_consuming()?.ok_or(Formatting)?;
        window.verify_index(TAG_FIELD_INDEX)?;
        let (tag, window) = window.deserialize_and_maybe_next::<u8>()?;
        let to_ret = match tag {
            PUBLIC_KEY_VARIANT_TAG => {
                let window = window.ok_or(Formatting)?;
                window.verify_index(PUBLIC_KEY_FIELD_INDEX)?;
                let (pub_key, window) = window.deserialize_and_maybe_next::<PublicKey>()?;
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(InitiatorAddr::PublicKey(pub_key))
            }
            ACCOUNT_HASH_VARIANT_TAG => {
                let window = window.ok_or(Formatting)?;
                window.verify_index(ACCOUNT_HASH_FIELD_INDEX)?;
                let (hash, window) = window.deserialize_and_maybe_next::<AccountHash>()?;
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(InitiatorAddr::AccountHash(hash))
            }
            _ => Err(Formatting),
        };
        to_ret.map(|endpoint| (endpoint, remainder))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bytesrepr, gens::initiator_addr_arb};
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
