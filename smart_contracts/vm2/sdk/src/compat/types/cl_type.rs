use crate::prelude::{
    collections::{BTreeMap, BTreeSet, LinkedList},
    *,
};

#[cfg(any(feature = "std", feature = "hashbrown"))]
use crate::prelude::collections::HashMap;

use crate::serializers::borsh::{
    io::{self, Read},
    BorshDeserialize, BorshSerialize,
};

const MAX_DEPTH: usize = 128;
const ERROR_NESTING_TOO_DEEP: &str = "CLType nesting exceeds maximum depth of 128";
const ERROR_INVALID_CLTYPE_TAG: &str = "Invalid CLType tag";

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

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, BorshSerialize, Debug)]
#[repr(u8)]
#[borsh(use_discriminant = true)]
pub enum CLType {
    /// `bool` primitive.
    Bool = CL_TYPE_TAG_BOOL,
    /// `i32` primitive.
    I32 = CL_TYPE_TAG_I32,
    /// `i64` primitive.
    I64 = CL_TYPE_TAG_I64,
    /// `u8` primitive.
    U8 = CL_TYPE_TAG_U8,
    /// `u32` primitive.
    U32 = CL_TYPE_TAG_U32,
    /// `u64` primitive.
    U64 = CL_TYPE_TAG_U64,
    /// `U128` large unsigned integer type.
    U128 = CL_TYPE_TAG_U128,
    /// `U256` large unsigned integer type.
    U256 = CL_TYPE_TAG_U256,
    /// `U512` large unsigned integer type.
    U512 = CL_TYPE_TAG_U512,
    /// `()` primitive.
    Unit = CL_TYPE_TAG_UNIT,
    /// `String` primitive.
    String = CL_TYPE_TAG_STRING,
    /// `Key` system type.
    Key = CL_TYPE_TAG_KEY,
    /// `URef` system type.
    URef = CL_TYPE_TAG_UREF,
    /// `Option` of a `CLType`.
    Option(Box<CLType>) = CL_TYPE_TAG_OPTION,
    /// Variable-length list of a single `CLType` (comparable to a `Vec`).
    List(Box<CLType>) = CL_TYPE_TAG_LIST,
    /// Fixed-length list of a single `CLType` (comparable to a Rust array).
    ByteArray(u32) = CL_TYPE_TAG_BYTE_ARRAY,
    /// `Result` with `Ok` and `Err` variants of `CLType`s.
    Result { ok: Box<CLType>, err: Box<CLType> } = CL_TYPE_TAG_RESULT,
    /// Map with keys of a single `CLType` and values of a single `CLType`.
    Map {
        key: Box<CLType>,
        value: Box<CLType>,
    } = CL_TYPE_TAG_MAP,
    /// 1-ary tuple of a `CLType`.
    Tuple1([Box<CLType>; 1]) = CL_TYPE_TAG_TUPLE1,
    /// 2-ary tuple of `CLType`s.
    Tuple2([Box<CLType>; 2]) = CL_TYPE_TAG_TUPLE2,
    /// 3-ary tuple of `CLType`s.
    Tuple3([Box<CLType>; 3]) = CL_TYPE_TAG_TUPLE3,
    /// Unspecified type.
    Any = CL_TYPE_TAG_ANY,
    /// [`PublicKey`](crate::types::PublicKey) system type.
    PublicKey = CL_TYPE_TAG_PUBLIC_KEY,
}

pub trait CLTyped {
    /// The `CLType` of `Self`.
    fn cl_type() -> CLType;
}

macro_rules! impl_cltyped_for {
    ($($ty:ty => $clty:expr),* $(,)?) => {
        $(
            impl CLTyped for $ty {
                fn cl_type() -> CLType {
                    $clty
                }
            }
        )*
    };
}

