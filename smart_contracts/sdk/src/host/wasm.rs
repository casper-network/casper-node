use crate::{host::Address, reserve_vec_space, storage::Keyspace};
use std::{
    ffi::c_void,
    mem::MaybeUninit,
    ptr::{self, NonNull},
};

use super::{CallError, CreateResult, Entry, Error, Manifest, ResultCode};
use borsh::BorshSerialize;
use vm_common::flags::ReturnFlags;

pub(crate) mod ext {
    use crate::{
        host::{CreateResult, Manifest},
        storage::Keyspace,
    };
    use std::ffi::c_void;

    #[derive(Debug)]
    #[repr(C)]
    pub struct ReadInfo {
        pub(crate) data: *const u8,
        /// Size in bytes.
        pub(crate) size: usize,
        /// Value tag.
        pub(crate) tag: u64,
    }

    extern "C" {
        // pub fn legacy_interface();
        // pub fn casper_new_uref();
        pub fn casper_read(
            key_space: u64,
            key_ptr: *const u8,
            key_size: usize,
            info: *mut ReadInfo,
            alloc: extern "C" fn(usize, *mut c_void) -> *mut u8,
            alloc_ctx: *const c_void,
        ) -> i32;
        pub fn casper_write(
            key_space: u64,
            key_ptr: *const u8,
            key_size: usize,
            value_tag: u64,
            value_ptr: *const u8,
            value_size: usize,
        ) -> i32;
        pub fn casper_print(msg_ptr: *const u8, msg_size: usize) -> i32;
        pub fn casper_return(flags: u32, data_ptr: *const u8, data_len: usize) -> !;
        pub fn casper_copy_input(
            alloc: extern "C" fn(usize, *mut c_void) -> *mut u8,
            alloc_ctx: *const c_void,
        ) -> *mut u8;
        pub fn casper_copy_output(output_ptr: *const u8, output_len: usize); // todo
        pub fn casper_create_contract(
            code_ptr: *const u8,
            code_size: usize,
            manifest_ptr: *const Manifest,
            entry_point_ptr: *const u8,
            entry_point_size: usize,
            input_ptr: *const u8,
            input_size: usize,
            result_ptr: *mut CreateResult,
        ) -> u32;

        pub fn casper_call(
            // acct_or_contract,
            address_ptr: *const u8,
            address_size: usize,
            value: u64,
            entry_point_ptr: *const u8, // nullptr
            entry_point_size: usize,
            input_ptr: *const u8,
            input_size: usize,
            alloc: extern "C" fn(usize, *mut c_void) -> *mut u8, // For capturing output data
            alloc_ctx: *const c_void,
        ) -> u32;
    }
}

pub fn casper_print(msg: &str) {
    let _res = unsafe { ext::casper_print(msg.as_ptr(), msg.len()) };
}

pub fn capser_return(flags: u32, data: &[u8]) -> ! {
    unsafe { ext::casper_return(flags, data.as_ptr(), data.len()) }
}

extern "C" fn alloc_callback<F: FnOnce(usize) -> Option<ptr::NonNull<u8>>>(
    len: usize,
    ctx: *mut c_void,
) -> *mut u8 {
    let opt_closure = ctx as *mut Option<F>;
    let allocated_ptr = unsafe { (*opt_closure).take().unwrap()(len) };
    match allocated_ptr {
        Some(ptr) => ptr.as_ptr(),
        None => ptr::null_mut(),
    }
}

/// Provided callback should ensure that it can provide a pointer that can store `size` bytes.
/// Function returns last pointer after writing data, or None otherwise.
pub fn copy_input_into<F: FnOnce(usize) -> Option<ptr::NonNull<u8>>>(
    alloc: Option<F>,
) -> Option<NonNull<u8>> {
    let ret =
        unsafe { ext::casper_copy_input(alloc_callback::<F>, &alloc as *const _ as *mut c_void) };
    NonNull::<u8>::new(ret)
}

pub fn casper_copy_input() -> Vec<u8> {
    let mut vec = Vec::new();
    let last_ptr = copy_input_into(Some(|size| reserve_vec_space(&mut vec, size)));
    last_ptr.unwrap();
    vec
}

pub fn copy_input_dest(dest: &mut [u8]) -> Option<&[u8]> {
    let last_ptr = copy_input_into(Some(|size| {
        if size > dest.len() {
            None
        } else {
            // SAFETY: `dest` is guaranteed to be non-null and large enough to hold `size`
            // bytes.
            Some(unsafe { ptr::NonNull::new_unchecked(dest.as_mut_ptr()) })
        }
    }));

    let end_ptr = last_ptr?;
    let length = unsafe { end_ptr.as_ptr().offset_from(dest.as_mut_ptr()) };
    let length: usize = length.try_into().unwrap();
    Some(&dest[..length])
}

pub fn casper_return(flags: ReturnFlags, data: Option<&[u8]>) -> ! {
    let (data_ptr, data_len) = match data {
        Some(data) => (data.as_ptr(), data.len()),
        None => (ptr::null(), 0),
    };
    unsafe { ext::casper_return(flags.bits(), data_ptr, data_len) };
    unreachable!()
}

