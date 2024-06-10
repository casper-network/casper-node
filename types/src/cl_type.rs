use alloc::{
    boxed::Box,
    collections::{BTreeMap, BTreeSet, VecDeque},
    string::String,
    vec::Vec,
};
use core::{
    fmt::{self, Display, Formatter},
    mem,
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use num_rational::Ratio;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Key, URef, U128, U256, U512,
};

// This must be less than 300 in order to avoid a stack overflow when deserializing.
pub(crate) const CL_TYPE_RECURSION_DEPTH: u8 = 50;

const CL_TYPE_TAG_BOOL: u8 = 0;
const CL_TYPE_TAG_I32: u8 = 1;
const CL_TYPE_TAG_I64: u8 = 2;
const CL_TYPE_TAG_U8: u8 = 3;
const CL_TYPE_TAG_U32: u8 = 4;
const CL_TYPE_TAG_U64: u8 = 5;
const CL_TYPE_TAG_U128: u8 = 6;
const CL_TYPE_TAG_U256: u8 = 7;
const CL_TYPE_TAG_U512: u8 = 8;
const CL_TYPE_TAG_UNIT: u8 = 9;
const CL_TYPE_TAG_STRING: u8 = 10;
const CL_TYPE_TAG_KEY: u8 = 11;
const CL_TYPE_TAG_UREF: u8 = 12;
const CL_TYPE_TAG_OPTION: u8 = 13;
const CL_TYPE_TAG_LIST: u8 = 14;
const CL_TYPE_TAG_BYTE_ARRAY: u8 = 15;
const CL_TYPE_TAG_RESULT: u8 = 16;
const CL_TYPE_TAG_MAP: u8 = 17;
const CL_TYPE_TAG_TUPLE1: u8 = 18;
const CL_TYPE_TAG_TUPLE2: u8 = 19;
const CL_TYPE_TAG_TUPLE3: u8 = 20;
const CL_TYPE_TAG_ANY: u8 = 21;
const CL_TYPE_TAG_PUBLIC_KEY: u8 = 22;

/// Casper types, i.e. types which can be stored and manipulated by smart contracts.
///
/// Provides a description of the underlying data type of a [`CLValue`](crate::CLValue).
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub enum CLType {
    /// `bool` primitive.
    Bool,
    /// `i32` primitive.
    I32,
    /// `i64` primitive.
    I64,
    /// `u8` primitive.
    U8,
    /// `u32` primitive.
    U32,
    /// `u64` primitive.
    U64,
    /// [`U128`] large unsigned integer type.
    U128,
    /// [`U256`] large unsigned integer type.
    U256,
    /// [`U512`] large unsigned integer type.
    U512,
    /// `()` primitive.
    Unit,
    /// `String` primitive.
    String,
    /// [`Key`] system type.
    Key,
    /// [`URef`] system type.
    URef,
    /// [`PublicKey`](crate::PublicKey) system type.
    PublicKey,
    /// `Option` of a `CLType`.
    #[cfg_attr(feature = "datasize", data_size(skip))]
    Option(Box<CLType>),
    /// Variable-length list of a single `CLType` (comparable to a `Vec`).
    #[cfg_attr(feature = "datasize", data_size(skip))]
    List(Box<CLType>),
    /// Fixed-length list of a single `CLType` (comparable to a Rust array).
    ByteArray(u32),
    /// `Result` with `Ok` and `Err` variants of `CLType`s.
    #[allow(missing_docs)] // generated docs are explicit enough.
    #[cfg_attr(feature = "datasize", data_size(skip))]
    Result { ok: Box<CLType>, err: Box<CLType> },
    /// Map with keys of a single `CLType` and values of a single `CLType`.
    #[allow(missing_docs)] // generated docs are explicit enough.
    #[cfg_attr(feature = "datasize", data_size(skip))]
    Map {
        key: Box<CLType>,
        value: Box<CLType>,
    },
    /// 1-ary tuple of a `CLType`.
    #[cfg_attr(feature = "datasize", data_size(skip))]
    Tuple1([Box<CLType>; 1]),
    /// 2-ary tuple of `CLType`s.
    #[cfg_attr(feature = "datasize", data_size(skip))]
    Tuple2([Box<CLType>; 2]),
    /// 3-ary tuple of `CLType`s.
    #[cfg_attr(feature = "datasize", data_size(skip))]
    Tuple3([Box<CLType>; 3]),
    /// Unspecified type.
    Any,
}

