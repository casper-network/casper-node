use crate::{
    bytesrepr,
    bytesrepr::{FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    checksummed_hex, CLType, CLTyped, PublicKey, URef, URefAddr,
};
use alloc::{string::String, vec::Vec};
use core::{
    fmt,
    fmt::{Display, Formatter},
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
use serde_helpers::{HumanReadableDelegatorKind, NonHumanReadableDelegatorKind};

/// DelegatorKindTag variants.
#[repr(u8)]
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, Clone)]
pub enum DelegatorKindTag {
    /// Public key.
    PublicKey = 0,
    /// Purse.
    Purse = 1,
}

/// Auction bid variants.
#[derive(Debug, PartialEq, Eq, Clone, PartialOrd, Ord)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
/// Kinds of delegation bids.
pub enum DelegatorKind {
    /// Delegation from public key.
    PublicKey(PublicKey),
    /// Delegation from purse.
    Purse(#[cfg_attr(feature = "json-schema", schemars(with = "String"))] URefAddr),
}

impl DelegatorKind {
    /// DelegatorKindTag.
    pub fn tag(&self) -> DelegatorKindTag {
        match self {
            DelegatorKind::PublicKey(_) => DelegatorKindTag::PublicKey,
            DelegatorKind::Purse(_) => DelegatorKindTag::Purse,
        }
    }

    /// Returns true if the kind is a purse.
    pub fn is_purse(&self) -> bool {
        matches!(self, DelegatorKind::Purse(_))
    }

    /// Returns public key if kind is a public key.
    pub fn maybe_public_key(&self) -> Option<PublicKey> {
        match self {
            DelegatorKind::PublicKey(ret) => Some(ret.clone()),
            DelegatorKind::Purse(_) => None,
        }
    }
}

impl ToBytes for DelegatorKind {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        let (tag, mut serialized_data) = match self {
            DelegatorKind::PublicKey(public_key) => {
                (DelegatorKindTag::PublicKey, public_key.to_bytes()?)
            }
            DelegatorKind::Purse(uref_addr) => (DelegatorKindTag::Purse, uref_addr.to_bytes()?),
        };
        result.push(tag as u8);
        result.append(&mut serialized_data);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                DelegatorKind::PublicKey(pk) => pk.serialized_length(),
                DelegatorKind::Purse(addr) => addr.serialized_length(),
            }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        writer.push(self.tag() as u8);
        match self {
            DelegatorKind::PublicKey(pk) => pk.write_bytes(writer)?,
            DelegatorKind::Purse(addr) => addr.write_bytes(writer)?,
        };
        Ok(())
    }
}

impl FromBytes for DelegatorKind {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder): (u8, &[u8]) = FromBytes::from_bytes(bytes)?;
        match tag {
            tag if tag == DelegatorKindTag::PublicKey as u8 => PublicKey::from_bytes(remainder)
                .map(|(pk, remainder)| (DelegatorKind::PublicKey(pk), remainder)),
            tag if tag == DelegatorKindTag::Purse as u8 => URefAddr::from_bytes(remainder)
                .map(|(addr, remainder)| (DelegatorKind::Purse(addr), remainder)),
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

impl Display for DelegatorKind {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            DelegatorKind::PublicKey(pk) => {
                write!(formatter, "{}", pk)
            }
            DelegatorKind::Purse(addr) => {
                write!(formatter, "{}", base16::encode_lower(addr))
            }
        }
    }
}

impl From<PublicKey> for DelegatorKind {
    fn from(value: PublicKey) -> Self {
        DelegatorKind::PublicKey(value)
    }
}

impl From<&PublicKey> for DelegatorKind {
    fn from(value: &PublicKey) -> Self {
        DelegatorKind::PublicKey(value.clone())
    }
}

impl From<URef> for DelegatorKind {
    fn from(value: URef) -> Self {
        DelegatorKind::Purse(value.addr())
    }
}

impl From<URefAddr> for DelegatorKind {
    fn from(value: URefAddr) -> Self {
        DelegatorKind::Purse(value)
    }
}

impl CLTyped for DelegatorKind {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<DelegatorKind> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> DelegatorKind {
        if rng.gen() {
            DelegatorKind::PublicKey(rng.gen())
        } else {
            DelegatorKind::Purse(rng.gen())
        }
    }
}

impl Serialize for DelegatorKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            HumanReadableDelegatorKind::from(self).serialize(serializer)
        } else {
            NonHumanReadableDelegatorKind::from(self).serialize(serializer)
        }
    }
}

#[derive(Debug)]
enum DelegatorKindError {
    DeserializationError(String),
}

impl Display for DelegatorKindError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            DelegatorKindError::DeserializationError(error) => {
                write!(f, "Error when deserializing DelegatorKind: {}", error)
            }
        }
    }
}

impl TryFrom<HumanReadableDelegatorKind> for DelegatorKind {
    type Error = DelegatorKindError;

