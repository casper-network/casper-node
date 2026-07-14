use super::serialization::CalltableSerializationEnvelope;
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    account::AccountHash,
    bytesrepr::{
        Error::{self, Formatting},
        FromBytes, ToBytes,
    },
    evm::Address,
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

const EOA_VARIANT_TAG: u8 = 2;
const EOA_FIELD_INDEX: u16 = 1;

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
    /// An externally-owned Ethereum account address.
    Eoa(Address),
}

impl InitiatorAddr {
    /// Returns the Casper account hash carried by this initiator, if any.
    pub fn account_hash(&self) -> Option<AccountHash> {
        match self {
            InitiatorAddr::PublicKey(public_key) => Some(public_key.to_account_hash()),
            InitiatorAddr::AccountHash(hash) => Some(*hash),
            InitiatorAddr::Eoa(_) => None,
        }
    }

    /// Returns the EVM address carried by this initiator, if any.
    pub fn evm_address(&self) -> Option<Address> {
        match self {
            InitiatorAddr::Eoa(address) => Some(*address),
            InitiatorAddr::PublicKey(_) | InitiatorAddr::AccountHash(_) => None,
        }
    }

    /// Returns a random `InitiatorAddr`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..=2) {
            0 => InitiatorAddr::PublicKey(PublicKey::random(rng)),
            1 => InitiatorAddr::AccountHash(rng.gen()),
            2 => InitiatorAddr::Eoa(Address::new(rng.gen())),
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
            InitiatorAddr::Eoa(address) => {
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
            InitiatorAddr::Eoa(address) => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &EOA_VARIANT_TAG)?
                    .add_field(EOA_FIELD_INDEX, &address)?
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
            EOA_VARIANT_TAG => {
                let window = window.ok_or(Formatting)?;
                window.verify_index(EOA_FIELD_INDEX)?;
                let (address, window) = window.deserialize_and_maybe_next::<Address>()?;
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(InitiatorAddr::Eoa(address))
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

impl From<Address> for InitiatorAddr {
    fn from(address: Address) -> Self {
        InitiatorAddr::Eoa(address)
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
            InitiatorAddr::Eoa(address) => write!(formatter, "EOA {}", address),
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
            InitiatorAddr::Eoa(address) => formatter.debug_tuple("Eoa").field(address).finish(),
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

    #[test]
    fn variant_tags_are_stable() {
        let rng = &mut TestRng::new();
        let public_key = InitiatorAddr::PublicKey(PublicKey::random(rng));
        let account_hash = InitiatorAddr::AccountHash(AccountHash::new([1; 32]));
        let eoa = InitiatorAddr::Eoa(Address::new([2; crate::evm::ADDRESS_LENGTH]));

        assert_eq!(serialized_tag(&public_key), 0);
        assert_eq!(serialized_tag(&account_hash), 1);
        assert_eq!(serialized_tag(&eoa), 2);
    }

    #[test]
    fn eoa_accessors_do_not_fabricate_an_account_hash() {
        let address = Address::new([3; crate::evm::ADDRESS_LENGTH]);
        let initiator = InitiatorAddr::from(address);

        assert_eq!(initiator.account_hash(), None);
        assert_eq!(initiator.evm_address(), Some(address));
        assert_eq!(
            InitiatorAddr::AccountHash(AccountHash::new([4; 32])).evm_address(),
            None
        );
    }

    #[test]
    fn eoa_serde_roundtrip() {
        let initiator = InitiatorAddr::Eoa(Address::new([5; crate::evm::ADDRESS_LENGTH]));
        let json = serde_json::to_string(&initiator).expect("should serialize EOA initiator");
        assert_eq!(
            json,
            r#"{"Eoa":"0x0505050505050505050505050505050505050505"}"#
        );
        let decoded =
            serde_json::from_str::<InitiatorAddr>(&json).expect("should deserialize EOA initiator");

        assert_eq!(decoded, initiator);
    }

    fn serialized_tag(initiator: &InitiatorAddr) -> u8 {
        let bytes = initiator.to_bytes().expect("initiator should serialize");
        let (payload, remainder) =
            CalltableSerializationEnvelope::from_bytes(2, &bytes).expect("valid calltable");
        assert!(remainder.is_empty());
        let window = payload
            .start_consuming()
            .expect("valid fields")
            .expect("tag field");
        window.verify_index(TAG_FIELD_INDEX).expect("tag index");
        window
            .deserialize_and_maybe_next::<u8>()
            .expect("tag should deserialize")
            .0
    }

    proptest! {
        #[test]
        fn generative_bytesrepr_roundtrip(val in initiator_addr_arb()) {
            bytesrepr::test_serialization_roundtrip(&val);
        }
    }
}