impl CLType {
    /// The `len()` of the `Vec<u8>` resulting from `self.to_bytes()`.
    pub fn serialized_length(&self) -> usize {
        mem::size_of::<u8>()
            + match self {
                CLType::Bool
                | CLType::I32
                | CLType::I64
                | CLType::U8
                | CLType::U32
                | CLType::U64
                | CLType::U128
                | CLType::U256
                | CLType::U512
                | CLType::Unit
                | CLType::String
                | CLType::Key
                | CLType::URef
                | CLType::PublicKey
                | CLType::Any => 0,
                CLType::Option(cl_type) | CLType::List(cl_type) => cl_type.serialized_length(),
                CLType::ByteArray(list_len) => list_len.serialized_length(),
                CLType::Result { ok, err } => ok.serialized_length() + err.serialized_length(),
                CLType::Map { key, value } => key.serialized_length() + value.serialized_length(),
                CLType::Tuple1(cl_type_array) => serialized_length_of_cl_tuple_type(cl_type_array),
                CLType::Tuple2(cl_type_array) => serialized_length_of_cl_tuple_type(cl_type_array),
                CLType::Tuple3(cl_type_array) => serialized_length_of_cl_tuple_type(cl_type_array),
            }
    }

    /// Returns `true` if the [`CLType`] is [`Option`].
    pub fn is_option(&self) -> bool {
        matches!(self, Self::Option(..))
    }

    /// Creates a `CLType::Map`.
    pub fn map(key: CLType, value: CLType) -> Self {
        CLType::Map {
            key: Box::new(key),
            value: Box::new(value),
        }
    }
}

/// Returns the `CLType` describing a "named key" on the system, i.e. a `(String, Key)`.
pub fn named_key_type() -> CLType {
    CLType::Tuple2([Box::new(CLType::String), Box::new(CLType::Key)])
}

impl CLType {
    pub(crate) fn append_bytes(&self, stream: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            CLType::Bool => stream.push(CL_TYPE_TAG_BOOL),
            CLType::I32 => stream.push(CL_TYPE_TAG_I32),
            CLType::I64 => stream.push(CL_TYPE_TAG_I64),
            CLType::U8 => stream.push(CL_TYPE_TAG_U8),
            CLType::U32 => stream.push(CL_TYPE_TAG_U32),
            CLType::U64 => stream.push(CL_TYPE_TAG_U64),
            CLType::U128 => stream.push(CL_TYPE_TAG_U128),
            CLType::U256 => stream.push(CL_TYPE_TAG_U256),
            CLType::U512 => stream.push(CL_TYPE_TAG_U512),
            CLType::Unit => stream.push(CL_TYPE_TAG_UNIT),
            CLType::String => stream.push(CL_TYPE_TAG_STRING),
            CLType::Key => stream.push(CL_TYPE_TAG_KEY),
            CLType::URef => stream.push(CL_TYPE_TAG_UREF),
            CLType::PublicKey => stream.push(CL_TYPE_TAG_PUBLIC_KEY),
            CLType::Option(cl_type) => {
                stream.push(CL_TYPE_TAG_OPTION);
                cl_type.append_bytes(stream)?;
            }
            CLType::List(cl_type) => {
                stream.push(CL_TYPE_TAG_LIST);
                cl_type.append_bytes(stream)?;
            }
            CLType::ByteArray(len) => {
                stream.push(CL_TYPE_TAG_BYTE_ARRAY);
                stream.append(&mut len.to_bytes()?);
            }
            CLType::Result { ok, err } => {
                stream.push(CL_TYPE_TAG_RESULT);
                ok.append_bytes(stream)?;
                err.append_bytes(stream)?;
            }
            CLType::Map { key, value } => {
                stream.push(CL_TYPE_TAG_MAP);
                key.append_bytes(stream)?;
                value.append_bytes(stream)?;
            }
            CLType::Tuple1(cl_type_array) => {
                serialize_cl_tuple_type(CL_TYPE_TAG_TUPLE1, cl_type_array, stream)?
            }
            CLType::Tuple2(cl_type_array) => {
                serialize_cl_tuple_type(CL_TYPE_TAG_TUPLE2, cl_type_array, stream)?
            }
            CLType::Tuple3(cl_type_array) => {
                serialize_cl_tuple_type(CL_TYPE_TAG_TUPLE3, cl_type_array, stream)?
            }
            CLType::Any => stream.push(CL_TYPE_TAG_ANY),
        }
        Ok(())
    }
}

