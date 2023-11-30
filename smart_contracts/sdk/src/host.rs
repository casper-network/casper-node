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

pub fn revert<T: BorshSerialize>(value: T) -> T {
    let data = borsh::to_vec(&value).expect("Revert value should serialize");
    casper_return(ReturnFlags::REVERT, Some(data.as_slice()))
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

pub fn call<Ret: BorshDeserialize>(
    contract_address: &Address,
    value: u64,
    entry_point_name: &str,
    args: &[u8],
) -> Result<CallResult<Ret>, CallError> {
    todo!()
    // let (data, result)  = casper_call(contract_address, value, entry_point_name, args);
    // match result {
    //     ResultCode::Success => Ok(CallResult { data: data.unwrap_or_default(), result, marker:
    // PhantomData }),     ResultCode::CalleeReverted => Ok(CallResult { data:
    // data.unwrap_or_default(), result, marker: PhantomData }),     ResultCode::CalleeTrapped
    // => Err(CallError::CalleeTrapped),     ResultCode::CalleeGasDepleted =>
    // Err(CallError::CalleeGasDepleted),     ResultCode::Unknown => Err(CallError::Unknown),
    // }
    // match result {
    //     Ok((result_code, data)) => match result_code {
    //         ResultCode::Success => Ok(data),
    //         ResultCode::CalleeReverted => Err(CallError::CalleeReverted),
    //         ResultCode::CalleeTrapped => Err(CallError::CalleeTrapped),
    //         ResultCode::CalleeGasDepleted => Err(CallError::CalleeGasDepleted),
    //         ResultCode::Unknown => Err(CallError::Unknown),
    //     },
    //     Err(_) => Err(CallError::Unknown),
    // }
}
