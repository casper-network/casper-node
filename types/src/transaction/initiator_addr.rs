use super::serialization::CalltableSerializationEnvelope;
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    account::AccountHash,
    bytesrepr::{
        Error::{self, Formatting},
        FromBytes, ToBytes,
    },
    evm,
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

const EVM_ADDRESS_VARIANT_TAG: u8 = 2;
const EVM_ADDRESS_FIELD_INDEX: u16 = 1;

/// The address of the initiator of a [`crate::Transaction`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "The address of the initiator of a transaction.")
)]
#[serde(deny_unknown_fields)]
pub enum InitiatorAddr {
    /// The public key of the initiator.
    PublicKey(PublicKey),
    /// The account hash derived from the public key of the initiator.
    AccountHash(AccountHash),
    /// The EVM-native address recovered from the signed Ethereum transaction.
    EvmAddress(evm::Address),
}

impl InitiatorAddr {
    /// Returns the Casper account hash, if this initiator has one.
    ///
    /// EVM initiators do not have a native Casper account hash. EVM-aware code
    /// should use [`InitiatorAddr::evm_address`] or
    /// [`crate::Transaction::evm_initiator_addr`].
    pub fn account_hash(&self) -> Option<AccountHash> {
        match self {
            InitiatorAddr::PublicKey(public_key) => Some(public_key.to_account_hash()),
            InitiatorAddr::AccountHash(hash) => Some(*hash),
            InitiatorAddr::EvmAddress(_) => None,
        }
    }

    /// Returns the native EVM address if this is an EVM initiator.
    pub fn evm_address(&self) -> Option<evm::Address> {
        match self {
            InitiatorAddr::EvmAddress(address) => Some(*address),
            InitiatorAddr::PublicKey(_) | InitiatorAddr::AccountHash(_) => None,
        }
    }

    /// Returns a random `InitiatorAddr`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..=2) {
            0 => InitiatorAddr::PublicKey(PublicKey::random(rng)),
            1 => InitiatorAddr::AccountHash(rng.gen()),
            2 => InitiatorAddr::EvmAddress(evm::Address::new(rng.gen())),
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
            InitiatorAddr::EvmAddress(address) => {
                vec![
                    crate::bytesrepr::U8_SERIALIZED_LENGTH,
                    address.serialized_length(),
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
            InitiatorAddr::EvmAddress(address) => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &EVM_ADDRESS_VARIANT_TAG)?
                    .add_field(EVM_ADDRESS_FIELD_INDEX, &address)?
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
            EVM_ADDRESS_VARIANT_TAG => {
                let window = window.ok_or(Formatting)?;
                window.verify_index(EVM_ADDRESS_FIELD_INDEX)?;
                let (address, window) = window.deserialize_and_maybe_next::<evm::Address>()?;
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(InitiatorAddr::EvmAddress(address))
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

impl From<evm::Address> for InitiatorAddr {
    fn from(address: evm::Address) -> Self {
        InitiatorAddr::EvmAddress(address)
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
            InitiatorAddr::EvmAddress(address) => {
                write!(formatter, "EVM address {}", address)
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
            InitiatorAddr::EvmAddress(address) => {
                formatter.debug_tuple("EvmAddress").field(address).finish()
            }
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