impl Display for CLType {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            CLType::Bool => write!(formatter, "bool"),
            CLType::I32 => write!(formatter, "i32"),
            CLType::I64 => write!(formatter, "i64"),
            CLType::U8 => write!(formatter, "u8"),
            CLType::U32 => write!(formatter, "u32"),
            CLType::U64 => write!(formatter, "u64"),
            CLType::U128 => write!(formatter, "u128"),
            CLType::U256 => write!(formatter, "u256"),
            CLType::U512 => write!(formatter, "u512"),
            CLType::Unit => write!(formatter, "unit"),
            CLType::String => write!(formatter, "string"),
            CLType::Key => write!(formatter, "key"),
            CLType::URef => write!(formatter, "uref"),
            CLType::PublicKey => write!(formatter, "public-key"),
            CLType::Option(t) => write!(formatter, "option<{t}>"),
            CLType::List(t) => write!(formatter, "list<{t}>"),
            CLType::ByteArray(len) => write!(formatter, "byte-array[{len}]"),
            CLType::Result { ok, err } => write!(formatter, "result<{ok}, {err}>"),
            CLType::Map { key, value } => write!(formatter, "map<{key}, {value}>"),
            CLType::Tuple1([t1]) => write!(formatter, "({t1},)"),
            CLType::Tuple2([t1, t2]) => write!(formatter, "({t1}, {t2})"),
            CLType::Tuple3([t1, t2, t3]) => write!(formatter, "({t1}, {t2}, {t3})"),
            CLType::Any => write!(formatter, "any"),
        }
    }
}

impl FromBytes for CLType {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        depth_limited_from_bytes(0, bytes)
    }
}

