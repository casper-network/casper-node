use xxhash_rust::const_xxh64::xxh64;

const TYPE_UID_SEED: u64 = 0;
/// A unique identifier for a type, represented as a 64-bit unsigned integer.
#[derive(Debug, Copy, Clone, PartialEq, PartialOrd)]
pub struct Uid(u64);

impl From<u64> for Uid {
    fn from(uid: u64) -> Self {
        Uid(uid)
    }
}

impl Uid {
    /// Creates a new `Uid` from bytes.
    pub const fn from_bytes(bytes: &[u8]) -> Self {
        // Use xxh64 to hash the bytes into a u64
        Uid(xxh64(bytes, TYPE_UID_SEED))
    }

    /// Creates a new `Uid` from a string.
    pub const fn from_name(s: &str) -> Self {
        // Use xxh64 to hash the string into a u64
        Uid(xxh64(s.as_bytes(), TYPE_UID_SEED))
    }

    /// Creates a new `Uid` from a value.
    ///
    /// This does not involve any hashing and is intended for use with known values.
    pub const fn from_u64(value: u64) -> Self {
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

    /// Returns the underlying UID as a `u64`.
    pub const fn as_u64(&self) -> u64 {
        self.0
    }

    /// Combines two `Uid`s into a new `Uid` by hashing their byte representations together.
    pub const fn combine(self, other: Uid) -> Self {
        // 0x01 tag + 16 little-endian bytes = 17-byte buffer
        const TAG: u8 = 0x01;

        let a_bytes = self.as_u64().to_le_bytes();
        let b_bytes = other.as_u64().to_le_bytes();

        let preimage = [
            // prefixed with a tag byte to make the domains disjoint
            // this is not strictly necessary, because we could leverage xxH64's "seed" parameter,
            // although prefixing with a tag byte is more explicit.
            TAG, // little-endian bytes of `a` and `b`
            a_bytes[0], a_bytes[1], a_bytes[2], a_bytes[3], a_bytes[4], a_bytes[5], a_bytes[6],
            a_bytes[7], b_bytes[0], b_bytes[1], b_bytes[2], b_bytes[3], b_bytes[4], b_bytes[5],
            b_bytes[6], b_bytes[7],
        ];
        Uid::from_bytes(&preimage)
    }
}

/// A trait for types that can provide a unique identifier (UID).
pub trait TypeUid {
    /// Returns the type tag as a byte slice.
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
    u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize, bool, char, String, str, f32,
    f64,
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
        let with_len = base.combine(Uid(N as u64));

        // 3) finally fold in the element’s fingerprint
        with_len.combine(T::UID)
    };
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

impl<T: TypeUid> TypeUid for &T {
    const UID: Uid = T::UID;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mix(a: Uid, b: Uid) -> Uid {
        a.combine(b)
    }

    #[test]
    fn test_type_tag() {
        assert_eq!(u8::UID, Uid::from_name("u8"));
        assert_eq!(String::UID, Uid::from_name("String"));
        assert_eq!(str::UID, Uid::from_name("str"));

        assert_ne!(<(u64, u32)>::UID, <(u32, u64)>::UID,);

        assert_eq!(
            <(u64, String)>::UID,
            mix(mix(Uid::from_name("Tuple2"), u64::UID), String::UID)
        );
        assert_eq!(
            <(u64, String, u8)>::UID,
            mix(
                mix(mix(Uid::from_name("Tuple3"), u64::UID), String::UID),
                u8::UID
            )
        );

        assert_eq!(
            <[u8; 32]>::UID,
            mix(mix(Uid::from_name("Array"), Uid::from(32u64)), u8::UID)
        );

        assert_ne!(<[u8; 32]>::UID, <[u8; 33]>::UID,);
        assert_ne!(<[u64; 32]>::UID, <[u32; 32]>::UID,);
    }
}