impl_cltyped_for! {
    bool => CLType::Bool,
    i8 => CLType::Any, // No variant exists
    i16 => CLType::Any, // No variant exists
    i32 => CLType::I32,
    i64 => CLType::I64,
    i128 => CLType::Any, // No variant exists
    u8 => CLType::U8,
    u16 => CLType::Any, // No variant exists
    u32 => CLType::U32,
    u64 => CLType::U64,
    u128 => CLType::U128,
    crate::types::U256 => CLType::U256,
    super::U512 => CLType::U512,
    () => CLType::Unit,
    str => CLType::String,
    String => CLType::String,
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

impl<T: CLTyped> CLTyped for LinkedList<T> {
    fn cl_type() -> CLType {
        CLType::List(Box::new(T::cl_type()))
    }
}

impl<T: CLTyped> CLTyped for BTreeSet<T> {
    fn cl_type() -> CLType {
        CLType::List(Box::new(T::cl_type()))
    }
}

impl<T: ?Sized + CLTyped> CLTyped for &T {
    fn cl_type() -> CLType {
        T::cl_type()
    }
}

impl<const COUNT: usize> CLTyped for [u8; COUNT] {
    fn cl_type() -> CLType {
        CLType::ByteArray(COUNT as u32)
    }
}

impl<const COUNT: usize> CLTyped for [u16; COUNT] {
    fn cl_type() -> CLType {
        CLType::ByteArray((COUNT * 2) as u32)
    }
}

impl<const COUNT: usize> CLTyped for [u32; COUNT] {
    fn cl_type() -> CLType {
        CLType::ByteArray((COUNT * 4) as u32)
    }
}

impl<const COUNT: usize> CLTyped for [u64; COUNT] {
    fn cl_type() -> CLType {
        CLType::ByteArray((COUNT * 8) as u32)
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

#[cfg(any(feature = "std", feature = "hashbrown"))]
impl<K: CLTyped, V: CLTyped> CLTyped for HashMap<K, V> {
    fn cl_type() -> CLType {
        let key = Box::new(K::cl_type());
        let value = Box::new(V::cl_type());
        CLType::Map { key, value }
    }
}

impl<T: CLTyped> CLTyped for Box<T> {
    fn cl_type() -> CLType {
        T::cl_type()
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

impl<T1, T2, T3, T4> CLTyped for (T1, T2, T3, T4) {
    fn cl_type() -> CLType {
        CLType::Any
    }
}
impl<T1, T2, T3, T4, T5> CLTyped for (T1, T2, T3, T4, T5) {
    fn cl_type() -> CLType {
        CLType::Any
    }
}
impl<T1, T2, T3, T4, T5, T6> CLTyped for (T1, T2, T3, T4, T5, T6) {
    fn cl_type() -> CLType {
        CLType::Any
    }
}
impl<T1, T2, T3, T4, T5, T6, T7> CLTyped for (T1, T2, T3, T4, T5, T6, T7) {
    fn cl_type() -> CLType {
        CLType::Any
    }
}
impl<T1, T2, T3, T4, T5, T6, T7, T8> CLTyped for (T1, T2, T3, T4, T5, T6, T7, T8) {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

/// Helper types for simulating recursion
enum FrameKind {
    Option,
    List,
    Result,
    Map,
    Tuple1,
    Tuple2,
    Tuple3,
}

struct Frame {
    /// Kind of the current frame.
    kind: FrameKind,
    /// Children of the current frame.
    children: Vec<CLType>,
    /// Number of expected children for this frame.
    expected: usize,
}

impl BorshDeserialize for CLType {
    fn deserialize_reader<R: Read>(reader: &mut R) -> crate::serializers::borsh::io::Result<Self> {
        let mut stack: Vec<Frame> = Vec::new();

        // 'current' holds the last parsed CLType.
        let mut current: Option<CLType> = None;

        loop {
            if current.is_none() {
                let mut tag_buf = [0u8; 1];
                reader.read_exact(&mut tag_buf)?;
                let tag = tag_buf[0];
                current = match tag {
                    CL_TYPE_TAG_BOOL => Some(CLType::Bool),
                    CL_TYPE_TAG_I32 => Some(CLType::I32),
                    CL_TYPE_TAG_I64 => Some(CLType::I64),
                    CL_TYPE_TAG_U8 => Some(CLType::U8),
                    CL_TYPE_TAG_U32 => Some(CLType::U32),
                    CL_TYPE_TAG_U64 => Some(CLType::U64),
                    CL_TYPE_TAG_U128 => Some(CLType::U128),
                    CL_TYPE_TAG_U256 => Some(CLType::U256),
                    CL_TYPE_TAG_U512 => Some(CLType::U512),
                    CL_TYPE_TAG_UNIT => Some(CLType::Unit),
                    CL_TYPE_TAG_STRING => Some(CLType::String),
                    CL_TYPE_TAG_KEY => Some(CLType::Key),
                    CL_TYPE_TAG_UREF => Some(CLType::URef),
                    CL_TYPE_TAG_ANY => Some(CLType::Any),
                    CL_TYPE_TAG_PUBLIC_KEY => Some(CLType::PublicKey),
                    // For types carrying inner types, push a frame and delay construction.
                    CL_TYPE_TAG_OPTION => {
                        if stack.len() >= MAX_DEPTH {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                ERROR_NESTING_TOO_DEEP,
                            ));
                        }
                        stack.push(Frame {
                            kind: FrameKind::Option,
                            children: Vec::with_capacity(1),
                            expected: 1,
                        });
                        None
                    }
                    CL_TYPE_TAG_LIST => {
                        if stack.len() >= MAX_DEPTH {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                ERROR_NESTING_TOO_DEEP,
                            ));
                        }
                        stack.push(Frame {
                            kind: FrameKind::List,
                            children: Vec::with_capacity(1),
                            expected: 1,
                        });
                        None
                    }
                    CL_TYPE_TAG_RESULT => {
                        if stack.len() >= MAX_DEPTH {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                ERROR_NESTING_TOO_DEEP,
                            ));
                        }
                        stack.push(Frame {
                            kind: FrameKind::Result,
                            children: Vec::with_capacity(2),
                            expected: 2,
                        });
                        None
                    }
                    CL_TYPE_TAG_MAP => {
                        if stack.len() >= MAX_DEPTH {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                ERROR_NESTING_TOO_DEEP,
                            ));
                        }
                        stack.push(Frame {
                            kind: FrameKind::Map,
                            children: Vec::with_capacity(2),
                            expected: 2,
                        });
                        None
                    }
                    CL_TYPE_TAG_TUPLE1 => {
                        if stack.len() >= MAX_DEPTH {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                ERROR_NESTING_TOO_DEEP,
                            ));
                        }
                        stack.push(Frame {
                            kind: FrameKind::Tuple1,
                            children: Vec::with_capacity(1),
                            expected: 1,
                        });
                        None
                    }
                    CL_TYPE_TAG_TUPLE2 => {
                        if stack.len() >= MAX_DEPTH {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                ERROR_NESTING_TOO_DEEP,
                            ));
                        }
                        stack.push(Frame {
                            kind: FrameKind::Tuple2,
                            children: Vec::with_capacity(2),
                            expected: 2,
                        });
                        None
                    }
                    CL_TYPE_TAG_TUPLE3 => {
                        if stack.len() >= MAX_DEPTH {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                ERROR_NESTING_TOO_DEEP,
                            ));
                        }
                        stack.push(Frame {
                            kind: FrameKind::Tuple3,
                            children: Vec::with_capacity(3),
                            expected: 3,
                        });
                        None
                    }
                    CL_TYPE_TAG_BYTE_ARRAY => {
                        let mut buf = [0u8; 4];
                        reader.read_exact(&mut buf)?;
                        let len = u32::from_le_bytes(buf);
                        Some(CLType::ByteArray(len))
                    }
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            ERROR_INVALID_CLTYPE_TAG,
                        ))
                    }
                };
                continue;
            }

            // If there's a pending frame, attach the current node as a child.
            match stack.pop() {
                Some(mut frame) => {
                    frame.children.push(current.take().unwrap());
                    if frame.children.len() < frame.expected {
                        // Still waiting for more children; push the frame back.
                        stack.push(frame);
                        continue;
                    } else {
                        // Frame is complete; build the composite CLType.
                        current = Some(match frame.kind {
                            FrameKind::Option => {
                                CLType::Option(Box::new(frame.children.into_iter().next().unwrap()))
                            }
                            FrameKind::List => {
                                CLType::List(Box::new(frame.children.into_iter().next().unwrap()))
                            }
                            FrameKind::Result => {
                                let mut iter = frame.children.into_iter();
                                let ok = iter.next().unwrap();
                                let err = iter.next().unwrap();
                                CLType::Result {
                                    ok: Box::new(ok),
                                    err: Box::new(err),
                                }
                            }
                            FrameKind::Map => {
                                let mut iter = frame.children.into_iter();
                                let key = iter.next().unwrap();
                                let value = iter.next().unwrap();
                                CLType::Map {
                                    key: Box::new(key),
                                    value: Box::new(value),
                                }
                            }
                            FrameKind::Tuple1 => {
                                let first = frame.children.into_iter().next().unwrap();
                                CLType::Tuple1([Box::new(first)])
                            }
                            FrameKind::Tuple2 => {
                                let mut iter = frame.children.into_iter();
                                let first = iter.next().unwrap();
                                let second = iter.next().unwrap();
                                CLType::Tuple2([Box::new(first), Box::new(second)])
                            }
                            FrameKind::Tuple3 => {
                                let mut iter = frame.children.into_iter();
                                let first = iter.next().unwrap();
                                let second = iter.next().unwrap();
                                let third = iter.next().unwrap();
                                CLType::Tuple3([Box::new(first), Box::new(second), Box::new(third)])
                            }
                        });
                        continue;
                    }
                }
                None => {
                    // No pending frames; the current node is the top-level value.
                    break;
                }
            }
        }

        debug_assert!(
            stack.is_empty(),
            "Stack should be empty at the end of parsing"
        );

        Ok(current.unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::iter;

    #[test]
    fn parsing_deeply_nested_types_should_fail_at_depth_limit() {
        let depth = MAX_DEPTH;
        // Test that parsing fails when depth exceeds 128
        let bytes: Vec<u8> = iter::empty()
            .chain(iter::repeat(CL_TYPE_TAG_TUPLE1).take(depth + 1)) // 129 levels of Tuple1 nesting
            .chain(iter::once(CL_TYPE_TAG_UNIT)) // Unit type at the end
            .collect();

        let result: Result<CLType, _> = borsh::from_slice(&bytes);
        assert!(result.is_err());

        // Test that parsing succeeds at exactly the limit
        let bytes_at_limit: Vec<u8> = iter::empty()
            .chain(iter::repeat(CL_TYPE_TAG_TUPLE1).take(depth)) // 128 levels of Tuple1 nesting
            .chain(iter::once(CL_TYPE_TAG_UNIT)) // Unit type at the end
            .collect();

        let result_at_limit: Result<CLType, _> = borsh::from_slice(&bytes_at_limit);
        assert!(result_at_limit.is_ok());
    }

    #[test]
    fn str_also_implements_cltyped() {
        assert_eq!(<str>::cl_type(), <String>::cl_type());
        assert_eq!(<&str>::cl_type(), <String>::cl_type());
    }
}
