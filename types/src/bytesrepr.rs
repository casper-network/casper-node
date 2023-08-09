//! Contains serialization and deserialization code for types used throughout the system.
mod bytes;

use alloc::{
    alloc::{alloc, Layout},
    collections::{BTreeMap, BTreeSet, VecDeque},
    str,
    string::String,
    vec,
    vec::Vec,
};
#[cfg(debug_assertions)]
use core::any;
use core::{
    convert::TryInto,
    fmt::{self, Display, Formatter},
    mem,
    ptr::NonNull,
};

#[cfg(feature = "datasize")]
use datasize::DataSize;
use num_integer::Integer;
use num_rational::Ratio;
use serde::{Deserialize, Serialize};

pub use bytes::Bytes;

/// The number of bytes in a serialized `()`.
pub const UNIT_SERIALIZED_LENGTH: usize = 0;
/// The number of bytes in a serialized `bool`.
pub const BOOL_SERIALIZED_LENGTH: usize = 1;
/// The number of bytes in a serialized `i32`.
pub const I32_SERIALIZED_LENGTH: usize = mem::size_of::<i32>();
/// The number of bytes in a serialized `i64`.
pub const I64_SERIALIZED_LENGTH: usize = mem::size_of::<i64>();
/// The number of bytes in a serialized `u8`.
pub const U8_SERIALIZED_LENGTH: usize = mem::size_of::<u8>();
/// The number of bytes in a serialized `u16`.
pub const U16_SERIALIZED_LENGTH: usize = mem::size_of::<u16>();
/// The number of bytes in a serialized `u32`.
pub const U32_SERIALIZED_LENGTH: usize = mem::size_of::<u32>();
/// The number of bytes in a serialized `u64`.
pub const U64_SERIALIZED_LENGTH: usize = mem::size_of::<u64>();
/// The number of bytes in a serialized [`U128`](crate::U128).
pub const U128_SERIALIZED_LENGTH: usize = mem::size_of::<u128>();
/// The number of bytes in a serialized [`U256`](crate::U256).
pub const U256_SERIALIZED_LENGTH: usize = U128_SERIALIZED_LENGTH * 2;
/// The number of bytes in a serialized [`U512`](crate::U512).
pub const U512_SERIALIZED_LENGTH: usize = U256_SERIALIZED_LENGTH * 2;
/// The tag representing a `None` value.
pub const OPTION_NONE_TAG: u8 = 0;
/// The tag representing a `Some` value.
pub const OPTION_SOME_TAG: u8 = 1;
/// The tag representing an `Err` value.
pub const RESULT_ERR_TAG: u8 = 0;
/// The tag representing an `Ok` value.
pub const RESULT_OK_TAG: u8 = 1;

/// A type which can be serialized to a `Vec<u8>`.
pub trait ToBytes {
    /// Serializes `&self` to a `Vec<u8>`.
    fn to_bytes(&self) -> Result<Vec<u8>, Error>;
    /// Consumes `self` and serializes to a `Vec<u8>`.
    fn into_bytes(self) -> Result<Vec<u8>, Error>
    where
        Self: Sized,
    {
        self.to_bytes()
    }
    /// Returns the length of the `Vec<u8>` which would be returned from a successful call to
    /// `to_bytes()` or `into_bytes()`.  The data is not actually serialized, so this call is
    /// relatively cheap.
    fn serialized_length(&self) -> usize;

    /// Writes `&self` into a mutable `writer`.
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        writer.extend(self.to_bytes()?);
        Ok(())
    }
}

/// A type which can be deserialized from a `Vec<u8>`.
pub trait FromBytes: Sized {
    /// Deserializes the slice into `Self`.
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error>;

    /// Deserializes the `Vec<u8>` into `Self`.
    fn from_vec(bytes: Vec<u8>) -> Result<(Self, Vec<u8>), Error> {
        Self::from_bytes(bytes.as_slice()).map(|(x, remainder)| (x, Vec::from(remainder)))
    }
}

/// Returns a `Vec<u8>` initialized with sufficient capacity to hold `to_be_serialized` after
/// serialization.
pub fn unchecked_allocate_buffer<T: ToBytes>(to_be_serialized: &T) -> Vec<u8> {
    let serialized_length = to_be_serialized.serialized_length();
    Vec::with_capacity(serialized_length)
}

/// Returns a `Vec<u8>` initialized with sufficient capacity to hold `to_be_serialized` after
/// serialization, or an error if the capacity would exceed `u32::max_value()`.
pub fn allocate_buffer<T: ToBytes>(to_be_serialized: &T) -> Result<Vec<u8>, Error> {
    let serialized_length = to_be_serialized.serialized_length();
    if serialized_length > u32::max_value() as usize {
        return Err(Error::OutOfMemory);
    }
    Ok(Vec::with_capacity(serialized_length))
}

/// Serialization and deserialization errors.
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[repr(u8)]
#[non_exhaustive]
pub enum Error {
    /// Early end of stream while deserializing.
    EarlyEndOfStream = 0,
    /// Formatting error while deserializing.
    Formatting,
    /// Not all input bytes were consumed in [`deserialize`].
    LeftOverBytes,
    /// Out of memory error.
    OutOfMemory,
    /// No serialized representation is available for a value.
    NotRepresentable,
    /// Exceeded a recursion depth limit.
    ExceededRecursionDepth,
}

impl Display for Error {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            Error::EarlyEndOfStream => {
                formatter.write_str("Deserialization error: early end of stream")
            }
            Error::Formatting => formatter.write_str("Deserialization error: formatting"),
            Error::LeftOverBytes => formatter.write_str("Deserialization error: left-over bytes"),
            Error::OutOfMemory => formatter.write_str("Serialization error: out of memory"),
            Error::NotRepresentable => {
                formatter.write_str("Serialization error: value is not representable.")
            }
            Error::ExceededRecursionDepth => formatter.write_str("exceeded recursion depth"),
        }
    }
}

