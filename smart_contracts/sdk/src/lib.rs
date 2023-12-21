// #![feature(wasm_import_memory)]

// #[linkage = "--import-memory"]

pub mod abi;
pub mod collections;
pub mod host;
pub mod schema;
pub mod storage;
pub mod types;

use std::{io, ptr::NonNull};

pub use casper_sdk_sys as sys;
use sys::CreateResult;
use types::CallError;

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
    *vec = Vec::with_capacity(size);
    unsafe {
        vec.set_len(size);
    }
    NonNull::new(vec.as_mut_ptr())
}

pub trait Contract {
    fn name() -> &'static str;
    fn create(
        entry_point: Option<&str>,
        input_data: Option<&[u8]>,
    ) -> Result<CreateResult, CallError>;
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
    () => {
        casper_sdk::host::casper_return(vm_common::flags::ReturnFlags::REVERT, None)
    };
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