fn depth_limited_from_bytes(depth: u8, bytes: &[u8]) -> Result<(CLType, &[u8]), bytesrepr::Error> {
    if depth >= CL_TYPE_RECURSION_DEPTH {
        return Err(bytesrepr::Error::ExceededRecursionDepth);
    }
    let depth = depth + 1;
    let (tag, remainder) = u8::from_bytes(bytes)?;
    match tag {
        CL_TYPE_TAG_BOOL => Ok((CLType::Bool, remainder)),
        CL_TYPE_TAG_I32 => Ok((CLType::I32, remainder)),
        CL_TYPE_TAG_I64 => Ok((CLType::I64, remainder)),
        CL_TYPE_TAG_U8 => Ok((CLType::U8, remainder)),
        CL_TYPE_TAG_U32 => Ok((CLType::U32, remainder)),
        CL_TYPE_TAG_U64 => Ok((CLType::U64, remainder)),
        CL_TYPE_TAG_U128 => Ok((CLType::U128, remainder)),
        CL_TYPE_TAG_U256 => Ok((CLType::U256, remainder)),
        CL_TYPE_TAG_U512 => Ok((CLType::U512, remainder)),
        CL_TYPE_TAG_UNIT => Ok((CLType::Unit, remainder)),
        CL_TYPE_TAG_STRING => Ok((CLType::String, remainder)),
        CL_TYPE_TAG_KEY => Ok((CLType::Key, remainder)),
        CL_TYPE_TAG_UREF => Ok((CLType::URef, remainder)),
        CL_TYPE_TAG_PUBLIC_KEY => Ok((CLType::PublicKey, remainder)),
        CL_TYPE_TAG_OPTION => {
            let (inner_type, remainder) = depth_limited_from_bytes(depth, remainder)?;
            let cl_type = CLType::Option(Box::new(inner_type));
            Ok((cl_type, remainder))
        }
        CL_TYPE_TAG_LIST => {
            let (inner_type, remainder) = depth_limited_from_bytes(depth, remainder)?;
            let cl_type = CLType::List(Box::new(inner_type));
            Ok((cl_type, remainder))
        }
        CL_TYPE_TAG_BYTE_ARRAY => {
            let (len, remainder) = u32::from_bytes(remainder)?;
            let cl_type = CLType::ByteArray(len);
            Ok((cl_type, remainder))
        }
        CL_TYPE_TAG_RESULT => {
            let (ok_type, remainder) = depth_limited_from_bytes(depth, remainder)?;
            let (err_type, remainder) = depth_limited_from_bytes(depth, remainder)?;
            let cl_type = CLType::Result {
                ok: Box::new(ok_type),
                err: Box::new(err_type),
            };
            Ok((cl_type, remainder))
        }
        CL_TYPE_TAG_MAP => {
            let (key_type, remainder) = depth_limited_from_bytes(depth, remainder)?;
            let (value_type, remainder) = depth_limited_from_bytes(depth, remainder)?;
            let cl_type = CLType::Map {
                key: Box::new(key_type),
                value: Box::new(value_type),
            };
            Ok((cl_type, remainder))
        }
        CL_TYPE_TAG_TUPLE1 => {
            let (mut inner_types, remainder) = parse_cl_tuple_types(depth, 1, remainder)?;
            // NOTE: Assumed safe as `parse_cl_tuple_types` is expected to have exactly 1
            // element
            let cl_type = CLType::Tuple1([inner_types.pop_front().unwrap()]);
            Ok((cl_type, remainder))
        }
        CL_TYPE_TAG_TUPLE2 => {
            let (mut inner_types, remainder) = parse_cl_tuple_types(depth, 2, remainder)?;
            // NOTE: Assumed safe as `parse_cl_tuple_types` is expected to have exactly 2
            // elements
            let cl_type = CLType::Tuple2([
                inner_types.pop_front().unwrap(),
                inner_types.pop_front().unwrap(),
            ]);
            Ok((cl_type, remainder))
        }
        CL_TYPE_TAG_TUPLE3 => {
            let (mut inner_types, remainder) = parse_cl_tuple_types(depth, 3, remainder)?;
            // NOTE: Assumed safe as `parse_cl_tuple_types` is expected to have exactly 3
            // elements
            let cl_type = CLType::Tuple3([
                inner_types.pop_front().unwrap(),
                inner_types.pop_front().unwrap(),
                inner_types.pop_front().unwrap(),
            ]);
            Ok((cl_type, remainder))
        }
        CL_TYPE_TAG_ANY => Ok((CLType::Any, remainder)),
        _ => Err(bytesrepr::Error::Formatting),
    }
}

fn serialize_cl_tuple_type<'a, T: IntoIterator<Item = &'a Box<CLType>>>(
    tag: u8,
    cl_type_array: T,
    stream: &mut Vec<u8>,
) -> Result<(), bytesrepr::Error> {
    stream.push(tag);
    for cl_type in cl_type_array {
        cl_type.append_bytes(stream)?;
    }
    Ok(())
}