/// Deserializes `bytes` into an instance of `T`.
///
/// Returns an error if the bytes cannot be deserialized into `T` or if not all of the input bytes
/// are consumed in the operation.
pub fn deserialize<T: FromBytes>(bytes: Vec<u8>) -> Result<T, Error> {
    let (t, remainder) = T::from_bytes(&bytes)?;
    if remainder.is_empty() {
        Ok(t)
    } else {
        Err(Error::LeftOverBytes)
    }
}

/// Deserializes a slice of bytes into an instance of `T`.
///
/// Returns an error if the bytes cannot be deserialized into `T` or if not all of the input bytes
/// are consumed in the operation.
pub fn deserialize_from_slice<I: AsRef<[u8]>, O: FromBytes>(bytes: I) -> Result<O, Error> {
    let (t, remainder) = O::from_bytes(bytes.as_ref())?;
    if remainder.is_empty() {
        Ok(t)
    } else {
        Err(Error::LeftOverBytes)
    }
}

/// Serializes `t` into a `Vec<u8>`.
pub fn serialize(t: impl ToBytes) -> Result<Vec<u8>, Error> {
    t.into_bytes()
}

/// Safely splits the slice at the given point.
pub(crate) fn safe_split_at(bytes: &[u8], n: usize) -> Result<(&[u8], &[u8]), Error> {
    if n > bytes.len() {
        Err(Error::EarlyEndOfStream)
    } else {
        Ok(bytes.split_at(n))
    }
}

impl ToBytes for () {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        Ok(Vec::new())
    }

    fn serialized_length(&self) -> usize {
        UNIT_SERIALIZED_LENGTH
    }
}

impl FromBytes for () {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        Ok(((), bytes))
    }
}

impl ToBytes for bool {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        u8::from(*self).to_bytes()
    }

    fn serialized_length(&self) -> usize {
        BOOL_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        writer.push(*self as u8);
        Ok(())
    }
}

impl FromBytes for bool {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        match bytes.split_first() {
            None => Err(Error::EarlyEndOfStream),
            Some((byte, rem)) => match byte {
                1 => Ok((true, rem)),
                0 => Ok((false, rem)),
                _ => Err(Error::Formatting),
            },
        }
    }
}

impl ToBytes for u8 {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        Ok(vec![*self])
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        writer.push(*self);
        Ok(())
    }
}

impl FromBytes for u8 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        match bytes.split_first() {
            None => Err(Error::EarlyEndOfStream),
            Some((byte, rem)) => Ok((*byte, rem)),
        }
    }
}

impl ToBytes for i32 {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        Ok(self.to_le_bytes().to_vec())
    }

    fn serialized_length(&self) -> usize {
        I32_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        writer.extend_from_slice(&self.to_le_bytes());
        Ok(())
    }
}

impl FromBytes for i32 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let mut result = [0u8; I32_SERIALIZED_LENGTH];
        let (bytes, remainder) = safe_split_at(bytes, I32_SERIALIZED_LENGTH)?;
        result.copy_from_slice(bytes);
        Ok((<i32>::from_le_bytes(result), remainder))
    }
}

impl ToBytes for i64 {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        Ok(self.to_le_bytes().to_vec())
    }

    fn serialized_length(&self) -> usize {
        I64_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        writer.extend_from_slice(&self.to_le_bytes());
        Ok(())
    }
}

impl FromBytes for i64 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let mut result = [0u8; I64_SERIALIZED_LENGTH];
        let (bytes, remainder) = safe_split_at(bytes, I64_SERIALIZED_LENGTH)?;
        result.copy_from_slice(bytes);
        Ok((<i64>::from_le_bytes(result), remainder))
    }
}

impl ToBytes for u16 {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        Ok(self.to_le_bytes().to_vec())
    }

    fn serialized_length(&self) -> usize {
        U16_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        writer.extend_from_slice(&self.to_le_bytes());
        Ok(())
    }
}

impl FromBytes for u16 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let mut result = [0u8; U16_SERIALIZED_LENGTH];
        let (bytes, remainder) = safe_split_at(bytes, U16_SERIALIZED_LENGTH)?;
        result.copy_from_slice(bytes);
        Ok((<u16>::from_le_bytes(result), remainder))
    }
}

impl ToBytes for u32 {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        Ok(self.to_le_bytes().to_vec())
    }

    fn serialized_length(&self) -> usize {
        U32_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        writer.extend_from_slice(&self.to_le_bytes());
        Ok(())
    }
}

impl FromBytes for u32 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let mut result = [0u8; U32_SERIALIZED_LENGTH];
        let (bytes, remainder) = safe_split_at(bytes, U32_SERIALIZED_LENGTH)?;
        result.copy_from_slice(bytes);
        Ok((<u32>::from_le_bytes(result), remainder))
    }
}

impl ToBytes for u64 {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        Ok(self.to_le_bytes().to_vec())
    }

    fn serialized_length(&self) -> usize {
        U64_SERIALIZED_LENGTH
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        writer.extend_from_slice(&self.to_le_bytes());
        Ok(())
    }
}

impl FromBytes for u64 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let mut result = [0u8; U64_SERIALIZED_LENGTH];
        let (bytes, remainder) = safe_split_at(bytes, U64_SERIALIZED_LENGTH)?;
        result.copy_from_slice(bytes);
        Ok((<u64>::from_le_bytes(result), remainder))
    }
}

impl ToBytes for String {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let bytes = self.as_bytes();
        u8_slice_to_bytes(bytes)
    }

    fn serialized_length(&self) -> usize {
        u8_slice_serialized_length(self.as_bytes())
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        write_u8_slice(self.as_bytes(), writer)?;
        Ok(())
    }
}

impl FromBytes for String {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (size, remainder) = u32::from_bytes(bytes)?;
        let (str_bytes, remainder) = safe_split_at(remainder, size as usize)?;
        let result = String::from_utf8(str_bytes.to_vec()).map_err(|_| Error::Formatting)?;
        Ok((result, remainder))
    }
}

fn ensure_efficient_serialization<T>() {
    #[cfg(debug_assertions)]
    debug_assert_ne!(
        any::type_name::<T>(),
        any::type_name::<u8>(),
        "You should use Bytes newtype wrapper for efficiency"
    );
}

