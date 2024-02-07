use alloc::vec::Vec;
use core::fmt::{self, Debug, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use hex_fmt::HexFmt;
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
    serde_helpers, EntityAddr, PublicKey,
};

const PUBLIC_KEY_TAG: u8 = 0;
const ACCOUNT_HASH_TAG: u8 = 1;
const ENTITY_ADDR_TAG: u8 = 2;

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
    /// The entity address of the initiator.
    #[serde(with = "serde_helpers::raw_32_byte_array")]
    #[cfg_attr(
        feature = "json-schema",
        schemars(
            with = "String",
            description = "Hex-encoded entity address of the initiator."
        )
    )]
    EntityAddr(EntityAddr),
}

impl InitiatorAddr {
    /// Returns a random `InitiatorAddr`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..3) {
            PUBLIC_KEY_TAG => InitiatorAddr::PublicKey(PublicKey::random(rng)),
            ACCOUNT_HASH_TAG => InitiatorAddr::AccountHash(rng.gen()),
            ENTITY_ADDR_TAG => InitiatorAddr::EntityAddr(rng.gen()),
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
            InitiatorAddr::EntityAddr(entity_addr) => {
                write!(formatter, "entity-addr({:10})", HexFmt(entity_addr))
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
            InitiatorAddr::EntityAddr(entity_addr) => formatter
                .debug_tuple("EntityAddr")
                .field(&HexFmt(entity_addr))
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
            InitiatorAddr::EntityAddr(entity_addr) => {
                ENTITY_ADDR_TAG.write_bytes(writer)?;
                entity_addr.write_bytes(writer)
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
                InitiatorAddr::EntityAddr(entity_addr) => entity_addr.serialized_length(),
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
            ENTITY_ADDR_TAG => {
                let (entity_addr, remainder) = EntityAddr::from_bytes(remainder)?;
                Ok((InitiatorAddr::EntityAddr(entity_addr), remainder))
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
