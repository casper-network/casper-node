// #![feature(wasm_import_memory)]

// #[linkage = "--import-memory"]

pub mod abi;
#[cfg(feature = "cli")]
pub mod cli;
pub mod collections;
pub mod host;
pub mod schema;
pub mod storage;
pub mod types;

use std::{io, ptr::NonNull};

use borsh::BorshSerialize;
pub use casper_sdk_sys as sys;
use sys::CreateResult;
use types::CallError;

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Selector(u32);

impl Selector {
    pub const fn new(selector: u32) -> Self {
        Selector(selector)
    }

    pub const fn get(&self) -> u32 {
        self.0
    }
}

#[cfg(target_arch = "wasm32")]
fn hook_impl(info: &std::panic::PanicInfo) {
    let msg = info.to_string();
    host::casper_print(&msg);
}

#[cfg(target_arch = "wasm32")]
#[inline]
pub fn set_panic_hook() {
    use std::sync::Once;
    static SET_HOOK: Once = Once::new();
    SET_HOOK.call_once(|| {
        std::panic::set_hook(Box::new(hook_impl));
    });
}

pub fn reserve_vec_space(vec: &mut Vec<u8>, size: usize) -> Option<NonNull<u8>> {
    if size == 0 {
        None
    } else {
        *vec = Vec::with_capacity(size);
        unsafe {
            vec.set_len(size);
        }
        NonNull::new(vec.as_mut_ptr())
    }
}

pub trait ToCallData {
    const SELECTOR: Selector;
    fn input_data(&self) -> Option<Vec<u8>>;
}

/// To derive this contract you have to use `#[casper]` macro on top of impl block.
///
/// This proc macro handles generation of a manifest.
pub trait Contract {
    fn name() -> &'static str;
    fn create<T: ToCallData>(call_data: T) -> Result<CreateResult, CallError>;
    fn default_create() -> Result<CreateResult, CallError>;
}

#[derive(Debug)]
pub enum Access {
    Private,
    Public,
}

#[derive(Debug)]
pub enum ApiError {
    Error1,
    Error2,
    MissingArgument,
    Io(io::Error),
}

// A println! like macro that calls `host::print` function.
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => ({
        $crate::host::casper_print(&format!($($arg)*));
    })
}

#[macro_export]
macro_rules! revert {
    () => {{
        casper_sdk::host::casper_return(vm_common::flags::ReturnFlags::REVERT, None);
        unreachable!()
    }};
    ($arg:expr) => {{
        let value = $arg;
        let data = borsh::to_vec(&value).expect("Revert value should serialize");
        casper_sdk::host::casper_return(
            vm_common::flags::ReturnFlags::REVERT,
            Some(data.as_slice()),
        );
        #[allow(unreachable_code)]
        value
    }};
}

pub trait UnwrapOrRevert<T> {
    /// Unwraps the value into its inner type or calls [`runtime::revert`] with a
    /// predetermined error code on failure.
    fn unwrap_or_revert(self) -> T;
}

impl<T, E> UnwrapOrRevert<T> for Result<T, E>
where
    E: BorshSerialize,
{
    fn unwrap_or_revert(self) -> T {
        self.unwrap_or_else(|error| {
            let error_data = borsh::to_vec(&error).expect("Revert value should serialize");
            host::casper_return(
                vm_common::flags::ReturnFlags::REVERT,
                Some(error_data.as_slice()),
            );
            unreachable!("Support for unwrap_or_revert")
        })
    }
}