fn iterator_serialized_length<'a, T: 'a + ToBytes>(ts: impl Iterator<Item = &'a T>) -> usize {
    U32_SERIALIZED_LENGTH + ts.map(ToBytes::serialized_length).sum::<usize>()
}

impl<T: ToBytes> ToBytes for Vec<T> {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        ensure_efficient_serialization::<T>();

        let mut result = try_vec_with_capacity(self.serialized_length())?;
        let length_32: u32 = self.len().try_into().map_err(|_| Error::NotRepresentable)?;
        result.append(&mut length_32.to_bytes()?);

        for item in self.iter() {
            result.append(&mut item.to_bytes()?);
        }

        Ok(result)
    }

    fn into_bytes(self) -> Result<Vec<u8>, Error> {
        ensure_efficient_serialization::<T>();

        let mut result = allocate_buffer(&self)?;
        let length_32: u32 = self.len().try_into().map_err(|_| Error::NotRepresentable)?;
        result.append(&mut length_32.to_bytes()?);

        for item in self {
            result.append(&mut item.into_bytes()?);
        }

        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        iterator_serialized_length(self.iter())
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        let length_32: u32 = self.len().try_into().map_err(|_| Error::NotRepresentable)?;
        writer.extend_from_slice(&length_32.to_le_bytes());
        for item in self.iter() {
            item.write_bytes(writer)?;
        }
        Ok(())
    }
}

// TODO Replace `try_vec_with_capacity` with `Vec::try_reserve_exact` once it's in stable.
fn try_vec_with_capacity<T>(capacity: usize) -> Result<Vec<T>, Error> {
    // see https://doc.rust-lang.org/src/alloc/raw_vec.rs.html#75-98
    let elem_size = mem::size_of::<T>();
    let alloc_size = capacity.checked_mul(elem_size).ok_or(Error::OutOfMemory)?;

    let ptr = if alloc_size == 0 {
        NonNull::<T>::dangling()
    } else {
        let align = mem::align_of::<T>();
        let layout = Layout::from_size_align(alloc_size, align).map_err(|_| Error::OutOfMemory)?;
        let raw_ptr = unsafe { alloc(layout) };
        let non_null_ptr = NonNull::<u8>::new(raw_ptr).ok_or(Error::OutOfMemory)?;
        non_null_ptr.cast()
    };
    unsafe { Ok(Vec::from_raw_parts(ptr.as_ptr(), 0, capacity)) }
}

fn vec_from_vec<T: FromBytes>(bytes: Vec<u8>) -> Result<(Vec<T>, Vec<u8>), Error> {
    ensure_efficient_serialization::<T>();

    Vec::<T>::from_bytes(bytes.as_slice()).map(|(x, remainder)| (x, Vec::from(remainder)))
}

impl<T: FromBytes> FromBytes for Vec<T> {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        ensure_efficient_serialization::<T>();

        let (count, mut stream) = u32::from_bytes(bytes)?;

        let mut result = try_vec_with_capacity(count as usize)?;
        for _ in 0..count {
            let (value, remainder) = T::from_bytes(stream)?;
            result.push(value);
            stream = remainder;
        }

        Ok((result, stream))
    }

    fn from_vec(bytes: Vec<u8>) -> Result<(Self, Vec<u8>), Error> {
        vec_from_vec(bytes)
    }
}

impl<T: ToBytes> ToBytes for VecDeque<T> {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let (slice1, slice2) = self.as_slices();
        let mut result = allocate_buffer(self)?;
        let length_32: u32 = self.len().try_into().map_err(|_| Error::NotRepresentable)?;
        result.append(&mut length_32.to_bytes()?);
        for item in slice1.iter().chain(slice2.iter()) {
            result.append(&mut item.to_bytes()?);
        }
        Ok(result)
    }

    fn into_bytes(self) -> Result<Vec<u8>, Error> {
        let vec: Vec<T> = self.into();
        vec.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        let (slice1, slice2) = self.as_slices();
        iterator_serialized_length(slice1.iter().chain(slice2.iter()))
    }
}

impl<T: FromBytes> FromBytes for VecDeque<T> {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (vec, bytes) = Vec::from_bytes(bytes)?;
        Ok((VecDeque::from(vec), bytes))
    }

    fn from_vec(bytes: Vec<u8>) -> Result<(Self, Vec<u8>), Error> {
        let (vec, bytes) = vec_from_vec(bytes)?;
        Ok((VecDeque::from(vec), bytes))
    }
}

impl<const COUNT: usize> ToBytes for [u8; COUNT] {
    #[inline(always)]
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        Ok(self.to_vec())
    }

    #[inline(always)]
    fn serialized_length(&self) -> usize {
        COUNT
    }

    #[inline(always)]
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        writer.extend_from_slice(self);
        Ok(())
    }
}

impl<const COUNT: usize> FromBytes for [u8; COUNT] {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (bytes, rem) = safe_split_at(bytes, COUNT)?;
        // SAFETY: safe_split_at makes sure `bytes` is exactly `COUNT` bytes.
        let ptr = bytes.as_ptr() as *const [u8; COUNT];
        let result = unsafe { *ptr };
        Ok((result, rem))
    }
}

impl<V: ToBytes> ToBytes for BTreeSet<V> {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = allocate_buffer(self)?;

        let num_keys: u32 = self.len().try_into().map_err(|_| Error::NotRepresentable)?;
        result.append(&mut num_keys.to_bytes()?);

        for value in self.iter() {
            result.append(&mut value.to_bytes()?);
        }

        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        U32_SERIALIZED_LENGTH + self.iter().map(|v| v.serialized_length()).sum::<usize>()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        let length_32: u32 = self.len().try_into().map_err(|_| Error::NotRepresentable)?;
        writer.extend_from_slice(&length_32.to_le_bytes());
        for value in self.iter() {
            value.write_bytes(writer)?;
        }
        Ok(())
    }
}

impl<V: FromBytes + Ord> FromBytes for BTreeSet<V> {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (num_keys, mut stream) = u32::from_bytes(bytes)?;
        let mut result = BTreeSet::new();
        for _ in 0..num_keys {
            let (v, rem) = V::from_bytes(stream)?;
            result.insert(v);
            stream = rem;
        }
        Ok((result, stream))
    }
}

