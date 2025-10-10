//! A module for computing unique type identifiers (UIDs) via compile-time hashing.
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, LinkedList},
    fmt::{LowerHex, UpperHex},
};
use borsh::{BorshSerialize, BorshDeserialize};

use xxhash_rust::const_xxh32::xxh32;

const TYPE_UID_SEED: u32 = 0;

/// Representation of an [`Uid`] value.
pub type UidRepr = u32;

/// Hashes a byte slice into a 32-bit unsigned integer using xxHash32 with a predefined seed.
const fn hash_bytes(bytes: &[u8]) -> u32 {
    xxh32(bytes, TYPE_UID_SEED)
}

/// A unique identifier for a type, represented as a 64-bit unsigned integer.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize)]
pub struct Uid(UidRepr);

impl From<UidRepr> for Uid {
    fn from(uid: UidRepr) -> Self {
        Uid(uid)
    }
}

impl From<Uid> for UidRepr {
    fn from(uid: Uid) -> Self {
        uid.0
    }
}

impl std::fmt::Display for Uid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{:08x}", self.0)
    }
}

impl LowerHex for Uid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:x}", self.0)
    }
}

impl UpperHex for Uid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:X}", self.0)
    }
}

impl Uid {
    /// The UID for an untyped value, which is zero.
    pub const UNTYPED: Uid = Uid(0);

    /// Creates a new `Uid` from bytes.
    pub const fn from_bytes(bytes: &[u8]) -> Self {
        // Use xxh32 to hash the bytes into a u32
        Uid(hash_bytes(bytes))
    }

    /// Creates a new `Uid` from a string.
    pub const fn from_name(s: &str) -> Self {
        // Use xxh32 to hash the string into a u32
        Uid(hash_bytes(s.as_bytes()))
    }

    /// Creates a new `Uid` from a value.
    ///
    /// This does not involve any hashing and is intended for use with known values.
    #[inline]
    pub const fn new_raw(value: UidRepr) -> Self {
        Uid(value)
    }

    /// Computes a unique identifier for a struct type based on its name and field UIDs.
    pub const fn from_fields(name: &str, fields: &[Uid]) -> Uid {
        // seed the fold with the *name* of the container
        let mut acc = Uid::from_name(name);
        let mut i = 0;
        while i < fields.len() {
            acc = acc.combine(fields[i]);
            i += 1;
        }
        acc
    }

    /// Returns the underlying UID as a `u32`.
    #[inline]
    pub const fn into_raw(&self) -> UidRepr {
        self.0
    }

    /// Combines two `Uid`s into a new `Uid` by hashing their byte representations together.
    pub const fn combine(self, other: Uid) -> Self {
        // 0x01 tag + 16 little-endian bytes = 17-byte buffer
        const TAG: u8 = 0x01;

        let a_bytes: [u8; 4] = self.into_raw().to_le_bytes();
        let b_bytes: [u8; 4] = other.into_raw().to_le_bytes();

        let preimage = [
            // prefixed with a tag byte to make the domains disjoint
            // this is not strictly necessary, because we could leverage xxH32's "seed" parameter,
            // although prefixing with a tag byte is more explicit.
            TAG, // little-endian bytes of `a` and `b`
            a_bytes[0], a_bytes[1], a_bytes[2], a_bytes[3], b_bytes[0], b_bytes[1], b_bytes[2],
            b_bytes[3],
        ];
        Uid::from_bytes(&preimage)
    }
}

/// A trait for types that can provide a unique identifier (UID).
pub trait TypeUid {
    /// Returns the type tag as a `Uid` value.
    const UID: Uid;
}

macro_rules! impl_type_uid {
    (@impl $type:ty) => {
        impl TypeUid for $type {
            const UID: Uid = Uid::from_name(stringify!($type));
        }
    };

    ($($type:ty),+ $(,)?) => {
        $(
            impl_type_uid!(@impl $type);
        )+
    };
}

impl_type_uid!(
    u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, bool, char, String, f32, f64,
);

impl<T: TypeUid> TypeUid for Option<T> {
    const UID: Uid = Uid::from_fields("Option", &[T::UID]);
}

impl<T: TypeUid> TypeUid for Vec<T> {
    const UID: Uid = Uid::from_fields("Vec", &[T::UID]);
}

impl<T: TypeUid> TypeUid for Box<T> {
    const UID: Uid = Uid::from_fields("Box", &[T::UID]);
}

impl<T: TypeUid> TypeUid for [T] {
    const UID: Uid = Uid::from_fields("Slice", &[T::UID]);
}

impl<T: TypeUid, E: TypeUid> TypeUid for Result<T, E> {
    const UID: Uid = Uid::from_fields("Result", &[T::UID, E::UID]);
}

// [T; N]  →  UID = mix( mix( hash("Array"), N ), UID::<T>() )
impl<T: TypeUid, const N: usize> TypeUid for [T; N] {
    const UID: Uid = {
        // 1) start with a leaf hash that means “Array”
        let base = Uid::from_name("Array");

        // 2) bring the length into the Merkle fold
        let with_len = base.combine(Uid(N as u32));

        // 3) finally fold in the element’s fingerprint
        with_len.combine(T::UID)
    };
}

impl<T: TypeUid> TypeUid for LinkedList<T> {
    const UID: Uid = Uid::from_fields("LinkedList", &[T::UID]);
}

