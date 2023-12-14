use core::slice;
use std::{marker::PhantomData, ptr::NonNull};

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
    // pub ty: u32,
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

    pub flags: u32,
}

#[repr(C)]
#[derive(Debug)]
pub struct Manifest {
    pub entry_points: *const EntryPoint,
    pub entry_points_size: usize,
}
#[repr(C)]
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CreateResult {
    pub package_address: [u8; 32],
    pub contract_address: [u8; 32],
    pub version: u32,
}

#[repr(C)]
#[derive(Debug, PartialEq)]
pub enum ResultCode {
    /// Called contract returned successfully.
    Success = 0,
    /// Callee contract reverted.
    CalleeReverted = 1,
    /// Called contract trapped.
    CalleeTrapped = 2,
    /// Called contract reached gas limit.
    CalleeGasDepleted = 3,
    Unknown,
}

impl From<u32> for ResultCode {
    fn from(value: u32) -> Self {
        match value {
            0 => ResultCode::Success,
            1 => ResultCode::CalleeReverted,
            2 => ResultCode::CalleeTrapped,
            3 => ResultCode::CalleeGasDepleted,
            _ => ResultCode::Unknown,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum CallError {
    CalleeReverted,
    CalleeTrapped,
    CalleeGasDepleted,
    Unknown,
}

#[cfg(target_arch = "wasm32")]
mod wasm;

#[cfg(not(target_arch = "wasm32"))]
mod native;

use borsh::{BorshDeserialize, BorshSerialize};

use vm_common::flags::ReturnFlags;
#[cfg(target_arch = "wasm32")]
pub use wasm::{
    casper_call, casper_copy_input, casper_create, casper_print, casper_read, casper_return,
    casper_write,
};

#[cfg(not(target_arch = "wasm32"))]
pub use native::{
    casper_call, casper_copy_input, casper_create, casper_print, casper_read, casper_return,
    casper_write,
};

use crate::{reserve_vec_space, storage::Keyspace, Contract};

pub fn read_state<T: Default + BorshDeserialize + Contract>() -> Result<T, Error> {
    let mut vec = Vec::new();
    let read_info = casper_read(Keyspace::State, |size| reserve_vec_space(&mut vec, size))?;
    match read_info {
        Some(_input) => Ok(borsh::from_slice(&vec).unwrap()),
        None => Ok(T::default()),
    }
}

pub fn write_state<T: Contract + BorshSerialize>(state: &T) -> Result<(), Error> {
    casper_write(Keyspace::State, 0, &borsh::to_vec(state).unwrap())?;
    Ok(())
}

/// TODO: Remove once procedural macros are improved, this is just to save the boilerplate when
/// doing things manually.
pub fn start<Args: BorshDeserialize, Ret: BorshSerialize>(func: impl Fn(Args) -> Ret) {
    // Set panic hook (assumes std is enabled etc.)
    #[cfg(target_arch = "wasm32")]
    {
        crate::set_panic_hook();
    }
    let input = casper_copy_input();
    let args: Args = BorshDeserialize::try_from_slice(&input).unwrap();
    let result = func(args);
    let serialized_result = borsh::to_vec(&result).unwrap();
    casper_return(ReturnFlags::empty(), Some(serialized_result.as_slice()));
}

pub struct CallResult<T: BorshDeserialize> {
    data: Vec<u8>,
    result: ResultCode,
    marker: PhantomData<T>,
}

pub fn call<Args: BorshSerialize, Ret: BorshDeserialize>(
    contract_address: &Address,
    value: u64,
    entry_point_name: &str,
    args: Args,
) -> Result<CallResult<Ret>, CallError> {
    let input_data = borsh::to_vec(&args).unwrap();
    let (maybe_data, result_code) =
        casper_call(contract_address, value, entry_point_name, &input_data);
    match result_code {
        ResultCode::Success | ResultCode::CalleeReverted => {
            let data = maybe_data.unwrap_or_default();
            Ok(CallResult {
                data,
                result: result_code,
                marker: PhantomData,
            })
        }
        ResultCode::CalleeTrapped => Err(CallError::CalleeTrapped),
        ResultCode::CalleeGasDepleted => Err(CallError::CalleeGasDepleted),
        ResultCode::Unknown => Err(CallError::Unknown),
    }
}