impl<K, V> ToBytes for BTreeMap<K, V>
where
    K: ToBytes,
    V: ToBytes,
{
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = allocate_buffer(self)?;

        let num_keys: u32 = self.len().try_into().map_err(|_| Error::NotRepresentable)?;
        result.append(&mut num_keys.to_bytes()?);

        for (key, value) in self.iter() {
            result.append(&mut key.to_bytes()?);
            result.append(&mut value.to_bytes()?);
        }

        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        U32_SERIALIZED_LENGTH
            + self
                .iter()
                .map(|(key, value)| key.serialized_length() + value.serialized_length())
                .sum::<usize>()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        let length_32: u32 = self.len().try_into().map_err(|_| Error::NotRepresentable)?;
        writer.extend_from_slice(&length_32.to_le_bytes());
        for (key, value) in self.iter() {
            key.write_bytes(writer)?;
            value.write_bytes(writer)?;
        }
        Ok(())
    }
}

impl<K, V> FromBytes for BTreeMap<K, V>
where
    K: FromBytes + Ord,
    V: FromBytes,
{
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (num_keys, mut stream) = u32::from_bytes(bytes)?;
        let mut result = BTreeMap::new();
        for _ in 0..num_keys {
            let (k, rem) = K::from_bytes(stream)?;
            let (v, rem) = V::from_bytes(rem)?;
            result.insert(k, v);
            stream = rem;
        }
        Ok((result, stream))
    }
}

impl<T: ToBytes> ToBytes for Option<T> {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        match self {
            None => Ok(vec![OPTION_NONE_TAG]),
            Some(v) => {
                let mut result = allocate_buffer(self)?;
                result.push(OPTION_SOME_TAG);

                let mut value = v.to_bytes()?;
                result.append(&mut value);

                Ok(result)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                Some(v) => v.serialized_length(),
                None => 0,
            }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        match self {
            None => writer.push(OPTION_NONE_TAG),
            Some(v) => {
                writer.push(OPTION_SOME_TAG);
                v.write_bytes(writer)?;
            }
        };
        Ok(())
    }
}

impl<T: FromBytes> FromBytes for Option<T> {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (tag, rem) = u8::from_bytes(bytes)?;
        match tag {
            OPTION_NONE_TAG => Ok((None, rem)),
            OPTION_SOME_TAG => {
                let (t, rem) = T::from_bytes(rem)?;
                Ok((Some(t), rem))
            }
            _ => Err(Error::Formatting),
        }
    }
}

impl<T: ToBytes, E: ToBytes> ToBytes for Result<T, E> {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = allocate_buffer(self)?;
        let (variant, mut value) = match self {
            Err(error) => (RESULT_ERR_TAG, error.to_bytes()?),
            Ok(result) => (RESULT_OK_TAG, result.to_bytes()?),
        };
        result.push(variant);
        result.append(&mut value);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                Ok(ok) => ok.serialized_length(),
                Err(error) => error.serialized_length(),
            }
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        match self {
            Err(error) => {
                writer.push(RESULT_ERR_TAG);
                error.write_bytes(writer)?;
            }
            Ok(result) => {
                writer.push(RESULT_OK_TAG);
                result.write_bytes(writer)?;
            }
        };
        Ok(())
    }
}

impl<T: FromBytes, E: FromBytes> FromBytes for Result<T, E> {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (variant, rem) = u8::from_bytes(bytes)?;
        match variant {
            RESULT_ERR_TAG => {
                let (value, rem) = E::from_bytes(rem)?;
                Ok((Err(value), rem))
            }
            RESULT_OK_TAG => {
                let (value, rem) = T::from_bytes(rem)?;
                Ok((Ok(value), rem))
            }
            _ => Err(Error::Formatting),
        }
    }
}

impl<T1: ToBytes> ToBytes for (T1,) {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl<T1: FromBytes> FromBytes for (T1,) {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (t1, remainder) = T1::from_bytes(bytes)?;
        Ok(((t1,), remainder))
    }
}

impl<T1: ToBytes, T2: ToBytes> ToBytes for (T1, T2) {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = allocate_buffer(self)?;
        result.append(&mut self.0.to_bytes()?);
        result.append(&mut self.1.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length() + self.1.serialized_length()
    }
}

impl<T1: FromBytes, T2: FromBytes> FromBytes for (T1, T2) {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (t1, remainder) = T1::from_bytes(bytes)?;
        let (t2, remainder) = T2::from_bytes(remainder)?;
        Ok(((t1, t2), remainder))
    }
}

impl<T1: ToBytes, T2: ToBytes, T3: ToBytes> ToBytes for (T1, T2, T3) {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = allocate_buffer(self)?;
        result.append(&mut self.0.to_bytes()?);
        result.append(&mut self.1.to_bytes()?);
        result.append(&mut self.2.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length() + self.1.serialized_length() + self.2.serialized_length()
    }
}

impl<T1: FromBytes, T2: FromBytes, T3: FromBytes> FromBytes for (T1, T2, T3) {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (t1, remainder) = T1::from_bytes(bytes)?;
        let (t2, remainder) = T2::from_bytes(remainder)?;
        let (t3, remainder) = T3::from_bytes(remainder)?;
        Ok(((t1, t2, t3), remainder))
    }
}

impl<T1: ToBytes, T2: ToBytes, T3: ToBytes, T4: ToBytes> ToBytes for (T1, T2, T3, T4) {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = allocate_buffer(self)?;
        result.append(&mut self.0.to_bytes()?);
        result.append(&mut self.1.to_bytes()?);
        result.append(&mut self.2.to_bytes()?);
        result.append(&mut self.3.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
            + self.1.serialized_length()
            + self.2.serialized_length()
            + self.3.serialized_length()
    }
}

impl<T1: FromBytes, T2: FromBytes, T3: FromBytes, T4: FromBytes> FromBytes for (T1, T2, T3, T4) {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (t1, remainder) = T1::from_bytes(bytes)?;
        let (t2, remainder) = T2::from_bytes(remainder)?;
        let (t3, remainder) = T3::from_bytes(remainder)?;
        let (t4, remainder) = T4::from_bytes(remainder)?;
        Ok(((t1, t2, t3, t4), remainder))
    }
}