fn parse_cl_tuple_types(
    depth: u8,
    count: usize,
    mut bytes: &[u8],
) -> Result<(VecDeque<Box<CLType>>, &[u8]), bytesrepr::Error> {
    let mut cl_types = VecDeque::with_capacity(count);
    for _ in 0..count {
        let (cl_type, remainder) = depth_limited_from_bytes(depth, bytes)?;
        cl_types.push_back(Box::new(cl_type));
        bytes = remainder;
    }

    Ok((cl_types, bytes))
}

fn serialized_length_of_cl_tuple_type<'a, T: IntoIterator<Item = &'a Box<CLType>>>(
    cl_type_array: T,
) -> usize {
    cl_type_array
        .into_iter()
        .map(|cl_type| cl_type.serialized_length())
        .sum()
}

/// A type which can be described as a [`CLType`].
pub trait CLTyped {
    /// The `CLType` of `Self`.
    fn cl_type() -> CLType;
}

impl CLTyped for bool {
    fn cl_type() -> CLType {
        CLType::Bool
    }
}

impl CLTyped for i32 {
    fn cl_type() -> CLType {
        CLType::I32
    }
}

impl CLTyped for i64 {
    fn cl_type() -> CLType {
        CLType::I64
    }
}

impl CLTyped for u8 {
    fn cl_type() -> CLType {
        CLType::U8
    }
}

impl CLTyped for u32 {
    fn cl_type() -> CLType {
        CLType::U32
    }
}

impl CLTyped for u64 {
    fn cl_type() -> CLType {
        CLType::U64
    }
}

impl CLTyped for U128 {
    fn cl_type() -> CLType {
        CLType::U128
    }
}

impl CLTyped for U256 {
    fn cl_type() -> CLType {
        CLType::U256
    }
}

impl CLTyped for U512 {
    fn cl_type() -> CLType {
        CLType::U512
    }
}

impl CLTyped for () {
    fn cl_type() -> CLType {
        CLType::Unit
    }
}

impl CLTyped for String {
    fn cl_type() -> CLType {
        CLType::String
    }
}

impl CLTyped for &str {
    fn cl_type() -> CLType {
        CLType::String
    }
}

impl CLTyped for Key {
    fn cl_type() -> CLType {
        CLType::Key
    }
}

impl CLTyped for URef {
    fn cl_type() -> CLType {
        CLType::URef
    }
}

impl<T: CLTyped> CLTyped for Option<T> {
    fn cl_type() -> CLType {
        CLType::Option(Box::new(T::cl_type()))
    }
}

impl<T: CLTyped> CLTyped for Vec<T> {
    fn cl_type() -> CLType {
        CLType::List(Box::new(T::cl_type()))
    }
}

impl<T: CLTyped> CLTyped for BTreeSet<T> {
    fn cl_type() -> CLType {
        CLType::List(Box::new(T::cl_type()))
    }
}

impl<T: CLTyped> CLTyped for &T {
    fn cl_type() -> CLType {
        T::cl_type()
    }
}

impl<const COUNT: usize> CLTyped for [u8; COUNT] {
    fn cl_type() -> CLType {
        CLType::ByteArray(COUNT as u32)
    }
}

impl<T: CLTyped, E: CLTyped> CLTyped for Result<T, E> {
    fn cl_type() -> CLType {
        let ok = Box::new(T::cl_type());
        let err = Box::new(E::cl_type());
        CLType::Result { ok, err }
    }
}

impl<K: CLTyped, V: CLTyped> CLTyped for BTreeMap<K, V> {
    fn cl_type() -> CLType {
        let key = Box::new(K::cl_type());
        let value = Box::new(V::cl_type());
        CLType::Map { key, value }
    }
}

impl<T1: CLTyped> CLTyped for (T1,) {
    fn cl_type() -> CLType {
        CLType::Tuple1([Box::new(T1::cl_type())])
    }
}

impl<T1: CLTyped, T2: CLTyped> CLTyped for (T1, T2) {
    fn cl_type() -> CLType {
        CLType::Tuple2([Box::new(T1::cl_type()), Box::new(T2::cl_type())])
    }
}

