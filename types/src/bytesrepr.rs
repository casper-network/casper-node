//! Contains serialization and deserialization code for types used throughout the system.

// Can be removed once https://github.com/rust-lang/rustfmt/issues/3362 is resolved.
#[rustfmt::skip]
use alloc::vec;
#[cfg(feature = "no-unstable-features")]
use alloc::alloc::{alloc, Layout};
#[cfg(not(feature = "no-unstable-features"))]
use alloc::collections::TryReserveError;
use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
    vec::Vec,
};
use core::mem::{self, MaybeUninit};
#[cfg(feature = "no-unstable-features")]
use core::ptr::NonNull;

use failure::Fail;

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
#[derive(Debug, Fail, PartialEq, Eq, Clone)]
#[repr(u8)]
pub enum Error {
    /// Early end of stream while deserializing.
    #[fail(display = "Deserialization error: early end of stream")]
    EarlyEndOfStream = 0,
    /// Formatting error while deserializing.
    #[fail(display = "Deserialization error: formatting")]
    Formatting,
    /// Not all input bytes were consumed in [`deserialize`].
    #[fail(display = "Deserialization error: left-over bytes")]
    LeftOverBytes,
    /// Out of memory error.
    #[fail(display = "Serialization error: out of memory")]
    OutOfMemory,
}

#[cfg(not(feature = "no-unstable-features"))]
impl From<TryReserveError> for Error {
    fn from(_: TryReserveError) -> Error {
        Error::OutOfMemory
    }
}

/// Deserializes `bytes` into an instance of `T`.
///
/// Returns an error if the bytes cannot be deserialized into `T` or if not all of the input bytes
/// are consumed in the operation.
pub fn deserialize<T: FromBytes>(bytes: Vec<u8>) -> Result<T, Error> {
    let (t, remainder) = T::from_vec(bytes)?;
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
        self.as_str().to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.as_str().serialized_length()
    }
}

impl FromBytes for String {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (str_bytes, rem): (Vec<u8>, &[u8]) = FromBytes::from_bytes(bytes)?;
        let result = String::from_utf8(str_bytes).map_err(|_| Error::Formatting)?;
        Ok((result, rem))
    }
}

#[allow(clippy::ptr_arg)]
fn vec_to_bytes<T: ToBytes>(vec: &Vec<T>) -> Result<Vec<u8>, Error> {
    let mut result = allocate_buffer(vec)?;
    result.append(&mut (vec.len() as u32).to_bytes()?);

    for item in vec.iter() {
        result.append(&mut item.to_bytes()?);
    }

    Ok(result)
}

fn vec_into_bytes<T: ToBytes>(vec: Vec<T>) -> Result<Vec<u8>, Error> {
    let mut result = allocate_buffer(&vec)?;
    result.append(&mut (vec.len() as u32).to_bytes()?);

    for item in vec {
        result.append(&mut item.into_bytes()?);
    }

    Ok(result)
}

fn vec_serialized_length<T: ToBytes>(vec: &[T]) -> usize {
    U32_SERIALIZED_LENGTH + vec.iter().map(ToBytes::serialized_length).sum::<usize>()
}

#[cfg(feature = "no-unstable-features")]
impl<T: ToBytes> ToBytes for Vec<T> {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        vec_to_bytes(self)
    }

    fn into_bytes(self) -> Result<Vec<u8>, Error> {
        vec_into_bytes(self)
    }

    fn serialized_length(&self) -> usize {
        vec_serialized_length(self)
    }
}

#[cfg(not(feature = "no-unstable-features"))]
impl<T: ToBytes> ToBytes for Vec<T> {
    default fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        vec_to_bytes(self)
    }

    default fn into_bytes(self) -> Result<Vec<u8>, Error> {
        vec_into_bytes(self)
    }

    default fn serialized_length(&self) -> usize {
        vec_serialized_length(self)
    }
}