impl<T1: ToBytes, T2: ToBytes, T3: ToBytes, T4: ToBytes, T5: ToBytes> ToBytes
    for (T1, T2, T3, T4, T5)
{
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = allocate_buffer(self)?;
        result.append(&mut self.0.to_bytes()?);
        result.append(&mut self.1.to_bytes()?);
        result.append(&mut self.2.to_bytes()?);
        result.append(&mut self.3.to_bytes()?);
        result.append(&mut self.4.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
            + self.1.serialized_length()
            + self.2.serialized_length()
            + self.3.serialized_length()
            + self.4.serialized_length()
    }
}

impl<T1: FromBytes, T2: FromBytes, T3: FromBytes, T4: FromBytes, T5: FromBytes> FromBytes
    for (T1, T2, T3, T4, T5)
{
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (t1, remainder) = T1::from_bytes(bytes)?;
        let (t2, remainder) = T2::from_bytes(remainder)?;
        let (t3, remainder) = T3::from_bytes(remainder)?;
        let (t4, remainder) = T4::from_bytes(remainder)?;
        let (t5, remainder) = T5::from_bytes(remainder)?;
        Ok(((t1, t2, t3, t4, t5), remainder))
    }
}

impl<T1: ToBytes, T2: ToBytes, T3: ToBytes, T4: ToBytes, T5: ToBytes, T6: ToBytes> ToBytes
    for (T1, T2, T3, T4, T5, T6)
{
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = allocate_buffer(self)?;
        result.append(&mut self.0.to_bytes()?);
        result.append(&mut self.1.to_bytes()?);
        result.append(&mut self.2.to_bytes()?);
        result.append(&mut self.3.to_bytes()?);
        result.append(&mut self.4.to_bytes()?);
        result.append(&mut self.5.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
            + self.1.serialized_length()
            + self.2.serialized_length()
            + self.3.serialized_length()
            + self.4.serialized_length()
            + self.5.serialized_length()
    }
}

impl<T1: FromBytes, T2: FromBytes, T3: FromBytes, T4: FromBytes, T5: FromBytes, T6: FromBytes>
    FromBytes for (T1, T2, T3, T4, T5, T6)
{
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (t1, remainder) = T1::from_bytes(bytes)?;
        let (t2, remainder) = T2::from_bytes(remainder)?;
        let (t3, remainder) = T3::from_bytes(remainder)?;
        let (t4, remainder) = T4::from_bytes(remainder)?;
        let (t5, remainder) = T5::from_bytes(remainder)?;
        let (t6, remainder) = T6::from_bytes(remainder)?;
        Ok(((t1, t2, t3, t4, t5, t6), remainder))
    }
}

impl<T1: ToBytes, T2: ToBytes, T3: ToBytes, T4: ToBytes, T5: ToBytes, T6: ToBytes, T7: ToBytes>
    ToBytes for (T1, T2, T3, T4, T5, T6, T7)
{
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = allocate_buffer(self)?;
        result.append(&mut self.0.to_bytes()?);
        result.append(&mut self.1.to_bytes()?);
        result.append(&mut self.2.to_bytes()?);
        result.append(&mut self.3.to_bytes()?);
        result.append(&mut self.4.to_bytes()?);
        result.append(&mut self.5.to_bytes()?);
        result.append(&mut self.6.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
            + self.1.serialized_length()
            + self.2.serialized_length()
            + self.3.serialized_length()
            + self.4.serialized_length()
            + self.5.serialized_length()
            + self.6.serialized_length()
    }
}

impl<
        T1: FromBytes,
        T2: FromBytes,
        T3: FromBytes,
        T4: FromBytes,
        T5: FromBytes,
        T6: FromBytes,
        T7: FromBytes,
    > FromBytes for (T1, T2, T3, T4, T5, T6, T7)
{
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (t1, remainder) = T1::from_bytes(bytes)?;
        let (t2, remainder) = T2::from_bytes(remainder)?;
        let (t3, remainder) = T3::from_bytes(remainder)?;
        let (t4, remainder) = T4::from_bytes(remainder)?;
        let (t5, remainder) = T5::from_bytes(remainder)?;
        let (t6, remainder) = T6::from_bytes(remainder)?;
        let (t7, remainder) = T7::from_bytes(remainder)?;
        Ok(((t1, t2, t3, t4, t5, t6, t7), remainder))
    }
}

impl<
        T1: ToBytes,
        T2: ToBytes,
        T3: ToBytes,
        T4: ToBytes,
        T5: ToBytes,
        T6: ToBytes,
        T7: ToBytes,
        T8: ToBytes,
    > ToBytes for (T1, T2, T3, T4, T5, T6, T7, T8)
{
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = allocate_buffer(self)?;
        result.append(&mut self.0.to_bytes()?);
        result.append(&mut self.1.to_bytes()?);
        result.append(&mut self.2.to_bytes()?);
        result.append(&mut self.3.to_bytes()?);
        result.append(&mut self.4.to_bytes()?);
        result.append(&mut self.5.to_bytes()?);
        result.append(&mut self.6.to_bytes()?);
        result.append(&mut self.7.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
            + self.1.serialized_length()
            + self.2.serialized_length()
            + self.3.serialized_length()
            + self.4.serialized_length()
            + self.5.serialized_length()
            + self.6.serialized_length()
            + self.7.serialized_length()
    }
}

impl<
        T1: FromBytes,
        T2: FromBytes,
        T3: FromBytes,
        T4: FromBytes,
        T5: FromBytes,
        T6: FromBytes,
        T7: FromBytes,
        T8: FromBytes,
    > FromBytes for (T1, T2, T3, T4, T5, T6, T7, T8)
{
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (t1, remainder) = T1::from_bytes(bytes)?;
        let (t2, remainder) = T2::from_bytes(remainder)?;
        let (t3, remainder) = T3::from_bytes(remainder)?;
        let (t4, remainder) = T4::from_bytes(remainder)?;
        let (t5, remainder) = T5::from_bytes(remainder)?;
        let (t6, remainder) = T6::from_bytes(remainder)?;
        let (t7, remainder) = T7::from_bytes(remainder)?;
        let (t8, remainder) = T8::from_bytes(remainder)?;
        Ok(((t1, t2, t3, t4, t5, t6, t7, t8), remainder))
    }
}

