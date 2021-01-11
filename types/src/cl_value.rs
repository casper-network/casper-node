// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use alloc::{string::String, vec::Vec};
use core::fmt;

use failure::Fail;
#[cfg(feature = "std")]
use schemars::JsonSchema;
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{json, Value};

use crate::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes, U32_SERIALIZED_LENGTH},
    CLType, CLTyped, Key, PublicKey, URef, U128, U256, U512,
};

/// Error while converting a [`CLValue`] into a given type.
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct CLTypeMismatch {
    /// The [`CLType`] into which the `CLValue` was being converted.
    pub expected: CLType,
    /// The actual underlying [`CLType`] of this `CLValue`, i.e. the type from which it was
    /// constructed.
    pub found: CLType,
}

impl fmt::Display for CLTypeMismatch {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(
            f,
            "Expected {:?} but found {:?}.",
            self.expected, self.found
        )
    }
}

/// Error relating to [`CLValue`] operations.
#[derive(Fail, PartialEq, Eq, Clone, Debug)]
pub enum CLValueError {
    /// An error while serializing or deserializing the underlying data.
    #[fail(display = "CLValue error: {}", _0)]
    Serialization(bytesrepr::Error),
    /// A type mismatch while trying to convert a [`CLValue`] into a given type.
    #[fail(display = "Type mismatch: {}", _0)]
    Type(CLTypeMismatch),
}

impl From<bytesrepr::Error> for CLValueError {
    fn from(error: bytesrepr::Error) -> Self {
        CLValueError::Serialization(error)
    }
}

/// A Casper value, i.e. a value which can be stored and manipulated by smart contracts.
///
/// It holds the underlying data as a type-erased, serialized `Vec<u8>` and also holds the
/// [`CLType`] of the underlying data as a separate member.
#[derive(PartialEq, Eq, Clone, Debug)]
#[cfg_attr(feature = "std", derive(JsonSchema))]
pub struct CLValue {
    cl_type: CLType,
    #[cfg_attr(
        feature = "std",
        schemars(
            with = "String",
            description = "Hex-encoded value, serialized using ToBytes."
        )
    )]
    bytes: Bytes,
}

impl CLValue {
    /// Constructs a `CLValue` from `t`.
    pub fn from_t<T: CLTyped + ToBytes>(t: T) -> Result<CLValue, CLValueError> {
        let bytes = t.into_bytes()?;

        Ok(CLValue {
            cl_type: T::cl_type(),
            bytes: bytes.into(),
        })
    }

    /// Consumes and converts `self` back into its underlying type.
    pub fn into_t<T: CLTyped + FromBytes>(self) -> Result<T, CLValueError> {
        let expected = T::cl_type();

        if self.cl_type == expected {
            Ok(bytesrepr::deserialize(self.bytes.into())?)
        } else {
            Err(CLValueError::Type(CLTypeMismatch {
                expected,
                found: self.cl_type,
            }))
        }
    }

    // This is only required in order to implement `TryFrom<state::CLValue> for CLValue` (i.e. the
    // conversion from the Protobuf `CLValue`) in a separate module to this one.
    #[doc(hidden)]
    pub fn from_components(cl_type: CLType, bytes: Vec<u8>) -> Self {
        Self {
            cl_type,
            bytes: bytes.into(),
        }
    }

    // This is only required in order to implement `From<CLValue> for state::CLValue` (i.e. the
    // conversion to the Protobuf `CLValue`) in a separate module to this one.
    #[doc(hidden)]
    pub fn destructure(self) -> (CLType, Bytes) {
        (self.cl_type, self.bytes)
    }

    /// The [`CLType`] of the underlying data.
    pub fn cl_type(&self) -> &CLType {
        &self.cl_type
    }

    /// Returns a reference to the serialized form of the underlying value held in this `CLValue`.
    pub fn inner_bytes(&self) -> &Vec<u8> {
        self.bytes.inner_bytes()
    }

    /// Returns the length of the `Vec<u8>` yielded after calling `self.to_bytes()`.
    ///
    /// Note, this method doesn't actually serialize `self`, and hence is relatively cheap.
    pub fn serialized_length(&self) -> usize {
        self.cl_type.serialized_length() + U32_SERIALIZED_LENGTH + self.bytes.len()
    }

    fn jsonify<T: FromBytes + Serialize>(&self) -> Option<Value> {
        Some(json!(bytesrepr::deserialize::<T>(
            self.bytes.clone().into()
        )
        .ok()?))
    }