#[cfg(feature = "no-unstable-features")]
fn try_vec_with_capacity<T>(capacity: usize) -> Result<Vec<T>, Error> {
    // see https://doc.rust-lang.org/src/alloc/raw_vec.rs.html#75-98
    let elem_size = mem::size_of::<T>();
    let alloc_size = capacity
        .checked_mul(elem_size)
        .ok_or_else(|| Error::OutOfMemory)?;

    let ptr = if alloc_size == 0 {
        NonNull::<T>::dangling()
    } else {
        let align = mem::align_of::<T>();
        let layout = Layout::from_size_align(alloc_size, align).unwrap();
        let raw_ptr = unsafe { alloc(layout) };
        let non_null_ptr = NonNull::<u8>::new(raw_ptr).ok_or_else(|| Error::OutOfMemory)?;
        non_null_ptr.cast()
    };
    unsafe { Ok(Vec::from_raw_parts(ptr.as_ptr(), 0, capacity)) }
}

#[cfg(not(feature = "no-unstable-features"))]
fn try_vec_with_capacity<T>(capacity: usize) -> Result<Vec<T>, Error> {
    let mut result: Vec<T> = Vec::new();
    result.try_reserve_exact(capacity)?;
    Ok(result)
}

fn vec_from_bytes<T: FromBytes>(bytes: &[u8]) -> Result<(Vec<T>, &[u8]), Error> {
    let (count, mut stream) = u32::from_bytes(bytes)?;

    let mut result = try_vec_with_capacity(count as usize)?;
    for _ in 0..count {
        let (value, remainder) = T::from_bytes(stream)?;
        result.push(value);
        stream = remainder;
    }

    Ok((result, stream))
}

fn vec_from_vec<T: FromBytes>(bytes: Vec<u8>) -> Result<(Vec<T>, Vec<u8>), Error> {
    Vec::<T>::from_bytes(bytes.as_slice()).map(|(x, remainder)| (x, Vec::from(remainder)))
}

#[cfg(feature = "no-unstable-features")]
impl<T: FromBytes> FromBytes for Vec<T> {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        vec_from_bytes(bytes)
    }

    fn from_vec(bytes: Vec<u8>) -> Result<(Self, Vec<u8>), Error> {
        vec_from_vec(bytes)
    }
}

#[cfg(not(feature = "no-unstable-features"))]
impl<T: FromBytes> FromBytes for Vec<T> {
    default fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        vec_from_bytes(bytes)
    }

    default fn from_vec(bytes: Vec<u8>) -> Result<(Self, Vec<u8>), Error> {
        vec_from_vec(bytes)
    }
}

#[cfg(not(feature = "no-unstable-features"))]
impl ToBytes for Vec<u8> {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = allocate_buffer(self)?;
        result.append(&mut (self.len() as u32).to_bytes()?);
        result.extend(self);
        Ok(result)
    }

    fn into_bytes(mut self) -> Result<Vec<u8>, Error> {
        let mut result = allocate_buffer(&self)?;
        result.append(&mut (self.len() as u32).to_bytes()?);
        result.append(&mut self);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        U32_SERIALIZED_LENGTH + self.len()
    }
}

#[cfg(not(feature = "no-unstable-features"))]
impl FromBytes for Vec<u8> {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (size, remainder) = u32::from_bytes(bytes)?;
        let (result, remainder) = safe_split_at(remainder, size as usize)?;
        Ok((result.to_vec(), remainder))
    }

    fn from_vec(bytes: Vec<u8>) -> Result<(Self, Vec<u8>), Error> {
        let (size, mut stream) = u32::from_vec(bytes)?;

        if size as usize > stream.len() {
            Err(Error::EarlyEndOfStream)
        } else {
            let remainder = stream.split_off(size as usize);
            Ok((stream, remainder))
        }
    }
}

