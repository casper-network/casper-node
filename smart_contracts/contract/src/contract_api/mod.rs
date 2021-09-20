//! Contains support for writing smart contracts.

pub mod account;
pub mod runtime;
pub mod storage;
pub mod system;

use alloc::{
    alloc::{alloc, Layout},
    vec::Vec,
};
use core::{mem, ptr::NonNull};

use casper_types::{bytesrepr::ToBytes, ApiError};

use crate::unwrap_or_revert::UnwrapOrRevert;

/// Calculates size and alignment for an array of T.
const fn size_align_for_array<T>(n: usize) -> (usize, usize) {
    (n * mem::size_of::<T>(), mem::align_of::<T>())
}

/// Allocates bytes
pub fn alloc_bytes(n: usize) -> NonNull<u8> {
    let (size, align) = size_align_for_array::<u8>(n);
    // We treat allocated memory as raw bytes, that will be later passed to deserializer which also
    // operates on raw bytes.
    let layout = Layout::from_size_align(size, align)
        .map_err(|_| ApiError::AllocLayout)
        .unwrap_or_revert();
    let raw_ptr = unsafe { alloc(layout) };
    NonNull::new(raw_ptr)
        .ok_or(ApiError::OutOfMemory)
        .unwrap_or_revert()
}

fn to_ptr<T: ToBytes>(t: T) -> (*const u8, usize, Vec<u8>) {
    let bytes = t.into_bytes().unwrap_or_revert();
    let ptr = bytes.as_ptr();
    let size = bytes.len();
    (ptr, size, bytes)
}

fn dictionary_item_key_to_ptr(dictionary_item_key: &str) -> (*const u8, usize) {
    let bytes = dictionary_item_key.as_bytes();
    let ptr = bytes.as_ptr();
    let size = bytes.len();
    (ptr, size)
}