impl<
        T1: ToBytes,
        T2: ToBytes,
        T3: ToBytes,
        T4: ToBytes,
        T5: ToBytes,
        T6: ToBytes,
        T7: ToBytes,
        T8: ToBytes,
        T9: ToBytes,
    > ToBytes for (T1, T2, T3, T4, T5, T6, T7, T8, T9)
{
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = allocate_buffer(self)?;
        result.append(&mut self.0.to_bytes()?);
        result.append(&mut self.1.to_bytes()?);
        result.append(&mut self.2.to_bytes()?);
        result.append(&mut self.3.to_bytes()?);
        result.append(&mut self.4.to_bytes()?);
        result.append(&mut self.5.to_bytes()?);
        result.append(&mut self.6.to_bytes()?);
        result.append(&mut self.7.to_bytes()?);
        result.append(&mut self.8.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
            + self.1.serialized_length()
            + self.2.serialized_length()
            + self.3.serialized_length()
            + self.4.serialized_length()
            + self.5.serialized_length()
            + self.6.serialized_length()
            + self.7.serialized_length()
            + self.8.serialized_length()
    }
}

impl<
        T1: FromBytes,
        T2: FromBytes,
        T3: FromBytes,
        T4: FromBytes,
        T5: FromBytes,
        T6: FromBytes,
        T7: FromBytes,
        T8: FromBytes,
        T9: FromBytes,
    > FromBytes for (T1, T2, T3, T4, T5, T6, T7, T8, T9)
{
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (t1, remainder) = T1::from_bytes(bytes)?;
        let (t2, remainder) = T2::from_bytes(remainder)?;
        let (t3, remainder) = T3::from_bytes(remainder)?;
        let (t4, remainder) = T4::from_bytes(remainder)?;
        let (t5, remainder) = T5::from_bytes(remainder)?;
        let (t6, remainder) = T6::from_bytes(remainder)?;
        let (t7, remainder) = T7::from_bytes(remainder)?;
        let (t8, remainder) = T8::from_bytes(remainder)?;
        let (t9, remainder) = T9::from_bytes(remainder)?;
        Ok(((t1, t2, t3, t4, t5, t6, t7, t8, t9), remainder))
    }
}

impl<
        T1: ToBytes,
        T2: ToBytes,
        T3: ToBytes,
        T4: ToBytes,
        T5: ToBytes,
        T6: ToBytes,
        T7: ToBytes,
        T8: ToBytes,
        T9: ToBytes,
        T10: ToBytes,
    > ToBytes for (T1, T2, T3, T4, T5, T6, T7, T8, T9, T10)
{
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = allocate_buffer(self)?;
        result.append(&mut self.0.to_bytes()?);
        result.append(&mut self.1.to_bytes()?);
        result.append(&mut self.2.to_bytes()?);
        result.append(&mut self.3.to_bytes()?);
        result.append(&mut self.4.to_bytes()?);
        result.append(&mut self.5.to_bytes()?);
        result.append(&mut self.6.to_bytes()?);
        result.append(&mut self.7.to_bytes()?);
        result.append(&mut self.8.to_bytes()?);
        result.append(&mut self.9.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
            + self.1.serialized_length()
            + self.2.serialized_length()
            + self.3.serialized_length()
            + self.4.serialized_length()
            + self.5.serialized_length()
            + self.6.serialized_length()
            + self.7.serialized_length()
            + self.8.serialized_length()
            + self.9.serialized_length()
    }
}

impl<
        T1: FromBytes,
        T2: FromBytes,
        T3: FromBytes,
        T4: FromBytes,
        T5: FromBytes,
        T6: FromBytes,
        T7: FromBytes,
        T8: FromBytes,
        T9: FromBytes,
        T10: FromBytes,
    > FromBytes for (T1, T2, T3, T4, T5, T6, T7, T8, T9, T10)
{
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (t1, remainder) = T1::from_bytes(bytes)?;
        let (t2, remainder) = T2::from_bytes(remainder)?;
        let (t3, remainder) = T3::from_bytes(remainder)?;
        let (t4, remainder) = T4::from_bytes(remainder)?;
        let (t5, remainder) = T5::from_bytes(remainder)?;
        let (t6, remainder) = T6::from_bytes(remainder)?;
        let (t7, remainder) = T7::from_bytes(remainder)?;
        let (t8, remainder) = T8::from_bytes(remainder)?;
        let (t9, remainder) = T9::from_bytes(remainder)?;
        let (t10, remainder) = T10::from_bytes(remainder)?;
        Ok(((t1, t2, t3, t4, t5, t6, t7, t8, t9, t10), remainder))
    }
}

impl ToBytes for str {
    #[inline]
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        u8_slice_to_bytes(self.as_bytes())
    }

    #[inline]
    fn serialized_length(&self) -> usize {
        u8_slice_serialized_length(self.as_bytes())
    }

    #[inline]
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        write_u8_slice(self.as_bytes(), writer)?;
        Ok(())
    }
}

impl ToBytes for &str {
    #[inline(always)]
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        (*self).to_bytes()
    }

    #[inline(always)]
    fn serialized_length(&self) -> usize {
        (*self).serialized_length()
    }

    #[inline]
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        write_u8_slice(self.as_bytes(), writer)?;
        Ok(())
    }
}

impl<T> ToBytes for &T
where
    T: ToBytes,
{
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        (*self).to_bytes()
    }

    fn serialized_length(&self) -> usize {
        (*self).serialized_length()
    }
}

impl<T> ToBytes for Ratio<T>
where
    T: Clone + Integer + ToBytes,
{
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        if self.denom().is_zero() {
            return Err(Error::Formatting);
        }
        (self.numer().clone(), self.denom().clone()).into_bytes()
    }

    fn serialized_length(&self) -> usize {
        (self.numer().clone(), self.denom().clone()).serialized_length()
    }
}