impl<T1: CLTyped, T2: CLTyped, T3: CLTyped> CLTyped for (T1, T2, T3) {
    fn cl_type() -> CLType {
        CLType::Tuple3([
            Box::new(T1::cl_type()),
            Box::new(T2::cl_type()),
            Box::new(T3::cl_type()),
        ])
    }
}

impl<T: CLTyped> CLTyped for Ratio<T> {
    fn cl_type() -> CLType {
        <(T, T)>::cl_type()
    }
}

#[cfg(test)]
mod tests {
    use std::{fmt::Debug, iter, string::ToString};

    use super::*;
    use crate::{
        bytesrepr::{FromBytes, ToBytes},
        AccessRights, CLValue,
    };

    fn round_trip<T: CLTyped + FromBytes + ToBytes + PartialEq + Debug + Clone>(value: &T) {
        let cl_value = CLValue::from_t(value.clone()).unwrap();

        let serialized_cl_value = cl_value.to_bytes().unwrap();
        assert_eq!(serialized_cl_value.len(), cl_value.serialized_length());
        let parsed_cl_value: CLValue = bytesrepr::deserialize(serialized_cl_value).unwrap();
        assert_eq!(cl_value, parsed_cl_value);

        let parsed_value = CLValue::into_t(cl_value).unwrap();
        assert_eq!(*value, parsed_value);
    }

    #[test]
    fn bool_should_work() {
        round_trip(&true);
        round_trip(&false);
    }

    #[test]
    fn u8_should_work() {
        round_trip(&1u8);
    }

    #[test]
    fn u32_should_work() {
        round_trip(&1u32);
    }

    #[test]
    fn i32_should_work() {
        round_trip(&-1i32);
    }

    #[test]
    fn u64_should_work() {
        round_trip(&1u64);
    }

    #[test]
    fn i64_should_work() {
        round_trip(&-1i64);
    }

    #[test]
    fn u128_should_work() {
        round_trip(&U128::one());
    }

    #[test]
    fn u256_should_work() {
        round_trip(&U256::one());
    }

    #[test]
    fn u512_should_work() {
        round_trip(&U512::one());
    }

    #[test]
    fn unit_should_work() {
        round_trip(&());
    }

    #[test]
    fn string_should_work() {
        round_trip(&String::from("abc"));
    }

    #[test]
    fn key_should_work() {
        let key = Key::URef(URef::new([0u8; 32], AccessRights::READ_ADD_WRITE));
        round_trip(&key);
    }

    #[test]
    fn uref_should_work() {
        let uref = URef::new([0u8; 32], AccessRights::READ_ADD_WRITE);
        round_trip(&uref);
    }

    #[test]
    fn option_of_cl_type_should_work() {
        let x: Option<i32> = Some(-1);
        let y: Option<i32> = None;

        round_trip(&x);
        round_trip(&y);
    }