pub fn casper_read<F: FnOnce(usize) -> Option<ptr::NonNull<u8>>>(
    key: Keyspace,
    f: F,
) -> Result<Option<Entry>, Error> {
    let (key_space, key_bytes) = match key {
        Keyspace::State => (0, &[][..]),
        Keyspace::Context(key_bytes) => (1, key_bytes),
    };

    let mut info = ext::ReadInfo {
        data: ptr::null(),
        size: 0,
        tag: 0,
    };

    extern "C" fn alloc_cb<F: FnOnce(usize) -> Option<ptr::NonNull<u8>>>(
        len: usize,
        ctx: *mut c_void,
    ) -> *mut u8 {
        let opt_closure = ctx as *mut Option<F>;
        let allocated_ptr = unsafe { (*opt_closure).take().unwrap()(len) };
        match allocated_ptr {
            Some(mut ptr) => unsafe { ptr.as_mut() },
            None => ptr::null_mut(),
        }
    }

    let ctx = &Some(f) as *const _ as *mut _;

    let ret = unsafe {
        ext::casper_read(
            key_space,
            key_bytes.as_ptr(),
            key_bytes.len(),
            &mut info as *mut ext::ReadInfo,
            alloc_cb::<F>,
            ctx,
        )
    };

    if ret == 0 {
        Ok(Some(Entry { tag: info.tag }))
    } else if ret == 1 {
        Ok(None)
    } else {
        Err(Error::Foo)
    }
}

pub fn casper_write(key: Keyspace, value_tag: u64, value: &[u8]) -> Result<(), Error> {
    let (key_space, key_bytes) = match key {
        Keyspace::State => (0, &[][..]),
        Keyspace::Context(key_bytes) => (1, key_bytes),
    };
    let _ret = unsafe {
        ext::casper_write(
            key_space,
            key_bytes.as_ptr(),
            key_bytes.len(),
            value_tag,
            value.as_ptr(),
            value.len(),
        )
    };
    Ok(())
}

pub fn casper_create(
    code: Option<&[u8]>,
    manifest: &Manifest,
    entry_point: Option<&str>,
    input_data: Option<&[u8]>,
) -> Result<CreateResult, CallError> {
    let (code_ptr, code_size): (*const u8, usize) = match code {
        Some(code) => (code.as_ptr(), code.len()),
        None => (ptr::null(), 0),
    };

    let mut result = MaybeUninit::uninit();

    let manifest_ptr = NonNull::from(manifest);

    let result_code = unsafe {
        ext::casper_create_contract(
            code_ptr,
            code_size,
            manifest_ptr.as_ptr(),
            entry_point.map(|s| s.as_ptr()).unwrap_or(ptr::null()),
            entry_point.map(|s| s.len()).unwrap_or(0),
            input_data.map(|s| s.as_ptr()).unwrap_or(ptr::null()),
            input_data.map(|s| s.len()).unwrap_or(0),
            result.as_mut_ptr(),
        )
    };

    match ResultCode::from(result_code) {
        ResultCode::Success => {
            let result = unsafe { result.assume_init() };
            Ok(result)
        }
        ResultCode::CalleeReverted => Err(CallError::CalleeReverted),
        ResultCode::CalleeTrapped => Err(CallError::CalleeTrapped),
        ResultCode::CalleeGasDepleted => Err(CallError::CalleeGasDepleted),
        ResultCode::Unknown => Err(CallError::Unknown),
    }
}

pub(crate) fn call_into<F: FnOnce(usize) -> Option<ptr::NonNull<u8>>>(
    address: &Address,
    value: u64,
    entry_point: &str,
    input_data: &[u8],
    alloc: Option<F>,
) -> ResultCode {
    let result_code = unsafe {
        ext::casper_call(
            address.as_ptr(),
            address.len(),
            value,
            entry_point.as_ptr(),
            entry_point.len(),
            input_data.as_ptr(),
            input_data.len(),
            alloc_callback::<F>,
            &alloc as *const _ as *mut _,
        )
    };
    ResultCode::from(result_code)
}

#[derive(Debug)]
pub struct CallResult {
    pub result_code: ResultCode,
    pub output: Vec<u8>,
}

impl CallResult {
    pub fn did_revert(&self) -> bool {
        self.result_code == ResultCode::CalleeReverted
    }
}

pub fn casper_call(
    address: &Address,
    value: u64,
    entry_point: &str,
    input_data: &[u8],
) -> (Option<Vec<u8>>, ResultCode) {
    let mut output = None;
    let result_code = call_into(
        address,
        value,
        entry_point,
        input_data,
        Some(|size| {
            let mut vec = Vec::new();
            reserve_vec_space(&mut vec, size);
            let result = Some(unsafe { ptr::NonNull::new_unchecked(vec.as_mut_ptr()) });
            output = Some(vec);
            result
        }),
    );
    (output, result_code)
}