    /// Returns a best-effort attempt to convert the `CLValue` into a meaningful JSON value.  For
    /// complex types, the returned `Value` will just be the map containing the `CLType` and the
    /// raw bytes as hex.
    #[rustfmt::skip]
    fn to_json(&self) -> Option<Value> {
        match &self.cl_type {
            // Simple types.
            CLType::Bool => self.jsonify::<bool>(),
            CLType::I32 => self.jsonify::<i32>(),
            CLType::I64 => self.jsonify::<i64>(),
            CLType::U8 => self.jsonify::<u8>(),
            CLType::U32 => self.jsonify::<u32>(),
            CLType::U64 => self.jsonify::<u64>(),
            CLType::U128 => self.jsonify::<U128>(),
            CLType::U256 => self.jsonify::<U256>(),
            CLType::U512 => self.jsonify::<U512>(),
            CLType::Unit => self.jsonify::<()>(),
            CLType::String => self.jsonify::<String>(),
            CLType::Key => self.jsonify::<Key>(),
            CLType::URef => self.jsonify::<URef>(),
            CLType::PublicKey => self.jsonify::<PublicKey>(),

            // Option of simple types.
            t if *t == Option::<bool>::cl_type() => self.jsonify::<Option<bool>>(),
            t if *t == Option::<i32>::cl_type() => self.jsonify::<Option<i32>>(),
            t if *t == Option::<i64>::cl_type() => self.jsonify::<Option<i64>>(),
            t if *t == Option::<u8>::cl_type() => self.jsonify::<Option<u8>>(),
            t if *t == Option::<u32>::cl_type() => self.jsonify::<Option<u32>>(),
            t if *t == Option::<u64>::cl_type() => self.jsonify::<Option<u64>>(),
            t if *t == Option::<U128>::cl_type() => self.jsonify::<Option<U128>>(),
            t if *t == Option::<U256>::cl_type() => self.jsonify::<Option<U256>>(),
            t if *t == Option::<U512>::cl_type() => self.jsonify::<Option<U512>>(),
            t if *t == Option::<()>::cl_type() => self.jsonify::<Option<()>>(),
            t if *t == Option::<String>::cl_type() => self.jsonify::<Option<String>>(),
            t if *t == Option::<Key>::cl_type() => self.jsonify::<Option<Key>>(),
            t if *t == Option::<URef>::cl_type() => self.jsonify::<Option<URef>>(),
            t if *t == Option::<PublicKey>::cl_type() => self.jsonify::<Option<PublicKey>>(),

            // Result of simple types.
            t if *t == Result::<bool, i32>::cl_type() => self.jsonify::<Result<bool, i32>>(),
            t if *t == Result::<bool, u32>::cl_type() => self.jsonify::<Result<bool, u32>>(),
            t if *t == Result::<bool, ()>::cl_type() => self.jsonify::<Result<bool, ()>>(),
            t if *t == Result::<bool, String>::cl_type() => self.jsonify::<Result<bool, String>>(),
            t if *t == Result::<i32, i32>::cl_type() => self.jsonify::<Result<i32, i32>>(),
            t if *t == Result::<i32, u32>::cl_type() => self.jsonify::<Result<i32, u32>>(),
            t if *t == Result::<i32, ()>::cl_type() => self.jsonify::<Result<i32, ()>>(),
            t if *t == Result::<i32, String>::cl_type() => self.jsonify::<Result<i32, String>>(),
            t if *t == Result::<i64, i32>::cl_type() => self.jsonify::<Result<i64, i32>>(),
            t if *t == Result::<i64, u32>::cl_type() => self.jsonify::<Result<i64, u32>>(),
            t if *t == Result::<i64, ()>::cl_type() => self.jsonify::<Result<i64, ()>>(),
            t if *t == Result::<i64, String>::cl_type() => self.jsonify::<Result<i64, String>>(),
            t if *t == Result::<u8, i32>::cl_type() => self.jsonify::<Result<u8, i32>>(),
            t if *t == Result::<u8, u32>::cl_type() => self.jsonify::<Result<u8, u32>>(),
            t if *t == Result::<u8, ()>::cl_type() => self.jsonify::<Result<u8, ()>>(),
            t if *t == Result::<u8, String>::cl_type() => self.jsonify::<Result<u8, String>>(),
            t if *t == Result::<u32, i32>::cl_type() => self.jsonify::<Result<u32, i32>>(),
            t if *t == Result::<u32, u32>::cl_type() => self.jsonify::<Result<u32, u32>>(),
            t if *t == Result::<u32, ()>::cl_type() => self.jsonify::<Result<u32, ()>>(),
            t if *t == Result::<u32, String>::cl_type() => self.jsonify::<Result<u32, String>>(),
            t if *t == Result::<u64, i32>::cl_type() => self.jsonify::<Result<u64, i32>>(),
            t if *t == Result::<u64, u32>::cl_type() => self.jsonify::<Result<u64, u32>>(),
            t if *t == Result::<u64, ()>::cl_type() => self.jsonify::<Result<u64, ()>>(),
            t if *t == Result::<u64, String>::cl_type() => self.jsonify::<Result<u64, String>>(),
            t if *t == Result::<U128, i32>::cl_type() => self.jsonify::<Result<U128, i32>>(),
            t if *t == Result::<U128, u32>::cl_type() => self.jsonify::<Result<U128, u32>>(),
            t if *t == Result::<U128, ()>::cl_type() => self.jsonify::<Result<U128, ()>>(),
            t if *t == Result::<U128, String>::cl_type() => self.jsonify::<Result<U128, String>>(),
            t if *t == Result::<U256, i32>::cl_type() => self.jsonify::<Result<U256, i32>>(),
            t if *t == Result::<U256, u32>::cl_type() => self.jsonify::<Result<U256, u32>>(),
            t if *t == Result::<U256, ()>::cl_type() => self.jsonify::<Result<U256, ()>>(),
            t if *t == Result::<U256, String>::cl_type() => self.jsonify::<Result<U256, String>>(),
            t if *t == Result::<U512, i32>::cl_type() => self.jsonify::<Result<U512, i32>>(),
            t if *t == Result::<U512, u32>::cl_type() => self.jsonify::<Result<U512, u32>>(),
            t if *t == Result::<U512, ()>::cl_type() => self.jsonify::<Result<U512, ()>>(),
            t if *t == Result::<U512, String>::cl_type() => self.jsonify::<Result<U512, String>>(),
            t if *t == Result::<(), i32>::cl_type() => self.jsonify::<Result<(), i32>>(),
            t if *t == Result::<(), u32>::cl_type() => self.jsonify::<Result<(), u32>>(),
            t if *t == Result::<(), ()>::cl_type() => self.jsonify::<Result<(), ()>>(),
            t if *t == Result::<(), String>::cl_type() => self.jsonify::<Result<(), String>>(),
            t if *t == Result::<String, i32>::cl_type() => self.jsonify::<Result<String, i32>>(),
            t if *t == Result::<String, u32>::cl_type() => self.jsonify::<Result<String, u32>>(),
            t if *t == Result::<String, ()>::cl_type() => self.jsonify::<Result<String, ()>>(),
            t if *t == Result::<String, String>::cl_type() => self.jsonify::<Result<String, String>>(),
            t if *t == Result::<Key, i32>::cl_type() => self.jsonify::<Result<Key, i32>>(),
            t if *t == Result::<Key, u32>::cl_type() => self.jsonify::<Result<Key, u32>>(),
            t if *t == Result::<Key, ()>::cl_type() => self.jsonify::<Result<Key, ()>>(),
            t if *t == Result::<Key, String>::cl_type() => self.jsonify::<Result<Key, String>>(),
            t if *t == Result::<URef, i32>::cl_type() => self.jsonify::<Result<URef, i32>>(),
            t if *t == Result::<URef, u32>::cl_type() => self.jsonify::<Result<URef, u32>>(),
            t if *t == Result::<URef, ()>::cl_type() => self.jsonify::<Result<URef, ()>>(),
            t if *t == Result::<URef, String>::cl_type() => self.jsonify::<Result<URef, String>>(),
            t if *t == Result::<PublicKey, i32>::cl_type() => self.jsonify::<Result<PublicKey, i32>>(),
            t if *t == Result::<PublicKey, u32>::cl_type() => self.jsonify::<Result<PublicKey, u32>>(),
            t if *t == Result::<PublicKey, ()>::cl_type() => self.jsonify::<Result<PublicKey, ()>>(),
            t if *t == Result::<PublicKey, String>::cl_type() => self.jsonify::<Result<PublicKey, String>>(),

            _ => None,
        }
    }
}

impl ToBytes for CLValue {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.clone().into_bytes()
    }

    fn into_bytes(self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = self.bytes.into_bytes()?;
        self.cl_type.append_bytes(&mut result)?;
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.bytes.serialized_length() + self.cl_type.serialized_length()
    }
}

