// #![feature(wasm_import_memory)]

// #[linkage = "--import-memory"]

pub mod abi;
pub mod field;
pub mod host;
pub mod schema;
pub mod storage;

use std::{
    cell::RefCell,
    collections::BTreeMap,
    fmt, io,
    marker::PhantomData,
    ptr::{self, NonNull},
};

use borsh::{BorshDeserialize, BorshSerialize};

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

use host::{CallError, CreateResult};

use vm_common::flags::EntryPointFlags;

pub fn reserve_vec_space(vec: &mut Vec<u8>, size: usize) -> Option<ptr::NonNull<u8>> {
    *vec = Vec::with_capacity(size);
    unsafe {
        vec.set_len(size);
    }
    NonNull::new(vec.as_mut_ptr())
}

pub trait Contract {
    fn new() -> Self;
    fn name() -> &'static str;
    fn create(
        entry_point: Option<&str>,
        input_data: Option<&[u8]>,
    ) -> Result<CreateResult, CallError>;
}

pub use field::Field;

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
