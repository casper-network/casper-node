use crate::{host::Address, reserve_vec_space};
use std::{
    ffi::c_void,
    mem::MaybeUninit,
    ptr::{self, NonNull},
};

use super::{CallError, CreateResult, Entry, Error, Manifest, ResultCode};

pub(crate) mod ext {
    use crate::host::{CreateResult, Manifest};
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
            manifest_ptr: *mut Manifest,
            result_ptr: *mut CreateResult,
        ) -> i32;

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

pub fn casper_return(flags: u32, data: &[u8]) -> ! {
    unsafe { ext::casper_return(flags, data.as_ptr(), data.len()) };
    unreachable!()
}

pub fn casper_read<F: FnOnce(usize) -> Option<ptr::NonNull<u8>>>(
    key_space: u64,
    key: &[u8],
    f: F,
) -> Result<Option<Entry>, Error> {
    // let mut info = MaybeUninit::uninit();
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
            key.as_ptr(),
            key.len(),
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

pub fn casper_write(key_space: u64, key: &[u8], value_tag: u64, value: &[u8]) -> Result<(), Error> {
    let _ret = unsafe {
        ext::casper_write(
            key_space,
            key.as_ptr(),
            key.len(),
            value_tag,
            value.as_ptr(),
            value.len(),
        )
    };
    Ok(())
}

pub fn casper_create(code: Option<&[u8]>, manifest: &Manifest) -> Result<CreateResult, Error> {
    let (code_ptr, code_size): (*const u8, usize) = match code {
        Some(code) => (code.as_ptr(), code.len()),
        None => (ptr::null(), 0),
    };

    let mut result = MaybeUninit::uninit();

    let manifest_ptr = NonNull::from(manifest);

    let ret = unsafe {
        ext::casper_create_contract(
            code_ptr,
            code_size,
            manifest_ptr.as_ptr(),
            result.as_mut_ptr(),
        )
    };
    if ret == 0 {
        let result = unsafe { result.assume_init() };
        Ok(result)
    } else {
        Err(Error::Foo) // todo: host -> wasm error handling
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

pub fn casper_call(
    address: &Address,
    value: u64,
    entry_point: &str,
    input_data: &[u8],
) -> Result<Vec<u8>, CallError> {
    let mut vec = Vec::new();
    let result_code = call_into(
        address,
        value,
        entry_point,
        input_data,
        Some(|size| {
            reserve_vec_space(&mut vec, size);
            Some(unsafe { ptr::NonNull::new_unchecked(vec.as_mut_ptr()) })
        }),
    );
    casper_print(&format!("result_code: {:?} vec={:?}", result_code, vec));
    match result_code {
        ResultCode::Success => Ok(vec),
        ResultCode::CalleeReverted => Ok(vec),
        ResultCode::CalleeTrapped => Err(CallError::CalleeTrapped),
        ResultCode::CalleeGasDepleted => Err(CallError::CalleeGasDepleted),
        ResultCode::Unknown => Err(CallError::Unknown),
    }
}