    #[test]
    fn vec_of_cl_type_should_work() {
        let vec = vec![String::from("a"), String::from("b")];
        round_trip(&vec);
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn small_array_of_u8_should_work() {
        macro_rules! test_small_array {
            ($($N:literal)+) => {
                $(
                    let mut array: [u8; $N] = Default::default();
                    for i in 0..$N {
                        array[i] = i as u8;
                    }
                    round_trip(&array);
                )+
            }
        }

        test_small_array! {
                 1  2  3  4  5  6  7  8  9
             10 11 12 13 14 15 16 17 18 19
             20 21 22 23 24 25 26 27 28 29
             30 31 32
        }
    }

    #[test]
    fn large_array_of_cl_type_should_work() {
        macro_rules! test_large_array {
            ($($N:literal)+) => {
                $(
                    let array = {
                        let mut tmp = [0u8; $N];
                        for i in 0..$N {
                            tmp[i] = i as u8;
                        }
                        tmp
                    };

                    let cl_value = CLValue::from_t(array.clone()).unwrap();

                    let serialized_cl_value = cl_value.to_bytes().unwrap();
                    let parsed_cl_value: CLValue = bytesrepr::deserialize(serialized_cl_value).unwrap();
                    assert_eq!(cl_value, parsed_cl_value);

                    let parsed_value: [u8; $N] = CLValue::into_t(cl_value).unwrap();
                    for i in 0..$N {
                        assert_eq!(array[i], parsed_value[i]);
                    }
                )+
            }
        }

        test_large_array! { 64 128 256 512 }
    }

    #[test]
    fn result_of_cl_type_should_work() {
        let x: Result<(), String> = Ok(());
        let y: Result<(), String> = Err(String::from("Hello, world!"));

        round_trip(&x);
        round_trip(&y);
    }

    #[test]
    fn map_of_cl_type_should_work() {
        let mut map: BTreeMap<String, u64> = BTreeMap::new();
        map.insert(String::from("abc"), 1);
        map.insert(String::from("xyz"), 2);

        round_trip(&map);
    }

    #[test]
    fn tuple_1_should_work() {
        let x = (-1i32,);

        round_trip(&x);
    }

    #[test]
    fn tuple_2_should_work() {
        let x = (-1i32, String::from("a"));

        round_trip(&x);
    }

    #[test]
    fn tuple_3_should_work() {
        let x = (-1i32, 1u32, String::from("a"));

        round_trip(&x);
    }

    #[test]
    fn parsing_nested_tuple_1_cltype_should_not_stack_overflow() {
        // The bytesrepr representation of the CLType for a
        // nested (((...((),),...),),) looks like:
        // [18, 18, 18, ..., 9]

        for i in 1..1000 {
            let bytes = iter::repeat(CL_TYPE_TAG_TUPLE1)
                .take(i)
                .chain(iter::once(CL_TYPE_TAG_UNIT))
                .collect();
            match bytesrepr::deserialize(bytes) {
                Ok(parsed_cltype) => assert!(matches!(parsed_cltype, CLType::Tuple1(_))),
                Err(error) => assert_eq!(error, bytesrepr::Error::ExceededRecursionDepth),
            }
        }
    }

    #[test]
    fn parsing_nested_tuple_1_value_should_not_stack_overflow() {
        // The bytesrepr representation of the CLValue for a
        // nested (((...((),),...),),) looks like:
        // [0, 0, 0, 0, 18, 18, 18, ..., 18, 9]

        for i in 1..1000 {
            let bytes = iter::repeat(0)
                .take(4)
                .chain(iter::repeat(CL_TYPE_TAG_TUPLE1).take(i))
                .chain(iter::once(CL_TYPE_TAG_UNIT))
                .collect();
            match bytesrepr::deserialize::<CLValue>(bytes) {
                Ok(parsed_clvalue) => {
                    assert!(matches!(parsed_clvalue.cl_type(), CLType::Tuple1(_)))
                }
                Err(error) => assert_eq!(error, bytesrepr::Error::ExceededRecursionDepth),
            }
        }
    }

    #[test]
    fn any_should_work() {
        #[derive(PartialEq, Debug, Clone)]
        struct Any(String);

        impl CLTyped for Any {
            fn cl_type() -> CLType {
                CLType::Any
            }
        }

        impl ToBytes for Any {
            fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
                self.0.to_bytes()
            }

            fn serialized_length(&self) -> usize {
                self.0.serialized_length()
            }
        }

        impl FromBytes for Any {
            fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
                let (inner, remainder) = String::from_bytes(bytes)?;
                Ok((Any(inner), remainder))
            }
        }

        let any = Any("Any test".to_string());
        round_trip(&any);
    }

    #[test]
    fn should_have_cltype_of_ref_to_cltyped() {
        assert_eq!(<Vec<&u64>>::cl_type(), <Vec<u64>>::cl_type())
    }
}
