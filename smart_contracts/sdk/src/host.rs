use core::slice;
use std::ptr::NonNull;

#[derive(Debug)]
pub enum Error {
    Foo,
    Bar,
}

#[derive(Debug)]
pub struct Entry {
    pub tag: u64,
}

#[repr(C)]
#[derive(Debug)]
pub struct Slice {
    ptr: *const u8,
    size: usize,
}

impl Slice {
    pub fn as_slice(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.ptr, self.size) }
    }
}
pub struct Param {
    pub name_ptr: *const u8,
    pub name_len: usize,
    pub ty: u32,
}

pub type Address = [u8; 32];

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct EntryPoint {
    pub name_ptr: *const u8,
    pub name_len: usize,

    pub params_ptr: *const Param, // pointer of pointers (preferred 'static lifetime)
    pub params_size: usize,

    pub fptr: extern "C" fn() -> (), // extern "C" fn(A1) -> (),
}

#[repr(C)]
#[derive(Debug)]
pub struct Manifest {
    pub entry_points: *const EntryPoint,
    pub entry_points_size: usize,
}
#[repr(C)]
#[derive(Debug, Default)]
pub struct CreateResult {
    pub package_address: [u8; 32],
    pub contract_address: [u8; 32],
    pub version: u32,
}

#[cfg(target_arch = "wasm32")]
mod wasm;

#[cfg(not(target_arch = "wasm32"))]
mod native;

use borsh::{BorshDeserialize, BorshSerialize};

#[cfg(target_arch = "wasm32")]
pub use wasm::{call, copy_input, create, print, read, revert, write};

#[cfg(not(target_arch = "wasm32"))]
pub use native::{call, copy_input, create, print, read, revert, write};

/// TODO: Remove once procedural macros are improved, this is just to save the boilerplate
pub fn start<Args: BorshDeserialize, Ret: BorshSerialize>(func: impl Fn(Args) -> Ret) {
    // Set panic hook (assumes std is enabled etc.)
    #[cfg(target_arch = "wasm32")]
    {
        crate::set_panic_hook();
    }
    let input = copy_input();
    let args: Args = BorshDeserialize::try_from_slice(&input).unwrap();
    let _ret = func(args);
}