macro_rules! impl_to_from_bytes_for_array {
    ($($N:literal)+) => {
        $(
            #[cfg(feature = "no-unstable-features")]
            impl<T: ToBytes> ToBytes for [T; $N] {
                fn to_bytes(&self) -> Result<Vec<u8>, Error> {
                    let mut result = allocate_buffer(self)?;
                    for item in self.iter() {
                        result.append(&mut item.to_bytes()?);
                    }
                    Ok(result)
                }

                fn serialized_length(&self) -> usize {
                    self.iter().map(ToBytes::serialized_length).sum::<usize>()
                }
            }

            #[cfg(feature = "no-unstable-features")]
            impl<T: FromBytes> FromBytes for [T; $N] {
                #[allow(clippy::reversed_empty_ranges)]
                fn from_bytes(mut bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
                    let mut result: MaybeUninit<[T; $N]> = MaybeUninit::uninit();
                    let result_ptr = result.as_mut_ptr() as *mut T;
                    unsafe {
                        for i in 0..$N {
                            let (t, remainder) = match T::from_bytes(bytes) {
                                Ok(success) => success,
                                Err(error) => {
                                    for j in 0..i {
                                        result_ptr.add(j).drop_in_place();
                                    }
                                    return Err(error);
                                }
                            };
                            result_ptr.add(i).write(t);
                            bytes = remainder;
                        }
                        Ok((result.assume_init(), bytes))
                    }
                }
            }

            #[cfg(not(feature = "no-unstable-features"))]
            impl<T: ToBytes> ToBytes for [T; $N] {
                default fn to_bytes(&self) -> Result<Vec<u8>, Error> {
                    let mut result = allocate_buffer(self)?;
                    for item in self.iter() {
                        result.append(&mut item.to_bytes()?);
                    }
                    Ok(result)
                }

                default fn serialized_length(&self) -> usize {
                    self.iter().map(ToBytes::serialized_length).sum::<usize>()
                }
            }

            #[cfg(not(feature = "no-unstable-features"))]
            impl<T: FromBytes> FromBytes for [T; $N] {
                default fn from_bytes(mut bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
                    let mut result: MaybeUninit<[T; $N]> = MaybeUninit::uninit();
                    let result_ptr = result.as_mut_ptr() as *mut T;
                    unsafe {
                        #[allow(clippy::reversed_empty_ranges)]
                        for i in 0..$N {
                            let (t, remainder) = match T::from_bytes(bytes) {
                                Ok(success) => success,
                                Err(error) => {
                                    for j in 0..i {
                                        result_ptr.add(j).drop_in_place();
                                    }
                                    return Err(error);
                                }
                            };
                            result_ptr.add(i).write(t);
                            bytes = remainder;
                        }
                        Ok((result.assume_init(), bytes))
                    }
                }
            }
        )+
    }
}

impl_to_from_bytes_for_array! {
     0  1  2  3  4  5  6  7  8  9
    10 11 12 13 14 15 16 17 18 19
    20 21 22 23 24 25 26 27 28 29
    30 31 32
    64 128 256 512
}

#[cfg(not(feature = "no-unstable-features"))]
macro_rules! impl_to_from_bytes_for_byte_array {
    ($($len:expr)+) => {
        $(
            impl ToBytes for [u8; $len] {
                fn to_bytes(&self) -> Result<Vec<u8>, Error> {
                    Ok(self.to_vec())
                }

                fn serialized_length(&self) -> usize { $len }
            }

            impl FromBytes for [u8; $len] {
                fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
                    let (bytes, rem) = safe_split_at(bytes, $len)?;
                    let mut result = [0u8; $len];
                    result.copy_from_slice(bytes);
                    Ok((result, rem))
                }
            }
        )+
    }
}

#[cfg(not(feature = "no-unstable-features"))]
impl_to_from_bytes_for_byte_array! {
     0  1  2  3  4  5  6  7  8  9
    10 11 12 13 14 15 16 17 18 19
    20 21 22 23 24 25 26 27 28 29
    30 31 32
    64 128 256 512
}

impl<V: ToBytes> ToBytes for BTreeSet<V> {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = allocate_buffer(self)?;

        let num_keys = self.len() as u32;
        result.append(&mut num_keys.to_bytes()?);

        for value in self.iter() {
            result.append(&mut value.to_bytes()?);
        }

        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        U32_SERIALIZED_LENGTH + self.iter().map(|v| v.serialized_length()).sum::<usize>()
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