impl FromBytes for CLValue {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (bytes, remainder) = FromBytes::from_bytes(bytes)?;
        let (cl_type, remainder) = FromBytes::from_bytes(remainder)?;
        let cl_value = CLValue { cl_type, bytes };
        Ok((cl_value, remainder))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CLValueJson {
    cl_type: CLType,
    serialized_bytes: String,
    parsed_to_json: Option<Value>,
}

impl Serialize for CLValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            CLValueJson {
                cl_type: self.cl_type.clone(),
                serialized_bytes: base16::encode_lower(&self.bytes),
                parsed_to_json: self.to_json(),
            }
            .serialize(serializer)
        } else {
            (&self.cl_type, &self.bytes).serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for CLValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (cl_type, bytes) = if deserializer.is_human_readable() {
            let json = CLValueJson::deserialize(deserializer)?;
            (
                json.cl_type.clone(),
                base16::decode(&json.serialized_bytes).map_err(D::Error::custom)?,
            )
        } else {
            <(CLType, Vec<u8>)>::deserialize(deserializer)?
        };
        Ok(CLValue {
            cl_type,
            bytes: bytes.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::*;
    use crate::{
        account::{AccountHash, ACCOUNT_HASH_LENGTH},
        key::KEY_HASH_LENGTH,
        AccessRights, DeployHash, TransferAddr, DEPLOY_HASH_LENGTH, TRANSFER_ADDR_LENGTH,
        UREF_ADDR_LENGTH,
    };

    #[test]
    fn serde_roundtrip() {
        let cl_value = CLValue::from_t(true).unwrap();
        let serialized = bincode::serialize(&cl_value).unwrap();
        let decoded = bincode::deserialize(&serialized).unwrap();
        assert_eq!(cl_value, decoded);
    }

    #[test]
    fn json_roundtrip() {
        let cl_value = CLValue::from_t(true).unwrap();
        let json_string = serde_json::to_string_pretty(&cl_value).unwrap();
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(cl_value, decoded);
    }

    fn check_to_json<T: CLTyped + ToBytes + FromBytes>(value: T, expected: &str) {
        let cl_value = CLValue::from_t(value).unwrap();
        let cl_value_as_json = serde_json::to_string(&cl_value).unwrap();
        // Remove the `serialized_bytes` field:
        // Split the string at `,"serialized_bytes":`.
        let pattern = r#","serialized_bytes":""#;
        let start_index = cl_value_as_json.find(pattern).unwrap();
        let (start, end) = cl_value_as_json.split_at(start_index);
        // Find the end of the value of the `serialized_bytes` field, and split there.
        let mut json_without_serialize_bytes = start.to_string();
        for (index, char) in end.char_indices().skip(pattern.len()) {
            if char == '"' {
                let (_to_remove, to_keep) = end.split_at(index + 1);
                json_without_serialize_bytes.push_str(to_keep);
                break;
            }
        }
        assert_eq!(json_without_serialize_bytes, expected);
    }

    mod simple_types {
        use super::*;
        use crate::crypto::SecretKey;

        #[test]
        fn bool_cl_value_should_encode_to_json() {
            check_to_json(true, r#"{"cl_type":"Bool","parsed_to_json":true}"#);
            check_to_json(false, r#"{"cl_type":"Bool","parsed_to_json":false}"#);
        }

        #[test]
        fn i32_cl_value_should_encode_to_json() {
            check_to_json(
                i32::min_value(),
                r#"{"cl_type":"I32","parsed_to_json":-2147483648}"#,
            );
            check_to_json(0_i32, r#"{"cl_type":"I32","parsed_to_json":0}"#);
            check_to_json(
                i32::max_value(),
                r#"{"cl_type":"I32","parsed_to_json":2147483647}"#,
            );
        }

        #[test]
        fn i64_cl_value_should_encode_to_json() {
            check_to_json(
                i64::min_value(),
                r#"{"cl_type":"I64","parsed_to_json":-9223372036854775808}"#,
            );
            check_to_json(0_i64, r#"{"cl_type":"I64","parsed_to_json":0}"#);
            check_to_json(
                i64::max_value(),
                r#"{"cl_type":"I64","parsed_to_json":9223372036854775807}"#,
            );
        }

        #[test]
        fn u8_cl_value_should_encode_to_json() {
            check_to_json(0_u8, r#"{"cl_type":"U8","parsed_to_json":0}"#);
            check_to_json(u8::max_value(), r#"{"cl_type":"U8","parsed_to_json":255}"#);
        }

        #[test]
        fn u32_cl_value_should_encode_to_json() {
            check_to_json(0_u32, r#"{"cl_type":"U32","parsed_to_json":0}"#);
            check_to_json(
                u32::max_value(),
                r#"{"cl_type":"U32","parsed_to_json":4294967295}"#,
            );
        }

        #[test]
        fn u64_cl_value_should_encode_to_json() {
            check_to_json(0_u64, r#"{"cl_type":"U64","parsed_to_json":0}"#);
            check_to_json(
                u64::max_value(),
                r#"{"cl_type":"U64","parsed_to_json":18446744073709551615}"#,
            );
        }

        #[test]
        fn u128_cl_value_should_encode_to_json() {
            check_to_json(U128::zero(), r#"{"cl_type":"U128","parsed_to_json":"0"}"#);
            check_to_json(
                U128::max_value(),
                r#"{"cl_type":"U128","parsed_to_json":"340282366920938463463374607431768211455"}"#,
            );
        }

        #[test]
        fn u256_cl_value_should_encode_to_json() {
            check_to_json(U256::zero(), r#"{"cl_type":"U256","parsed_to_json":"0"}"#);
            check_to_json(
                U256::max_value(),
                r#"{"cl_type":"U256","parsed_to_json":"115792089237316195423570985008687907853269984665640564039457584007913129639935"}"#,
            );
        }

        #[test]
        fn u512_cl_value_should_encode_to_json() {
            check_to_json(U512::zero(), r#"{"cl_type":"U512","parsed_to_json":"0"}"#);
            check_to_json(
                U512::max_value(),
                r#"{"cl_type":"U512","parsed_to_json":"13407807929942597099574024998205846127479365820592393377723561443721764030073546976801874298166903427690031858186486050853753882811946569946433649006084095"}"#,
            );
        }

        #[test]
        fn unit_cl_value_should_encode_to_json() {
            check_to_json((), r#"{"cl_type":"Unit","parsed_to_json":null}"#);
        }

        #[test]
        fn string_cl_value_should_encode_to_json() {
            check_to_json(String::new(), r#"{"cl_type":"String","parsed_to_json":""}"#);
            check_to_json(
                "test string".to_string(),
                r#"{"cl_type":"String","parsed_to_json":"test string"}"#,
            );
        }

        #[test]
        fn key_cl_value_should_encode_to_json() {
            let key_account = Key::Account(AccountHash::new([1; ACCOUNT_HASH_LENGTH]));
            check_to_json(
                key_account,
                r#"{"cl_type":"Key","parsed_to_json":{"Account":"account-hash-0101010101010101010101010101010101010101010101010101010101010101"}}"#,
            );

            let key_hash = Key::Hash([2; KEY_HASH_LENGTH]);
            check_to_json(
                key_hash,
                r#"{"cl_type":"Key","parsed_to_json":{"Hash":"hash-0202020202020202020202020202020202020202020202020202020202020202"}}"#,
            );

            let key_uref = Key::URef(URef::new([3; UREF_ADDR_LENGTH], AccessRights::READ));
            check_to_json(
                key_uref,
                r#"{"cl_type":"Key","parsed_to_json":{"URef":"uref-0303030303030303030303030303030303030303030303030303030303030303-001"}}"#,
            );

            let key_transfer = Key::Transfer(TransferAddr::new([4; TRANSFER_ADDR_LENGTH]));
            check_to_json(
                key_transfer,
                r#"{"cl_type":"Key","parsed_to_json":{"Transfer":"transfer-0404040404040404040404040404040404040404040404040404040404040404"}}"#,
            );

            let key_deploy_info = Key::DeployInfo(DeployHash::new([5; DEPLOY_HASH_LENGTH]));
            check_to_json(
                key_deploy_info,
                r#"{"cl_type":"Key","parsed_to_json":{"DeployInfo":"deploy-0505050505050505050505050505050505050505050505050505050505050505"}}"#,
            );
        }

        #[test]
        fn uref_cl_value_should_encode_to_json() {
            let uref = URef::new([6; UREF_ADDR_LENGTH], AccessRights::READ_ADD_WRITE);
            check_to_json(
                uref,
                r#"{"cl_type":"URef","parsed_to_json":"uref-0606060606060606060606060606060606060606060606060606060606060606-007"}"#,
            );
        }

        #[test]
        fn public_key_cl_value_should_encode_to_json() {
            check_to_json(
                PublicKey::from(SecretKey::ed25519([7; SecretKey::ED25519_LENGTH])),
                r#"{"cl_type":"PublicKey","parsed_to_json":"01ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c"}"#,
            );
            check_to_json(
                PublicKey::from(SecretKey::secp256k1([8; SecretKey::SECP256K1_LENGTH])),
                r#"{"cl_type":"PublicKey","parsed_to_json":"0203f991f944d1e1954a7fc8b9bf62e0d78f015f4c07762d505e20e6c45260a3661b"}"#,
            );
        }
    }

    mod option {
        use super::*;
        use crate::crypto::SecretKey;

        #[test]
        fn bool_cl_value_should_encode_to_json() {
            check_to_json(
                Some(true),
                r#"{"cl_type":{"Option":"Bool"},"parsed_to_json":true}"#,
            );
            check_to_json(
                Some(false),
                r#"{"cl_type":{"Option":"Bool"},"parsed_to_json":false}"#,
            );
            check_to_json(
                Option::<bool>::None,
                r#"{"cl_type":{"Option":"Bool"},"parsed_to_json":null}"#,
            );
        }

        #[test]
        fn i32_cl_value_should_encode_to_json() {
            check_to_json(
                Some(i32::min_value()),
                r#"{"cl_type":{"Option":"I32"},"parsed_to_json":-2147483648}"#,
            );
            check_to_json(
                Some(0_i32),
                r#"{"cl_type":{"Option":"I32"},"parsed_to_json":0}"#,
            );
            check_to_json(
                Some(i32::max_value()),
                r#"{"cl_type":{"Option":"I32"},"parsed_to_json":2147483647}"#,
            );
            check_to_json(
                Option::<i32>::None,
                r#"{"cl_type":{"Option":"I32"},"parsed_to_json":null}"#,
            );
        }

        #[test]
        fn i64_cl_value_should_encode_to_json() {
            check_to_json(
                Some(i64::min_value()),
                r#"{"cl_type":{"Option":"I64"},"parsed_to_json":-9223372036854775808}"#,
            );
            check_to_json(
                Some(0_i64),
                r#"{"cl_type":{"Option":"I64"},"parsed_to_json":0}"#,
            );
            check_to_json(
                Some(i64::max_value()),
                r#"{"cl_type":{"Option":"I64"},"parsed_to_json":9223372036854775807}"#,
            );
            check_to_json(
                Option::<i64>::None,
                r#"{"cl_type":{"Option":"I64"},"parsed_to_json":null}"#,
            );
        }

        #[test]
        fn u8_cl_value_should_encode_to_json() {
            check_to_json(
                Some(0_u8),
                r#"{"cl_type":{"Option":"U8"},"parsed_to_json":0}"#,
            );
            check_to_json(
                Some(u8::max_value()),
                r#"{"cl_type":{"Option":"U8"},"parsed_to_json":255}"#,
            );
            check_to_json(
                Option::<u8>::None,
                r#"{"cl_type":{"Option":"U8"},"parsed_to_json":null}"#,
            );
        }

        #[test]
        fn u32_cl_value_should_encode_to_json() {
            check_to_json(
                Some(0_u32),
                r#"{"cl_type":{"Option":"U32"},"parsed_to_json":0}"#,
            );
            check_to_json(
                Some(u32::max_value()),
                r#"{"cl_type":{"Option":"U32"},"parsed_to_json":4294967295}"#,
            );
            check_to_json(
                Option::<u32>::None,
                r#"{"cl_type":{"Option":"U32"},"parsed_to_json":null}"#,
            );
        }

        #[test]
        fn u64_cl_value_should_encode_to_json() {
            check_to_json(
                Some(0_u64),
                r#"{"cl_type":{"Option":"U64"},"parsed_to_json":0}"#,
            );
            check_to_json(
                Some(u64::max_value()),
                r#"{"cl_type":{"Option":"U64"},"parsed_to_json":18446744073709551615}"#,
            );
            check_to_json(
                Option::<u64>::None,
                r#"{"cl_type":{"Option":"U64"},"parsed_to_json":null}"#,
            );
        }

        #[test]
        fn u128_cl_value_should_encode_to_json() {
            check_to_json(
                Some(U128::zero()),
                r#"{"cl_type":{"Option":"U128"},"parsed_to_json":"0"}"#,
            );
            check_to_json(
                Some(U128::max_value()),
                r#"{"cl_type":{"Option":"U128"},"parsed_to_json":"340282366920938463463374607431768211455"}"#,
            );
            check_to_json(
                Option::<U128>::None,
                r#"{"cl_type":{"Option":"U128"},"parsed_to_json":null}"#,
            );
        }

        #[test]
        fn u256_cl_value_should_encode_to_json() {
            check_to_json(
                Some(U256::zero()),
                r#"{"cl_type":{"Option":"U256"},"parsed_to_json":"0"}"#,
            );
            check_to_json(
                Some(U256::max_value()),
                r#"{"cl_type":{"Option":"U256"},"parsed_to_json":"115792089237316195423570985008687907853269984665640564039457584007913129639935"}"#,
            );
            check_to_json(
                Option::<U256>::None,
                r#"{"cl_type":{"Option":"U256"},"parsed_to_json":null}"#,
            );
        }

        #[test]
        fn u512_cl_value_should_encode_to_json() {
            check_to_json(
                Some(U512::zero()),
                r#"{"cl_type":{"Option":"U512"},"parsed_to_json":"0"}"#,
            );
            check_to_json(
                Some(U512::max_value()),
                r#"{"cl_type":{"Option":"U512"},"parsed_to_json":"13407807929942597099574024998205846127479365820592393377723561443721764030073546976801874298166903427690031858186486050853753882811946569946433649006084095"}"#,
            );
            check_to_json(
                Option::<U512>::None,
                r#"{"cl_type":{"Option":"U512"},"parsed_to_json":null}"#,
            );
        }

        #[test]
        fn unit_cl_value_should_encode_to_json() {
            check_to_json(
                Some(()),
                r#"{"cl_type":{"Option":"Unit"},"parsed_to_json":null}"#,
            );
            check_to_json(
                Option::<()>::None,
                r#"{"cl_type":{"Option":"Unit"},"parsed_to_json":null}"#,
            );
        }

        #[test]
        fn string_cl_value_should_encode_to_json() {
            check_to_json(
                Some(String::new()),
                r#"{"cl_type":{"Option":"String"},"parsed_to_json":""}"#,
            );
            check_to_json(
                Some("test string".to_string()),
                r#"{"cl_type":{"Option":"String"},"parsed_to_json":"test string"}"#,
            );
            check_to_json(
                Option::<String>::None,
                r#"{"cl_type":{"Option":"String"},"parsed_to_json":null}"#,
            );
        }

        #[test]
        fn key_cl_value_should_encode_to_json() {
            let key_account = Key::Account(AccountHash::new([1; ACCOUNT_HASH_LENGTH]));
            check_to_json(
                Some(key_account),
                r#"{"cl_type":{"Option":"Key"},"parsed_to_json":{"Account":"account-hash-0101010101010101010101010101010101010101010101010101010101010101"}}"#,
            );

            let key_hash = Key::Hash([2; KEY_HASH_LENGTH]);
            check_to_json(
                Some(key_hash),
                r#"{"cl_type":{"Option":"Key"},"parsed_to_json":{"Hash":"hash-0202020202020202020202020202020202020202020202020202020202020202"}}"#,
            );

            let key_uref = Key::URef(URef::new([3; UREF_ADDR_LENGTH], AccessRights::READ));
            check_to_json(
                Some(key_uref),
                r#"{"cl_type":{"Option":"Key"},"parsed_to_json":{"URef":"uref-0303030303030303030303030303030303030303030303030303030303030303-001"}}"#,
            );

            let key_transfer = Key::Transfer(TransferAddr::new([4; TRANSFER_ADDR_LENGTH]));
            check_to_json(
                Some(key_transfer),
                r#"{"cl_type":{"Option":"Key"},"parsed_to_json":{"Transfer":"transfer-0404040404040404040404040404040404040404040404040404040404040404"}}"#,
            );

            let key_deploy_info = Key::DeployInfo(DeployHash::new([5; DEPLOY_HASH_LENGTH]));
            check_to_json(
                Some(key_deploy_info),
                r#"{"cl_type":{"Option":"Key"},"parsed_to_json":{"DeployInfo":"deploy-0505050505050505050505050505050505050505050505050505050505050505"}}"#,
            );

            check_to_json(
                Option::<Key>::None,
                r#"{"cl_type":{"Option":"Key"},"parsed_to_json":null}"#,
            )
        }

        #[test]
        fn uref_cl_value_should_encode_to_json() {
            let uref = URef::new([6; UREF_ADDR_LENGTH], AccessRights::READ_ADD_WRITE);
            check_to_json(
                Some(uref),
                r#"{"cl_type":{"Option":"URef"},"parsed_to_json":"uref-0606060606060606060606060606060606060606060606060606060606060606-007"}"#,
            );
            check_to_json(
                Option::<URef>::None,
                r#"{"cl_type":{"Option":"URef"},"parsed_to_json":null}"#,
            )
        }

        #[test]
        fn public_key_cl_value_should_encode_to_json() {
            check_to_json(
                Some(PublicKey::from(SecretKey::ed25519(
                    [7; SecretKey::ED25519_LENGTH],
                ))),
                r#"{"cl_type":{"Option":"PublicKey"},"parsed_to_json":"01ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c"}"#,
            );
            check_to_json(
                Some(PublicKey::from(SecretKey::secp256k1(
                    [8; SecretKey::SECP256K1_LENGTH],
                ))),
                r#"{"cl_type":{"Option":"PublicKey"},"parsed_to_json":"0203f991f944d1e1954a7fc8b9bf62e0d78f015f4c07762d505e20e6c45260a3661b"}"#,
            );
            check_to_json(
                Option::<PublicKey>::None,
                r#"{"cl_type":{"Option":"PublicKey"},"parsed_to_json":null}"#,
            )
        }
    }

    mod result {
        use super::*;
        use crate::crypto::SecretKey;

        #[test]
        fn bool_cl_value_should_encode_to_json() {
            check_to_json(
                Result::<bool, i32>::Ok(true),
                r#"{"cl_type":{"Result":{"ok":"Bool","err":"I32"}},"parsed_to_json":{"Ok":true}}"#,
            );
            check_to_json(
                Result::<bool, u32>::Ok(true),
                r#"{"cl_type":{"Result":{"ok":"Bool","err":"U32"}},"parsed_to_json":{"Ok":true}}"#,
            );
            check_to_json(
                Result::<bool, ()>::Ok(true),
                r#"{"cl_type":{"Result":{"ok":"Bool","err":"Unit"}},"parsed_to_json":{"Ok":true}}"#,
            );
            check_to_json(
                Result::<bool, String>::Ok(true),
                r#"{"cl_type":{"Result":{"ok":"Bool","err":"String"}},"parsed_to_json":{"Ok":true}}"#,
            );
            check_to_json(
                Result::<bool, i32>::Err(-1),
                r#"{"cl_type":{"Result":{"ok":"Bool","err":"I32"}},"parsed_to_json":{"Err":-1}}"#,
            );
            check_to_json(
                Result::<bool, u32>::Err(1),
                r#"{"cl_type":{"Result":{"ok":"Bool","err":"U32"}},"parsed_to_json":{"Err":1}}"#,
            );
            check_to_json(
                Result::<bool, ()>::Err(()),
                r#"{"cl_type":{"Result":{"ok":"Bool","err":"Unit"}},"parsed_to_json":{"Err":null}}"#,
            );
            check_to_json(
                Result::<bool, String>::Err("e".to_string()),
                r#"{"cl_type":{"Result":{"ok":"Bool","err":"String"}},"parsed_to_json":{"Err":"e"}}"#,
            );
        }

        #[test]
        fn i32_cl_value_should_encode_to_json() {
            check_to_json(
                Result::<i32, i32>::Ok(-1),
                r#"{"cl_type":{"Result":{"ok":"I32","err":"I32"}},"parsed_to_json":{"Ok":-1}}"#,
            );
            check_to_json(
                Result::<i32, u32>::Ok(-1),
                r#"{"cl_type":{"Result":{"ok":"I32","err":"U32"}},"parsed_to_json":{"Ok":-1}}"#,
            );
            check_to_json(
                Result::<i32, ()>::Ok(-1),
                r#"{"cl_type":{"Result":{"ok":"I32","err":"Unit"}},"parsed_to_json":{"Ok":-1}}"#,
            );
            check_to_json(
                Result::<i32, String>::Ok(-1),
                r#"{"cl_type":{"Result":{"ok":"I32","err":"String"}},"parsed_to_json":{"Ok":-1}}"#,
            );
            check_to_json(
                Result::<i32, i32>::Err(-1),
                r#"{"cl_type":{"Result":{"ok":"I32","err":"I32"}},"parsed_to_json":{"Err":-1}}"#,
            );
            check_to_json(
                Result::<i32, u32>::Err(1),
                r#"{"cl_type":{"Result":{"ok":"I32","err":"U32"}},"parsed_to_json":{"Err":1}}"#,
            );
            check_to_json(
                Result::<i32, ()>::Err(()),
                r#"{"cl_type":{"Result":{"ok":"I32","err":"Unit"}},"parsed_to_json":{"Err":null}}"#,
            );
            check_to_json(
                Result::<i32, String>::Err("e".to_string()),
                r#"{"cl_type":{"Result":{"ok":"I32","err":"String"}},"parsed_to_json":{"Err":"e"}}"#,
            );
        }

        #[test]
        fn i64_cl_value_should_encode_to_json() {
            check_to_json(
                Result::<i64, i32>::Ok(-1),
                r#"{"cl_type":{"Result":{"ok":"I64","err":"I32"}},"parsed_to_json":{"Ok":-1}}"#,
            );
            check_to_json(
                Result::<i64, u32>::Ok(-1),
                r#"{"cl_type":{"Result":{"ok":"I64","err":"U32"}},"parsed_to_json":{"Ok":-1}}"#,
            );
            check_to_json(
                Result::<i64, ()>::Ok(-1),
                r#"{"cl_type":{"Result":{"ok":"I64","err":"Unit"}},"parsed_to_json":{"Ok":-1}}"#,
            );
            check_to_json(
                Result::<i64, String>::Ok(-1),
                r#"{"cl_type":{"Result":{"ok":"I64","err":"String"}},"parsed_to_json":{"Ok":-1}}"#,
            );
            check_to_json(
                Result::<i64, i32>::Err(-1),
                r#"{"cl_type":{"Result":{"ok":"I64","err":"I32"}},"parsed_to_json":{"Err":-1}}"#,
            );
            check_to_json(
                Result::<i64, u32>::Err(1),
                r#"{"cl_type":{"Result":{"ok":"I64","err":"U32"}},"parsed_to_json":{"Err":1}}"#,
            );
            check_to_json(
                Result::<i64, ()>::Err(()),
                r#"{"cl_type":{"Result":{"ok":"I64","err":"Unit"}},"parsed_to_json":{"Err":null}}"#,
            );
            check_to_json(
                Result::<i64, String>::Err("e".to_string()),
                r#"{"cl_type":{"Result":{"ok":"I64","err":"String"}},"parsed_to_json":{"Err":"e"}}"#,
            );
        }

        #[test]
        fn u8_cl_value_should_encode_to_json() {
            check_to_json(
                Result::<u8, i32>::Ok(1),
                r#"{"cl_type":{"Result":{"ok":"U8","err":"I32"}},"parsed_to_json":{"Ok":1}}"#,
            );
            check_to_json(
                Result::<u8, u32>::Ok(1),
                r#"{"cl_type":{"Result":{"ok":"U8","err":"U32"}},"parsed_to_json":{"Ok":1}}"#,
            );
            check_to_json(
                Result::<u8, ()>::Ok(1),
                r#"{"cl_type":{"Result":{"ok":"U8","err":"Unit"}},"parsed_to_json":{"Ok":1}}"#,
            );
            check_to_json(
                Result::<u8, String>::Ok(1),
                r#"{"cl_type":{"Result":{"ok":"U8","err":"String"}},"parsed_to_json":{"Ok":1}}"#,
            );
            check_to_json(
                Result::<u8, i32>::Err(-1),
                r#"{"cl_type":{"Result":{"ok":"U8","err":"I32"}},"parsed_to_json":{"Err":-1}}"#,
            );
            check_to_json(
                Result::<u8, u32>::Err(1),
                r#"{"cl_type":{"Result":{"ok":"U8","err":"U32"}},"parsed_to_json":{"Err":1}}"#,
            );
            check_to_json(
                Result::<u8, ()>::Err(()),
                r#"{"cl_type":{"Result":{"ok":"U8","err":"Unit"}},"parsed_to_json":{"Err":null}}"#,
            );
            check_to_json(
                Result::<u8, String>::Err("e".to_string()),
                r#"{"cl_type":{"Result":{"ok":"U8","err":"String"}},"parsed_to_json":{"Err":"e"}}"#,
            );
        }

        #[test]
        fn u32_cl_value_should_encode_to_json() {
            check_to_json(
                Result::<u32, i32>::Ok(1),
                r#"{"cl_type":{"Result":{"ok":"U32","err":"I32"}},"parsed_to_json":{"Ok":1}}"#,
            );
            check_to_json(
                Result::<u32, u32>::Ok(1),
                r#"{"cl_type":{"Result":{"ok":"U32","err":"U32"}},"parsed_to_json":{"Ok":1}}"#,
            );
            check_to_json(
                Result::<u32, ()>::Ok(1),
                r#"{"cl_type":{"Result":{"ok":"U32","err":"Unit"}},"parsed_to_json":{"Ok":1}}"#,
            );
            check_to_json(
                Result::<u32, String>::Ok(1),
                r#"{"cl_type":{"Result":{"ok":"U32","err":"String"}},"parsed_to_json":{"Ok":1}}"#,
            );
            check_to_json(
                Result::<u32, i32>::Err(-1),
                r#"{"cl_type":{"Result":{"ok":"U32","err":"I32"}},"parsed_to_json":{"Err":-1}}"#,
            );
            check_to_json(
                Result::<u32, u32>::Err(1),
                r#"{"cl_type":{"Result":{"ok":"U32","err":"U32"}},"parsed_to_json":{"Err":1}}"#,
            );
            check_to_json(
                Result::<u32, ()>::Err(()),
                r#"{"cl_type":{"Result":{"ok":"U32","err":"Unit"}},"parsed_to_json":{"Err":null}}"#,
            );
            check_to_json(
                Result::<u32, String>::Err("e".to_string()),
                r#"{"cl_type":{"Result":{"ok":"U32","err":"String"}},"parsed_to_json":{"Err":"e"}}"#,
            );
        }

        #[test]
        fn u64_cl_value_should_encode_to_json() {
            check_to_json(
                Result::<u64, i32>::Ok(1),
                r#"{"cl_type":{"Result":{"ok":"U64","err":"I32"}},"parsed_to_json":{"Ok":1}}"#,
            );
            check_to_json(
                Result::<u64, u32>::Ok(1),
                r#"{"cl_type":{"Result":{"ok":"U64","err":"U32"}},"parsed_to_json":{"Ok":1}}"#,
            );
            check_to_json(
                Result::<u64, ()>::Ok(1),
                r#"{"cl_type":{"Result":{"ok":"U64","err":"Unit"}},"parsed_to_json":{"Ok":1}}"#,
            );
            check_to_json(
                Result::<u64, String>::Ok(1),
                r#"{"cl_type":{"Result":{"ok":"U64","err":"String"}},"parsed_to_json":{"Ok":1}}"#,
            );
            check_to_json(
                Result::<u64, i32>::Err(-1),
                r#"{"cl_type":{"Result":{"ok":"U64","err":"I32"}},"parsed_to_json":{"Err":-1}}"#,
            );
            check_to_json(
                Result::<u64, u32>::Err(1),
                r#"{"cl_type":{"Result":{"ok":"U64","err":"U32"}},"parsed_to_json":{"Err":1}}"#,
            );
            check_to_json(
                Result::<u64, ()>::Err(()),
                r#"{"cl_type":{"Result":{"ok":"U64","err":"Unit"}},"parsed_to_json":{"Err":null}}"#,
            );
            check_to_json(
                Result::<u64, String>::Err("e".to_string()),
                r#"{"cl_type":{"Result":{"ok":"U64","err":"String"}},"parsed_to_json":{"Err":"e"}}"#,
            );
        }

        #[test]
        fn u128_cl_value_should_encode_to_json() {
            check_to_json(
                Result::<U128, i32>::Ok(1.into()),
                r#"{"cl_type":{"Result":{"ok":"U128","err":"I32"}},"parsed_to_json":{"Ok":"1"}}"#,
            );
            check_to_json(
                Result::<U128, u32>::Ok(1.into()),
                r#"{"cl_type":{"Result":{"ok":"U128","err":"U32"}},"parsed_to_json":{"Ok":"1"}}"#,
            );
            check_to_json(
                Result::<U128, ()>::Ok(1.into()),
                r#"{"cl_type":{"Result":{"ok":"U128","err":"Unit"}},"parsed_to_json":{"Ok":"1"}}"#,
            );
            check_to_json(
                Result::<U128, String>::Ok(1.into()),
                r#"{"cl_type":{"Result":{"ok":"U128","err":"String"}},"parsed_to_json":{"Ok":"1"}}"#,
            );
            check_to_json(
                Result::<U128, i32>::Err(-1),
                r#"{"cl_type":{"Result":{"ok":"U128","err":"I32"}},"parsed_to_json":{"Err":-1}}"#,
            );
            check_to_json(
                Result::<U128, u32>::Err(1),
                r#"{"cl_type":{"Result":{"ok":"U128","err":"U32"}},"parsed_to_json":{"Err":1}}"#,
            );
            check_to_json(
                Result::<U128, ()>::Err(()),
                r#"{"cl_type":{"Result":{"ok":"U128","err":"Unit"}},"parsed_to_json":{"Err":null}}"#,
            );
            check_to_json(
                Result::<U128, String>::Err("e".to_string()),
                r#"{"cl_type":{"Result":{"ok":"U128","err":"String"}},"parsed_to_json":{"Err":"e"}}"#,
            );
        }

        #[test]
        fn u256_cl_value_should_encode_to_json() {
            check_to_json(
                Result::<U256, i32>::Ok(1.into()),
                r#"{"cl_type":{"Result":{"ok":"U256","err":"I32"}},"parsed_to_json":{"Ok":"1"}}"#,
            );
            check_to_json(
                Result::<U256, u32>::Ok(1.into()),
                r#"{"cl_type":{"Result":{"ok":"U256","err":"U32"}},"parsed_to_json":{"Ok":"1"}}"#,
            );
            check_to_json(
                Result::<U256, ()>::Ok(1.into()),
                r#"{"cl_type":{"Result":{"ok":"U256","err":"Unit"}},"parsed_to_json":{"Ok":"1"}}"#,
            );
            check_to_json(
                Result::<U256, String>::Ok(1.into()),
                r#"{"cl_type":{"Result":{"ok":"U256","err":"String"}},"parsed_to_json":{"Ok":"1"}}"#,
            );
            check_to_json(
                Result::<U256, i32>::Err(-1),
                r#"{"cl_type":{"Result":{"ok":"U256","err":"I32"}},"parsed_to_json":{"Err":-1}}"#,
            );
            check_to_json(
                Result::<U256, u32>::Err(1),
                r#"{"cl_type":{"Result":{"ok":"U256","err":"U32"}},"parsed_to_json":{"Err":1}}"#,
            );
            check_to_json(
                Result::<U256, ()>::Err(()),
                r#"{"cl_type":{"Result":{"ok":"U256","err":"Unit"}},"parsed_to_json":{"Err":null}}"#,
            );
            check_to_json(
                Result::<U256, String>::Err("e".to_string()),
                r#"{"cl_type":{"Result":{"ok":"U256","err":"String"}},"parsed_to_json":{"Err":"e"}}"#,
            );
        }

        #[test]
        fn u512_cl_value_should_encode_to_json() {
            check_to_json(
                Result::<U512, i32>::Ok(1.into()),
                r#"{"cl_type":{"Result":{"ok":"U512","err":"I32"}},"parsed_to_json":{"Ok":"1"}}"#,
            );
            check_to_json(
                Result::<U512, u32>::Ok(1.into()),
                r#"{"cl_type":{"Result":{"ok":"U512","err":"U32"}},"parsed_to_json":{"Ok":"1"}}"#,
            );
            check_to_json(
                Result::<U512, ()>::Ok(1.into()),
                r#"{"cl_type":{"Result":{"ok":"U512","err":"Unit"}},"parsed_to_json":{"Ok":"1"}}"#,
            );
            check_to_json(
                Result::<U512, String>::Ok(1.into()),
                r#"{"cl_type":{"Result":{"ok":"U512","err":"String"}},"parsed_to_json":{"Ok":"1"}}"#,
            );
            check_to_json(
                Result::<U512, i32>::Err(-1),
                r#"{"cl_type":{"Result":{"ok":"U512","err":"I32"}},"parsed_to_json":{"Err":-1}}"#,
            );
            check_to_json(
                Result::<U512, u32>::Err(1),
                r#"{"cl_type":{"Result":{"ok":"U512","err":"U32"}},"parsed_to_json":{"Err":1}}"#,
            );
            check_to_json(
                Result::<U512, ()>::Err(()),
                r#"{"cl_type":{"Result":{"ok":"U512","err":"Unit"}},"parsed_to_json":{"Err":null}}"#,
            );
            check_to_json(
                Result::<U512, String>::Err("e".to_string()),
                r#"{"cl_type":{"Result":{"ok":"U512","err":"String"}},"parsed_to_json":{"Err":"e"}}"#,
            );
        }

        #[test]
        fn unit_cl_value_should_encode_to_json() {
            check_to_json(
                Result::<(), i32>::Ok(()),
                r#"{"cl_type":{"Result":{"ok":"Unit","err":"I32"}},"parsed_to_json":{"Ok":null}}"#,
            );
            check_to_json(
                Result::<(), u32>::Ok(()),
                r#"{"cl_type":{"Result":{"ok":"Unit","err":"U32"}},"parsed_to_json":{"Ok":null}}"#,
            );
            check_to_json(
                Result::<(), ()>::Ok(()),
                r#"{"cl_type":{"Result":{"ok":"Unit","err":"Unit"}},"parsed_to_json":{"Ok":null}}"#,
            );
            check_to_json(
                Result::<(), String>::Ok(()),
                r#"{"cl_type":{"Result":{"ok":"Unit","err":"String"}},"parsed_to_json":{"Ok":null}}"#,
            );
            check_to_json(
                Result::<(), i32>::Err(-1),
                r#"{"cl_type":{"Result":{"ok":"Unit","err":"I32"}},"parsed_to_json":{"Err":-1}}"#,
            );
            check_to_json(
                Result::<(), u32>::Err(1),
                r#"{"cl_type":{"Result":{"ok":"Unit","err":"U32"}},"parsed_to_json":{"Err":1}}"#,
            );
            check_to_json(
                Result::<(), ()>::Err(()),
                r#"{"cl_type":{"Result":{"ok":"Unit","err":"Unit"}},"parsed_to_json":{"Err":null}}"#,
            );
            check_to_json(
                Result::<(), String>::Err("e".to_string()),
                r#"{"cl_type":{"Result":{"ok":"Unit","err":"String"}},"parsed_to_json":{"Err":"e"}}"#,
            );
        }

        #[test]
        fn string_cl_value_should_encode_to_json() {
            check_to_json(
                Result::<String, i32>::Ok("test string".to_string()),
                r#"{"cl_type":{"Result":{"ok":"String","err":"I32"}},"parsed_to_json":{"Ok":"test string"}}"#,
            );
            check_to_json(
                Result::<String, u32>::Ok("test string".to_string()),
                r#"{"cl_type":{"Result":{"ok":"String","err":"U32"}},"parsed_to_json":{"Ok":"test string"}}"#,
            );
            check_to_json(
                Result::<String, ()>::Ok("test string".to_string()),
                r#"{"cl_type":{"Result":{"ok":"String","err":"Unit"}},"parsed_to_json":{"Ok":"test string"}}"#,
            );
            check_to_json(
                Result::<String, String>::Ok("test string".to_string()),
                r#"{"cl_type":{"Result":{"ok":"String","err":"String"}},"parsed_to_json":{"Ok":"test string"}}"#,
            );
            check_to_json(
                Result::<String, i32>::Err(-1),
                r#"{"cl_type":{"Result":{"ok":"String","err":"I32"}},"parsed_to_json":{"Err":-1}}"#,
            );
            check_to_json(
                Result::<String, u32>::Err(1),
                r#"{"cl_type":{"Result":{"ok":"String","err":"U32"}},"parsed_to_json":{"Err":1}}"#,
            );
            check_to_json(
                Result::<String, ()>::Err(()),
                r#"{"cl_type":{"Result":{"ok":"String","err":"Unit"}},"parsed_to_json":{"Err":null}}"#,
            );
            check_to_json(
                Result::<String, String>::Err("e".to_string()),
                r#"{"cl_type":{"Result":{"ok":"String","err":"String"}},"parsed_to_json":{"Err":"e"}}"#,
            );
        }

        #[test]
        fn key_cl_value_should_encode_to_json() {
            let key = Key::Hash([2; KEY_HASH_LENGTH]);
            check_to_json(
                Result::<Key, i32>::Ok(key),
                r#"{"cl_type":{"Result":{"ok":"Key","err":"I32"}},"parsed_to_json":{"Ok":{"Hash":"hash-0202020202020202020202020202020202020202020202020202020202020202"}}}"#,
            );
            check_to_json(
                Result::<Key, u32>::Ok(key),
                r#"{"cl_type":{"Result":{"ok":"Key","err":"U32"}},"parsed_to_json":{"Ok":{"Hash":"hash-0202020202020202020202020202020202020202020202020202020202020202"}}}"#,
            );
            check_to_json(
                Result::<Key, ()>::Ok(key),
                r#"{"cl_type":{"Result":{"ok":"Key","err":"Unit"}},"parsed_to_json":{"Ok":{"Hash":"hash-0202020202020202020202020202020202020202020202020202020202020202"}}}"#,
            );
            check_to_json(
                Result::<Key, String>::Ok(key),
                r#"{"cl_type":{"Result":{"ok":"Key","err":"String"}},"parsed_to_json":{"Ok":{"Hash":"hash-0202020202020202020202020202020202020202020202020202020202020202"}}}"#,
            );
            check_to_json(
                Result::<Key, i32>::Err(-1),
                r#"{"cl_type":{"Result":{"ok":"Key","err":"I32"}},"parsed_to_json":{"Err":-1}}"#,
            );
            check_to_json(
                Result::<Key, u32>::Err(1),
                r#"{"cl_type":{"Result":{"ok":"Key","err":"U32"}},"parsed_to_json":{"Err":1}}"#,
            );
            check_to_json(
                Result::<Key, ()>::Err(()),
                r#"{"cl_type":{"Result":{"ok":"Key","err":"Unit"}},"parsed_to_json":{"Err":null}}"#,
            );
            check_to_json(
                Result::<Key, String>::Err("e".to_string()),
                r#"{"cl_type":{"Result":{"ok":"Key","err":"String"}},"parsed_to_json":{"Err":"e"}}"#,
            );
        }

        #[test]
        fn uref_cl_value_should_encode_to_json() {
            let uref = URef::new([6; UREF_ADDR_LENGTH], AccessRights::READ_ADD_WRITE);
            check_to_json(
                Result::<URef, i32>::Ok(uref),
                r#"{"cl_type":{"Result":{"ok":"URef","err":"I32"}},"parsed_to_json":{"Ok":"uref-0606060606060606060606060606060606060606060606060606060606060606-007"}}"#,
            );
            check_to_json(
                Result::<URef, u32>::Ok(uref),
                r#"{"cl_type":{"Result":{"ok":"URef","err":"U32"}},"parsed_to_json":{"Ok":"uref-0606060606060606060606060606060606060606060606060606060606060606-007"}}"#,
            );
            check_to_json(
                Result::<URef, ()>::Ok(uref),
                r#"{"cl_type":{"Result":{"ok":"URef","err":"Unit"}},"parsed_to_json":{"Ok":"uref-0606060606060606060606060606060606060606060606060606060606060606-007"}}"#,
            );
            check_to_json(
                Result::<URef, String>::Ok(uref),
                r#"{"cl_type":{"Result":{"ok":"URef","err":"String"}},"parsed_to_json":{"Ok":"uref-0606060606060606060606060606060606060606060606060606060606060606-007"}}"#,
            );
            check_to_json(
                Result::<URef, i32>::Err(-1),
                r#"{"cl_type":{"Result":{"ok":"URef","err":"I32"}},"parsed_to_json":{"Err":-1}}"#,
            );
            check_to_json(
                Result::<URef, u32>::Err(1),
                r#"{"cl_type":{"Result":{"ok":"URef","err":"U32"}},"parsed_to_json":{"Err":1}}"#,
            );
            check_to_json(
                Result::<URef, ()>::Err(()),
                r#"{"cl_type":{"Result":{"ok":"URef","err":"Unit"}},"parsed_to_json":{"Err":null}}"#,
            );
            check_to_json(
                Result::<URef, String>::Err("e".to_string()),
                r#"{"cl_type":{"Result":{"ok":"URef","err":"String"}},"parsed_to_json":{"Err":"e"}}"#,
            );
        }

        #[test]
        fn public_key_cl_value_should_encode_to_json() {
            let public_key = SecretKey::secp256k1([8; SecretKey::SECP256K1_LENGTH]).into();
            check_to_json(
                Result::<PublicKey, i32>::Ok(public_key),
                r#"{"cl_type":{"Result":{"ok":"PublicKey","err":"I32"}},"parsed_to_json":{"Ok":"0203f991f944d1e1954a7fc8b9bf62e0d78f015f4c07762d505e20e6c45260a3661b"}}"#,
            );
            check_to_json(
                Result::<PublicKey, u32>::Ok(public_key),
                r#"{"cl_type":{"Result":{"ok":"PublicKey","err":"U32"}},"parsed_to_json":{"Ok":"0203f991f944d1e1954a7fc8b9bf62e0d78f015f4c07762d505e20e6c45260a3661b"}}"#,
            );
            check_to_json(
                Result::<PublicKey, ()>::Ok(public_key),
                r#"{"cl_type":{"Result":{"ok":"PublicKey","err":"Unit"}},"parsed_to_json":{"Ok":"0203f991f944d1e1954a7fc8b9bf62e0d78f015f4c07762d505e20e6c45260a3661b"}}"#,
            );
            check_to_json(
                Result::<PublicKey, String>::Ok(public_key),
                r#"{"cl_type":{"Result":{"ok":"PublicKey","err":"String"}},"parsed_to_json":{"Ok":"0203f991f944d1e1954a7fc8b9bf62e0d78f015f4c07762d505e20e6c45260a3661b"}}"#,
            );
            check_to_json(
                Result::<PublicKey, i32>::Err(-1),
                r#"{"cl_type":{"Result":{"ok":"PublicKey","err":"I32"}},"parsed_to_json":{"Err":-1}}"#,
            );
            check_to_json(
                Result::<PublicKey, u32>::Err(1),
                r#"{"cl_type":{"Result":{"ok":"PublicKey","err":"U32"}},"parsed_to_json":{"Err":1}}"#,
            );
            check_to_json(
                Result::<PublicKey, ()>::Err(()),
                r#"{"cl_type":{"Result":{"ok":"PublicKey","err":"Unit"}},"parsed_to_json":{"Err":null}}"#,
            );
            check_to_json(
                Result::<PublicKey, String>::Err("e".to_string()),
                r#"{"cl_type":{"Result":{"ok":"PublicKey","err":"String"}},"parsed_to_json":{"Err":"e"}}"#,
            );
        }
    }
}