impl<T> FromBytes for Ratio<T>
where
    T: Clone + FromBytes + Integer,
{
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let ((numer, denom), rem): ((T, T), &[u8]) = FromBytes::from_bytes(bytes)?;
        if denom.is_zero() {
            return Err(Error::Formatting);
        }
        Ok((Ratio::new(numer, denom), rem))
    }
}

/// Serializes a slice of bytes with a length prefix.
///
/// This function is serializing a slice of bytes with an addition of a 4 byte length prefix.
///
/// For safety you should prefer to use [`vec_u8_to_bytes`]. For efficiency reasons you should also
/// avoid using serializing Vec<u8>.
fn u8_slice_to_bytes(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let serialized_length = u8_slice_serialized_length(bytes);
    let mut vec = try_vec_with_capacity(serialized_length)?;
    let length_prefix: u32 = bytes
        .len()
        .try_into()
        .map_err(|_| Error::NotRepresentable)?;
    let length_prefix_bytes = length_prefix.to_le_bytes();
    vec.extend_from_slice(&length_prefix_bytes);
    vec.extend_from_slice(bytes);
    Ok(vec)
}

fn write_u8_slice(bytes: &[u8], writer: &mut Vec<u8>) -> Result<(), Error> {
    let length_32: u32 = bytes
        .len()
        .try_into()
        .map_err(|_| Error::NotRepresentable)?;
    writer.extend_from_slice(&length_32.to_le_bytes());
    writer.extend_from_slice(bytes);
    Ok(())
}

/// Serializes a vector of bytes with a length prefix.
///
/// For efficiency you should avoid serializing Vec<u8>.
#[allow(clippy::ptr_arg)]
#[inline]
pub(crate) fn vec_u8_to_bytes(vec: &Vec<u8>) -> Result<Vec<u8>, Error> {
    u8_slice_to_bytes(vec.as_slice())
}

/// Returns serialized length of serialized slice of bytes.
///
/// This function adds a length prefix in the beginning.
#[inline(always)]
fn u8_slice_serialized_length(bytes: &[u8]) -> usize {
    U32_SERIALIZED_LENGTH + bytes.len()
}

#[allow(clippy::ptr_arg)]
#[inline]
pub(crate) fn vec_u8_serialized_length(vec: &Vec<u8>) -> usize {
    u8_slice_serialized_length(vec.as_slice())
}

// This test helper is not intended to be used by third party crates.
#[doc(hidden)]
/// Returns `true` if a we can serialize and then deserialize a value
pub fn test_serialization_roundtrip<T>(t: &T)
where
    T: alloc::fmt::Debug + ToBytes + FromBytes + PartialEq,
{
    let serialized = ToBytes::to_bytes(t).expect("Unable to serialize data");
    assert_eq!(
        serialized.len(),
        t.serialized_length(),
        "\nLength of serialized data: {},\nserialized_length() yielded: {},\nserialized data: {:?}, t is {:?}",
        serialized.len(),
        t.serialized_length(),
        serialized,
        t
    );
    let mut written_bytes = vec![];
    t.write_bytes(&mut written_bytes)
        .expect("Unable to serialize data via write_bytes");
    assert_eq!(serialized, written_bytes);

    let deserialized_from_slice =
        deserialize_from_slice(&serialized).expect("Unable to deserialize data");
    // assert!(*t == deserialized);
    assert_eq!(*t, deserialized_from_slice);

    let deserialized = deserialize::<T>(serialized).expect("Unable to deserialize data");
    assert_eq!(*t, deserialized);
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_not_serialize_zero_denominator() {
        let malicious = Ratio::new_raw(1, 0);
        assert_eq!(malicious.to_bytes().unwrap_err(), Error::Formatting);
    }

    #[test]
    fn should_not_deserialize_zero_denominator() {
        let malicious_bytes = (1u64, 0u64).to_bytes().unwrap();
        let result: Result<Ratio<u64>, Error> = super::deserialize(malicious_bytes);
        assert_eq!(result.unwrap_err(), Error::Formatting);
    }

    #[test]
    fn should_have_generic_tobytes_impl_for_borrowed_types() {
        struct NonCopyable;

        impl ToBytes for NonCopyable {
            fn to_bytes(&self) -> Result<Vec<u8>, Error> {
                Ok(vec![1, 2, 3])
            }

            fn serialized_length(&self) -> usize {
                3
            }
        }

        let noncopyable: &NonCopyable = &NonCopyable;

        assert_eq!(noncopyable.to_bytes().unwrap(), vec![1, 2, 3]);
        assert_eq!(noncopyable.serialized_length(), 3);
        assert_eq!(noncopyable.into_bytes().unwrap(), vec![1, 2, 3]);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "You should use Bytes newtype wrapper for efficiency")]
    fn should_fail_to_serialize_slice_of_u8() {
        let bytes = b"0123456789".to_vec();
        bytes.to_bytes().unwrap();
    }
}

#[cfg(test)]
mod proptests {
    use std::collections::VecDeque;

    use proptest::{collection::vec, prelude::*};

    use crate::{
        bytesrepr::{self, bytes::gens::bytes_arb, ToBytes},
        gens::*,
    };