        let num_keys = self.len() as u32;
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
            None => Ok(vec![0]),
            Some(v) => {
                let mut result = allocate_buffer(self)?;
                result.push(1);

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
}

impl<T: FromBytes> FromBytes for Option<T> {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (tag, rem) = u8::from_bytes(bytes)?;
        match tag {
            0 => Ok((None, rem)),
            1 => {
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
            Err(error) => (0, error.to_bytes()?),
            Ok(result) => (1, result.to_bytes()?),
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
}

impl<T: FromBytes, E: FromBytes> FromBytes for Result<T, E> {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (variant, rem) = u8::from_bytes(bytes)?;
        match variant {
            0 => {
                let (value, rem) = E::from_bytes(rem)?;
                Ok((Err(value), rem))
            }
            1 => {
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
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        if self.len() > u32::max_value() as usize - U32_SERIALIZED_LENGTH {
            return Err(Error::OutOfMemory);
        }
        self.as_bytes().to_vec().into_bytes()
    }

    fn serialized_length(&self) -> usize {
        U32_SERIALIZED_LENGTH + self.as_bytes().len()
    }
}

impl ToBytes for &str {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        (*self).to_bytes()
    }

    fn serialized_length(&self) -> usize {
        (*self).serialized_length()
    }
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
    let deserialized = deserialize::<T>(serialized).expect("Unable to deserialize data");
    assert!(*t == deserialized)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[test]
    fn check_array_from_bytes_doesnt_leak() {
        thread_local!(static INSTANCE_COUNT: RefCell<usize> = RefCell::new(0));
        const MAX_INSTANCES: usize = 10;

        struct LeakChecker;

        impl FromBytes for LeakChecker {
            fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
                let instance_num = INSTANCE_COUNT.with(|count| *count.borrow());
                if instance_num >= MAX_INSTANCES {
                    Err(Error::Formatting)
                } else {
                    INSTANCE_COUNT.with(|count| *count.borrow_mut() += 1);
                    Ok((LeakChecker, bytes))
                }
            }
        }

        impl Drop for LeakChecker {
            fn drop(&mut self) {
                INSTANCE_COUNT.with(|count| *count.borrow_mut() -= 1);
            }
        }

        // Check we can construct an array of `MAX_INSTANCES` of `LeakChecker`s.
        {
            let bytes = (MAX_INSTANCES as u32).to_bytes().unwrap();
            let _array = <[LeakChecker; MAX_INSTANCES]>::from_bytes(&bytes).unwrap();
            // Assert `INSTANCE_COUNT == MAX_INSTANCES`
            INSTANCE_COUNT.with(|count| assert_eq!(MAX_INSTANCES, *count.borrow()));
        }

        // Assert the `INSTANCE_COUNT` has dropped to zero again.
        INSTANCE_COUNT.with(|count| assert_eq!(0, *count.borrow()));

        // Try to construct an array of `LeakChecker`s where the `MAX_INSTANCES + 1`th instance
        // returns an error.
        let bytes = (MAX_INSTANCES as u32 + 1).to_bytes().unwrap();
        let result = <[LeakChecker; MAX_INSTANCES + 1]>::from_bytes(&bytes);
        assert!(result.is_err());

        // Assert the `INSTANCE_COUNT` has dropped to zero again.
        INSTANCE_COUNT.with(|count| assert_eq!(0, *count.borrow()));
    }
}

#[cfg(test)]
mod proptests {
    use std::vec::Vec;

    use proptest::{collection::vec, prelude::*};

    use crate::{
        bytesrepr::{self, FromBytes, ToBytes, U32_SERIALIZED_LENGTH},
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
        fn test_vec_u8(u in vec(any::<u8>(), 1..100)) {
            bytesrepr::test_serialization_roundtrip(&u);
        }

        #[test]
        fn test_vec_i32(u in vec(any::<i32>(), 1..100)) {
            bytesrepr::test_serialization_roundtrip(&u);
        }

        #[test]
        fn test_vec_vec_u8(u in vec(vec(any::<u8>(), 1..100), 10)) {
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
    }

    #[test]
    fn vec_u8_from_bytes() {
        let data: Vec<u8> = vec![1, 2, 3, 4, 5];
        let data_bytes = data.to_bytes().unwrap();
        assert!(Vec::<u8>::from_bytes(&data_bytes[..U32_SERIALIZED_LENGTH / 2]).is_err());
        assert!(Vec::<u8>::from_bytes(&data_bytes[..U32_SERIALIZED_LENGTH]).is_err());
        assert!(Vec::<u8>::from_bytes(&data_bytes[..U32_SERIALIZED_LENGTH + 2]).is_err());
    }
}