impl<T: TypeUid> TypeUid for BTreeSet<T> {
    const UID: Uid = Uid::from_fields("BTreeSet", &[T::UID]);
}

impl<K: TypeUid, V: TypeUid> TypeUid for BTreeMap<K, V> {
    const UID: Uid = Uid::from_fields("BTreeMap", &[K::UID, V::UID]);
}

impl<K: TypeUid, V: TypeUid> TypeUid for HashMap<K, V> {
    const UID: Uid = Uid::from_fields("HashMap", &[K::UID, V::UID]);
}

macro_rules! impl_type_uid_for_tuple {
    ( $prefix:ident: $($name:ident),+ ) => {
        impl< $($name: TypeUid),+ > TypeUid for ( $($name,)+ ) {
            const UID: Uid = Uid::from_fields(stringify!($prefix), &[$($name::UID),+]);
        }
    };
}

impl TypeUid for () {
    const UID: Uid = {
        // We don't call it "Unit" to avoid confusion with the Rust unit type `()`; other languages
        // may not have notion of "unit"
        Uid::from_name("Tuple0")
    };
}

// Implement for tuples of various sizes (from 1-tuple to 12-tuple)
impl_type_uid_for_tuple!(Tuple1: T1);
impl_type_uid_for_tuple!(Tuple2: T1, T2);
impl_type_uid_for_tuple!(Tuple3: T1, T2, T3);
impl_type_uid_for_tuple!(Tuple4: T1, T2, T3, T4);
impl_type_uid_for_tuple!(Tuple5: T1, T2, T3, T4, T5);
impl_type_uid_for_tuple!(Tuple6: T1, T2, T3, T4, T5, T6);
impl_type_uid_for_tuple!(Tuple7: T1, T2, T3, T4, T5, T6, T7);
impl_type_uid_for_tuple!(Tuple8: T1, T2, T3, T4, T5, T6, T7, T8);
impl_type_uid_for_tuple!(Tuple9: T1, T2, T3, T4, T5, T6, T7, T8, T9);
impl_type_uid_for_tuple!(Tuple10: T1, T2, T3, T4, T5, T6, T7, T8, T9, T10);
impl_type_uid_for_tuple!(Tuple11: T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11);
impl_type_uid_for_tuple!(Tuple12: T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12);

impl<T: ?Sized + TypeUid> TypeUid for &T {
    const UID: Uid = T::UID;
}

impl TypeUid for str {
    const UID: Uid = String::UID; // This makes str and &str's UID equivalent to a String's UID. This is intentional as other
                                  // languages may not have a separate string type.
}

impl<const N: usize> TypeUid for bnum::BUint<N> {
    /// The UID for U256 is defined as a constant.
    ///
    /// The UID is computed is equal to UID of a fixed-size array of 32 `u32` elements. This is
    /// consistent with how we handle fixed-size arrays in the SDK, and how U256 is represented in
    /// the schema itself: a fixed-size sequence.
    const UID: Uid = <[u32; N]>::UID;
}

/// Free-standing function that allow syntax of `type_uid::of::<T>`
#[inline(always)]
#[must_use]
pub const fn of<T: TypeUid>() -> Uid {
    T::UID
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mix(a: Uid, b: Uid) -> Uid {
        a.combine(b)
    }

    #[test]
    fn combine() {
        assert_eq!(
            mix(mix(Uid::from_name("Tuple2"), u32::UID), String::UID),
            super::of::<(u32, String)>()
        );
    }

    #[test]
    fn test_type_tag() {
        assert_eq!(u8::UID, Uid::from_name("u8"));
        assert_eq!(String::UID, Uid::from_name("String"));

        // assert_ne!(<(u32, u64)>::UID, <(u64, u32)>::UID, "Ordering of types in tuples matters");
        assert_ne!(<(u64, u32)>::UID, <(u32, u64)>::UID,);

        assert_eq!(
            <(u32, String)>::UID,
            mix(mix(Uid::from_name("Tuple2"), u32::UID), String::UID)
        );
        assert_eq!(
            <(u32, String, u8)>::UID,
            mix(
                mix(mix(Uid::from_name("Tuple3"), u32::UID), String::UID),
                u8::UID
            )
        );

        assert_eq!(
            <[u8; 32]>::UID,
            mix(mix(Uid::from_name("Array"), Uid::from(32u32)), u8::UID)
        );

        assert_ne!(<[u8; 32]>::UID, <[u8; 33]>::UID,);
        assert_ne!(<[u32; 32]>::UID, <[u64; 32]>::UID,);
    }

    #[test]
    fn references_are_equivalent() {
        // This behavior is different to `std::any::TypeId` because in our model the reference to T
        // and T itself are the same types in the metadata description. In other words we
        // don't have different representation of a &String and a String, so we don't need to ensure
        // these types are distinct.
        assert_eq!(<&str>::UID, String::UID,);
        assert_eq!(<&String>::UID, String::UID,);
        assert_eq!(str::UID, <&str>::UID,);

        assert_eq!(<&[u8]>::UID, <[u8]>::UID,);
    }

    #[test]
    fn display() {
        let uid = Uid::new_raw(0xb4dc0d3);
        assert_eq!(uid.to_string(), "0x0b4dc0d3");
        assert_eq!(format!("{:x}", uid), "b4dc0d3");
        assert_eq!(format!("{:X}", uid), "B4DC0D3");
    }
}