    fn try_from(value: HumanReadableDelegatorKind) -> Result<Self, Self::Error> {
        match value {
            HumanReadableDelegatorKind::PublicKey(public_key) => {
                Ok(DelegatorKind::PublicKey(public_key))
            }
            HumanReadableDelegatorKind::Purse(encoded) => {
                let decoded = checksummed_hex::decode(encoded).map_err(|e| {
                    DelegatorKindError::DeserializationError(format!(
                        "Failed to decode encoded URefAddr: {}",
                        e
                    ))
                })?;
                let uref_addr = URefAddr::try_from(decoded.as_ref()).map_err(|e| {
                    DelegatorKindError::DeserializationError(format!(
                        "Failed to build uref address: {}",
                        e
                    ))
                })?;
                Ok(DelegatorKind::Purse(uref_addr))
            }
        }
    }
}

impl From<NonHumanReadableDelegatorKind> for DelegatorKind {
    fn from(value: NonHumanReadableDelegatorKind) -> Self {
        match value {
            NonHumanReadableDelegatorKind::PublicKey(public_key) => {
                DelegatorKind::PublicKey(public_key)
            }
            NonHumanReadableDelegatorKind::Purse(addr) => DelegatorKind::Purse(addr),
        }
    }
}

impl<'de> Deserialize<'de> for DelegatorKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let human_readable = HumanReadableDelegatorKind::deserialize(deserializer)?;
            DelegatorKind::try_from(human_readable)
                .map_err(|error| SerdeError::custom(format!("{:?}", error)))
        } else {
            let non_human_readable = NonHumanReadableDelegatorKind::deserialize(deserializer)?;
            Ok(DelegatorKind::from(non_human_readable))
        }
    }
}

mod serde_helpers {
    use super::DelegatorKind;
    use crate::{PublicKey, URefAddr};
    use alloc::string::String;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    pub(super) enum HumanReadableDelegatorKind {
        PublicKey(PublicKey),
        Purse(String),
    }

    #[derive(Serialize, Deserialize)]
    pub(super) enum NonHumanReadableDelegatorKind {
        PublicKey(PublicKey),
        Purse(URefAddr),
    }

    impl From<&DelegatorKind> for HumanReadableDelegatorKind {
        fn from(delegator_kind: &DelegatorKind) -> Self {
            match delegator_kind {
                DelegatorKind::PublicKey(public_key) => {
                    HumanReadableDelegatorKind::PublicKey(public_key.clone())
                }
                DelegatorKind::Purse(uref_addr) => {
                    HumanReadableDelegatorKind::Purse(base16::encode_lower(uref_addr))
                }
            }
        }
    }

    impl From<&DelegatorKind> for NonHumanReadableDelegatorKind {
        fn from(delegator_kind: &DelegatorKind) -> Self {
            match delegator_kind {
                DelegatorKind::PublicKey(public_key) => {
                    NonHumanReadableDelegatorKind::PublicKey(public_key.clone())
                }
                DelegatorKind::Purse(uref_addr) => NonHumanReadableDelegatorKind::Purse(*uref_addr),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use rand::Rng;

    use crate::{
        bytesrepr, system::auction::delegator_kind::DelegatorKind, testing::TestRng, PublicKey,
        SecretKey,
    };

    #[test]
    fn purse_serialized_as_string() {
        let delegator_kind_payload = DelegatorKind::Purse([1; 32]);
        let serialized = serde_json::to_string(&delegator_kind_payload).unwrap();
        assert_eq!(
            serialized,
            "{\"Purse\":\"0101010101010101010101010101010101010101010101010101010101010101\"}"
        );
    }

    #[test]
    fn given_broken_address_purse_deserialziation_fails() {
        let failing =
            "{\"Purse\":\"Z101010101010101010101010101010101010101010101010101010101010101\"}";
        let ret = serde_json::from_str::<DelegatorKind>(failing);
        assert!(ret.is_err());
        let failing = "{\"Purse\":\"01010101010101010101010101010101010101010101010101010101\"}";
        let ret = serde_json::from_str::<DelegatorKind>(failing);
        assert!(ret.is_err());
    }

    #[test]
    fn json_roundtrip() {
        let rng = &mut TestRng::new();

        let delegator_kind_payload = DelegatorKind::PublicKey(PublicKey::random(rng));
        let json_string = serde_json::to_string_pretty(&delegator_kind_payload).unwrap();
        let decoded: DelegatorKind = serde_json::from_str(&json_string).unwrap();
        assert_eq!(decoded, delegator_kind_payload);

        let delegator_kind_payload = DelegatorKind::Purse(rng.gen());
        let json_string = serde_json::to_string_pretty(&delegator_kind_payload).unwrap();
        let decoded: DelegatorKind = serde_json::from_str(&json_string).unwrap();
        assert_eq!(decoded, delegator_kind_payload);
    }

    #[test]
    fn serialization_roundtrip() {
        let delegator_kind = DelegatorKind::PublicKey(PublicKey::from(
            &SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH]).unwrap(),
        ));

        bytesrepr::test_serialization_roundtrip(&delegator_kind);

        let delegator_kind = DelegatorKind::Purse([43; 32]);

        bytesrepr::test_serialization_roundtrip(&delegator_kind);
    }
}

#[cfg(test)]
mod prop_tests {
    use proptest::prelude::*;

    use crate::{bytesrepr, gens};

    proptest! {
        #[test]
        fn test_value_bid(kind in gens::delegator_kind_arb()) {
            bytesrepr::test_serialization_roundtrip(&kind);
        }
    }
}
