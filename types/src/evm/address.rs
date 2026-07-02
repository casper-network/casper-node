use alloc::{string::String, vec::Vec};
use core::{
    convert::TryFrom,
    fmt::{self, Display, Formatter},
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use alloy_primitives::keccak256;

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    CLType, CLTyped, PublicKey,
};

/// The number of bytes in an EVM address.
pub const ADDRESS_LENGTH: usize = 20;

const ADDRESS_SERIALIZED_LENGTH: usize = ADDRESS_LENGTH;

/// A 20-byte Ethereum account or contract address.
#[derive(
    Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize,
)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct Address([u8; ADDRESS_LENGTH]);

impl Address {
    /// The zero EVM address.
    pub const ZERO: Address = Address([0; ADDRESS_LENGTH]);

    /// Creates an address from raw bytes.
    pub const fn new(bytes: [u8; ADDRESS_LENGTH]) -> Self {
        Address(bytes)
    }

    /// Returns the raw bytes backing this address.
    pub const fn value(self) -> [u8; ADDRESS_LENGTH] {
        self.0
    }

    /// Returns the raw bytes backing this address by reference.
    pub const fn as_bytes(&self) -> &[u8; ADDRESS_LENGTH] {
        &self.0
    }

    /// Returns a lower-case hexadecimal string without a `0x` prefix.
    pub fn to_hex_string(self) -> String {
        base16::encode_lower(&self.0)
    }

    /// Returns the Ethereum address for a secp256k1 public key.
    ///
    /// Ethereum addresses are the low 20 bytes of the Keccak-256 hash of the
    /// uncompressed secp256k1 public key without its SEC1 prefix byte.
    /// Non-secp256k1 Casper keys do not have an EVM-native address.
    pub fn from_public_key(public_key: &PublicKey) -> Option<Self> {
        let PublicKey::Secp256k1(public_key) = public_key else {
            return None;
        };
        let encoded = public_key.to_encoded_point(false);
        let bytes = encoded.as_bytes();
        let digest = keccak256(&bytes[1..]);
        let mut address = [0u8; ADDRESS_LENGTH];
        address.copy_from_slice(&digest.as_slice()[digest.len() - ADDRESS_LENGTH..]);
        Some(Address::new(address))
    }

    /// Returns the EVM `block.coinbase` receive address for a Casper block proposer.
    ///
    /// Secp256k1 proposers use their Ethereum-native address. Non-secp256k1
    /// proposers use a Casper-defined alias derived from the last 20 bytes of
    /// their account hash. That alias is only a receive address for proposer
    /// rewards and `block.coinbase` transfers; it is not controlled by an
    /// Ethereum signing key and cannot be used to sign EVM transactions.
    pub fn from_block_proposer_public_key(public_key: &PublicKey) -> Self {
        if let Some(address) = Self::from_public_key(public_key) {
            return address;
        }

        let account_hash = public_key.to_account_hash();
        let mut address = [0u8; ADDRESS_LENGTH];
        address.copy_from_slice(
            &account_hash.as_bytes()[account_hash.as_bytes().len() - ADDRESS_LENGTH..],
        );
        Address::new(address)
    }
}

impl AsRef<[u8]> for Address {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Display for Address {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "0x{}", self.to_hex_string())
    }
}

#[cfg(feature = "json-schema")]
impl JsonSchema for Address {
    fn schema_name() -> String {
        String::from("Address")
    }

    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        let schema = gen.subschema_for::<String>();
        let mut schema_object = schema.into_object();
        schema_object.metadata().description =
            Some("A 20-byte Ethereum account or contract address encoded as hexadecimal.".into());
        schema_object.into()
    }
}

impl CLTyped for Address {
    fn cl_type() -> CLType {
        CLType::ByteArray(ADDRESS_LENGTH as u32)
    }
}

impl ToBytes for Address {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        Ok(self.0.to_vec())
    }

    fn serialized_length(&self) -> usize {
        ADDRESS_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.extend_from_slice(&self.0);
        Ok(())
    }
}

impl FromBytes for Address {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        if bytes.len() < ADDRESS_LENGTH {
            return Err(bytesrepr::Error::EarlyEndOfStream);
        }
        let (address, remainder) = bytes.split_at(ADDRESS_LENGTH);
        let address =
            <[u8; ADDRESS_LENGTH]>::try_from(address).map_err(|_| bytesrepr::Error::Formatting)?;
        Ok((Address(address), remainder))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CLValue, SecretKey};

    #[test]
    fn evm_address_cl_value_roundtrip() {
        let address = Address::new([0x11; ADDRESS_LENGTH]);
        let cl_value = CLValue::from_t(address).expect("address should serialize");

        assert_eq!(
            cl_value.cl_type(),
            &CLType::ByteArray(ADDRESS_LENGTH as u32)
        );
        assert_eq!(
            cl_value
                .to_t::<Address>()
                .expect("address should deserialize"),
            address
        );
    }

    #[test]
    fn block_proposer_address_preserves_secp256k1_ethereum_address() {
        let secret_key = SecretKey::secp256k1_from_bytes([7; SecretKey::SECP256K1_LENGTH]).unwrap();
        let public_key = PublicKey::from(&secret_key);

        assert_eq!(
            Address::from_block_proposer_public_key(&public_key),
            Address::from_public_key(&public_key).unwrap()
        );
    }

    #[test]
    fn block_proposer_address_uses_last_twenty_account_hash_bytes_for_ed25519() {
        let secret_key = SecretKey::ed25519_from_bytes([9; SecretKey::ED25519_LENGTH]).unwrap();
        let public_key = PublicKey::from(&secret_key);
        let account_hash = public_key.to_account_hash();
        let mut expected = [0u8; ADDRESS_LENGTH];
        expected.copy_from_slice(&account_hash.as_bytes()[12..]);

        assert_eq!(
            Address::from_block_proposer_public_key(&public_key),
            Address::new(expected)
        );
        assert!(Address::from_public_key(&public_key).is_none());
    }

    #[test]
    fn block_proposer_address_is_total_for_system_public_key() {
        let address = Address::from_block_proposer_public_key(&PublicKey::System);
        let account_hash = PublicKey::System.to_account_hash();
        let mut expected = [0u8; ADDRESS_LENGTH];
        expected.copy_from_slice(&account_hash.as_bytes()[12..]);

        assert_eq!(address, Address::new(expected));
    }
}