    proptest! {
        #[test]
        fn test_bool(u in any::<bool>()) {
            bytesrepr::test_serialization_roundtrip(&u);
        }

        #[test]
        fn test_u8(u in any::<u8>()) {
            bytesrepr::test_serialization_roundtrip(&u);
        }

        #[test]
        fn test_u16(u in any::<u16>()) {
            bytesrepr::test_serialization_roundtrip(&u);
        }

        #[test]
        fn test_u32(u in any::<u32>()) {
            bytesrepr::test_serialization_roundtrip(&u);
        }

        #[test]
        fn test_i32(u in any::<i32>()) {
            bytesrepr::test_serialization_roundtrip(&u);
        }

        #[test]
        fn test_u64(u in any::<u64>()) {
            bytesrepr::test_serialization_roundtrip(&u);
        }

        #[test]
        fn test_i64(u in any::<i64>()) {
            bytesrepr::test_serialization_roundtrip(&u);
        }

        #[test]
        fn test_u8_slice_32(s in u8_slice_32()) {
            bytesrepr::test_serialization_roundtrip(&s);
        }

        #[test]
        fn test_vec_u8(u in bytes_arb(1..100)) {
            bytesrepr::test_serialization_roundtrip(&u);
        }

        #[test]
        fn test_vec_i32(u in vec(any::<i32>(), 1..100)) {
            bytesrepr::test_serialization_roundtrip(&u);
        }

        #[test]
        fn test_vecdeque_i32((front, back) in (vec(any::<i32>(), 1..100), vec(any::<i32>(), 1..100))) {
            let mut vec_deque = VecDeque::new();
            for f in front {
                vec_deque.push_front(f);
            }
            for f in back {
                vec_deque.push_back(f);
            }
            bytesrepr::test_serialization_roundtrip(&vec_deque);
        }

        #[test]
        fn test_vec_vec_u8(u in vec(bytes_arb(1..100), 10)) {
            bytesrepr::test_serialization_roundtrip(&u);
        }

        #[test]
        fn test_uref_map(m in named_keys_arb(20)) {
            bytesrepr::test_serialization_roundtrip(&m);
        }

        #[test]
        fn test_array_u8_32(arr in any::<[u8; 32]>()) {
            bytesrepr::test_serialization_roundtrip(&arr);
        }

        #[test]
        fn test_string(s in "\\PC*") {
            bytesrepr::test_serialization_roundtrip(&s);
        }

        #[test]
        fn test_str(s in "\\PC*") {
            let not_a_string_object = s.as_str();
            not_a_string_object.to_bytes().expect("should serialize a str");
        }

        #[test]
        fn test_option(o in proptest::option::of(key_arb())) {
            bytesrepr::test_serialization_roundtrip(&o);
        }

        #[test]
        fn test_unit(unit in Just(())) {
            bytesrepr::test_serialization_roundtrip(&unit);
        }

        #[test]
        fn test_u128_serialization(u in u128_arb()) {
            bytesrepr::test_serialization_roundtrip(&u);
        }

        #[test]
        fn test_u256_serialization(u in u256_arb()) {
            bytesrepr::test_serialization_roundtrip(&u);
        }

        #[test]
        fn test_u512_serialization(u in u512_arb()) {
            bytesrepr::test_serialization_roundtrip(&u);
        }

        #[test]
        fn test_key_serialization(key in key_arb()) {
            bytesrepr::test_serialization_roundtrip(&key);
        }

        #[test]
        fn test_cl_value_serialization(cl_value in cl_value_arb()) {
            bytesrepr::test_serialization_roundtrip(&cl_value);
        }

        #[test]
        fn test_access_rights(access_right in access_rights_arb()) {
            bytesrepr::test_serialization_roundtrip(&access_right);
        }

        #[test]
        fn test_uref(uref in uref_arb()) {
            bytesrepr::test_serialization_roundtrip(&uref);
        }

        #[test]
        fn test_account_hash(pk in account_hash_arb()) {
            bytesrepr::test_serialization_roundtrip(&pk);
        }

        #[test]
        fn test_result(result in result_arb()) {
            bytesrepr::test_serialization_roundtrip(&result);
        }

        #[test]
        fn test_phase_serialization(phase in phase_arb()) {
            bytesrepr::test_serialization_roundtrip(&phase);
        }

        #[test]
        fn test_protocol_version(protocol_version in protocol_version_arb()) {
            bytesrepr::test_serialization_roundtrip(&protocol_version);
        }

        #[test]
        fn test_sem_ver(sem_ver in sem_ver_arb()) {
            bytesrepr::test_serialization_roundtrip(&sem_ver);
        }

        #[test]
        fn test_tuple1(t in (any::<u8>(),)) {
            bytesrepr::test_serialization_roundtrip(&t);
        }

        #[test]
        fn test_tuple2(t in (any::<u8>(),any::<u32>())) {
            bytesrepr::test_serialization_roundtrip(&t);
        }

        #[test]
        fn test_tuple3(t in (any::<u8>(),any::<u32>(),any::<i32>())) {
            bytesrepr::test_serialization_roundtrip(&t);
        }

        #[test]
        fn test_tuple4(t in (any::<u8>(),any::<u32>(),any::<i32>(), any::<i32>())) {
            bytesrepr::test_serialization_roundtrip(&t);
        }
        #[test]
        fn test_tuple5(t in (any::<u8>(),any::<u32>(),any::<i32>(), any::<i32>(), any::<i32>())) {
            bytesrepr::test_serialization_roundtrip(&t);
        }
        #[test]
        fn test_tuple6(t in (any::<u8>(),any::<u32>(),any::<i32>(), any::<i32>(), any::<i32>(), any::<i32>())) {
            bytesrepr::test_serialization_roundtrip(&t);
        }
        #[test]
        fn test_tuple7(t in (any::<u8>(),any::<u32>(),any::<i32>(), any::<i32>(), any::<i32>(), any::<i32>(), any::<i32>())) {
            bytesrepr::test_serialization_roundtrip(&t);
        }
        #[test]
        fn test_tuple8(t in (any::<u8>(),any::<u32>(),any::<i32>(), any::<i32>(), any::<i32>(), any::<i32>(), any::<i32>(), any::<i32>())) {
            bytesrepr::test_serialization_roundtrip(&t);
        }
        #[test]
        fn test_tuple9(t in (any::<u8>(),any::<u32>(),any::<i32>(), any::<i32>(), any::<i32>(), any::<i32>(), any::<i32>(), any::<i32>(), any::<i32>())) {
            bytesrepr::test_serialization_roundtrip(&t);
        }
        #[test]
        fn test_tuple10(t in (any::<u8>(),any::<u32>(),any::<i32>(), any::<i32>(), any::<i32>(), any::<i32>(), any::<i32>(), any::<i32>(), any::<i32>(), any::<i32>())) {
            bytesrepr::test_serialization_roundtrip(&t);
        }
        #[test]
        fn test_ratio_u64(t in (any::<u64>(), 1..u64::max_value())) {
            bytesrepr::test_serialization_roundtrip(&t);
        }
    }
}
