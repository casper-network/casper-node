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

#[repr(C, packed)]
#[derive(Copy, Clone, Debug)]
pub struct EntryPoint {
    pub name_ptr: *const u8,
    pub name_len: usize,

    pub params_ptr: *mut Param, // pointer of pointers (preferred 'static lifetime)
    pub params_size: usize,

    pub fptr: extern "C" fn() -> (), // extern "C" fn(A1) -> (),
}

#[repr(C)]
#[derive(Debug)]
pub struct Manifest {
    pub entry_points: *mut [EntryPoint],
    pub entry_points_size: usize,
}
#[repr(C)]
#[derive(Debug, Default)]
pub struct CreateResult {
    package_address: [u8; 32],
    contract_address: [u8; 32],
    version: u32,
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use bytes::Bytes;

    use std::{
        alloc::{self, Layout},
        ffi::c_void,
        mem::{self, MaybeUninit},
        ptr::{self, NonNull},
    };

    use crate::reserve_vec_space;

    use super::{Address, CreateResult, Entry, Error, Manifest};

    #[derive(Debug)]
    #[repr(C)]
    pub struct ReadInfo {
        data: *const u8,
        /// Size in bytes.
        size: usize,
        /// Value tag.
        tag: u64,
    }

    extern "C" {
        pub fn casper_read(
            key_space: u64,
            key_ptr: *const u8,
            key_size: usize,
            info: *mut ReadInfo,
            alloc: extern "C" fn(usize, *mut c_void) -> *const u8,
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
        pub fn casper_revert(code: u32);
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
    }

    pub fn print(msg: &str) {
        let _res = unsafe { casper_print(msg.as_ptr(), msg.len()) };
    }

    pub fn revert(code: u32) -> ! {
        unsafe { casper_revert(code) };
        unreachable!()
    }

    extern "C" fn alloc_callback<F: FnOnce(usize) -> *mut u8>(
        len: usize,
        ctx: *mut c_void,
    ) -> *mut u8 {
        let opt_closure = ctx as *mut Option<F>;
        let mut ptr = unsafe { (*opt_closure).take().unwrap()(len) };
        ptr
    }

    /// Provided callback should ensure that it can provide a pointer that can store `size` bytes.
    /// Function returns last pointer after writing data, or None otherwise.
    pub fn copy_input_into<F: FnOnce(usize) -> *mut u8>(alloc: Option<F>) -> Option<NonNull<u8>> {
        let ret =
            unsafe { casper_copy_input(alloc_callback::<F>, &alloc as *const _ as *mut c_void) };
        NonNull::<u8>::new(ret)
    }

    pub fn copy_input() -> Vec<u8> {
        let mut vec = Vec::new();
        let last_ptr = copy_input_into(Some(|size| {
            print(&format!("callback alloc called {size}"));
            reserve_vec_space(&mut vec, size)
        }));
        last_ptr.unwrap();
        vec
    }

    pub fn copy_input_dest(dest: &mut [u8]) -> Option<&[u8]> {
        let last_ptr = copy_input_into(Some(|size| {
            if size > dest.len() {
                return ptr::null_mut();
            } else {
                dest.as_mut_ptr()
            }
        }));

        match last_ptr {
            Some(end_ptr) => {
                let length = unsafe { end_ptr.as_ptr().offset_from(dest.as_mut_ptr()) };
                let length: usize = length.try_into().unwrap();
                Some(&dest[..length])
            }
            None => None,
        }
    }

    pub fn read_into<'a>(
        key_space: u64,
        key: &[u8],
        destination: &'a mut [u8],
    ) -> Option<&'a [u8]> {
        let mut what_size: Option<usize> = None;
        read(key_space, key, |size| {
            what_size = Some(size);
            destination.as_mut_ptr()
        });

        let size = what_size?;

        Some(&destination[..size])
    }

    pub fn read<F: FnOnce(usize) -> *mut u8>(
        key_space: u64,
        key: &[u8],
        f: F,
    ) -> Result<Option<Entry>, Error> {
        // let mut info = MaybeUninit::uninit();
        let mut info = ReadInfo {
            data: ptr::null(),
            size: 0,
            tag: 0,
        };

        extern "C" fn alloc_cb<F: FnOnce(usize) -> *mut u8>(
            len: usize,
            ctx: *mut c_void,
        ) -> *const u8 {
            let opt_closure = ctx as *mut Option<F>;
            unsafe { (*opt_closure).take().unwrap()(len) }
        }

        let ctx = &Some(f) as *const _ as *mut c_void;

        let ret = unsafe {
            casper_read(
                key_space,
                key.as_ptr(),
                key.len(),
                &mut info as *mut ReadInfo,
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

    pub fn write(key_space: u64, key: &[u8], value_tag: u64, value: &[u8]) -> Result<(), Error> {
        let _ret = unsafe {
            casper_write(
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

    // pub fn create_entity() -> Address { todo!() }

    // struct UpgradeResult {

    // }

    pub fn create(code: Option<&[u8]>, manifest: &Manifest) -> Result<CreateResult, Error> {
        let (code_ptr, code_size): (*const u8, usize) = match code {
            Some(code) => (code.as_ptr(), code.len()),
            None => (ptr::null(), 0),
        };

        let mut result = MaybeUninit::uninit();

        let manifest_ptr = NonNull::from(manifest);

        let ret = unsafe {
            casper_create_contract(
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
            Err(Error::Foo)
        }
    }

    // pub fn upgrade(package_address: &[u8], code: Option<&[u8]>) -> (ContractHash, u32) {
    //     todo!()
    // }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::{
        borrow::{Borrow, BorrowMut},
        cell::RefCell,
        collections::BTreeMap,
        ptr,
    };

    use bytes::Bytes;

    use super::{CreateResult, Entry, Error, Manifest};

    #[derive(PartialEq, Eq, PartialOrd, Ord, Clone)]
    struct TaggedValue {
        tag: u64,
        value: Bytes,
    }

    #[derive(PartialEq, Eq, PartialOrd, Ord, Clone)]
    struct BorrowedTaggedValue<'a> {
        tag: u64,
        value: &'a [u8],
    }
    type Container = BTreeMap<u64, BTreeMap<Bytes, TaggedValue>>;

    #[derive(Default, Clone)]
    pub(crate) struct LocalKV {
        db: Container,
    }

    // impl LocalKV {
    //     pub(crate) fn update(&mut self, db: LocalKV) {
    //         self.db = db.db
    //     }
    // }

    thread_local! {
        static DB: RefCell<LocalKV> = RefCell::new(LocalKV::default());
    }

    pub fn print(msg: &str) {
        println!("💻 {msg}");
    }
    pub fn write(key_space: u64, key: &[u8], value_tag: u64, value: &[u8]) -> Result<(), Error> {
        DB.with(|db| {
            db.borrow_mut().db.entry(key_space).or_default().insert(
                Bytes::copy_from_slice(key),
                TaggedValue {
                    tag: value_tag,
                    value: Bytes::copy_from_slice(value),
                },
            );
        });
        Ok(())
    }
    pub fn read(
        key_space: u64,
        key: &[u8],
        func: impl FnOnce(usize) -> Option<ptr::NonNull<u8>>,
    ) -> Result<Option<Entry>, Error> {
        let value = DB.with(|db| db.borrow().db.get(&key_space)?.get(key).cloned());
        match value {
            Some(tagged_value) => {
                let entry = Entry {
                    tag: tagged_value.tag,
                };

                let ptr = func(tagged_value.value.len());

                if let Some(ptr) = ptr {
                    unsafe {
                        ptr::copy_nonoverlapping(
                            tagged_value.value.as_ptr(),
                            ptr.as_ptr(),
                            tagged_value.value.len(),
                        );
                    }
                }

                Ok(Some(entry))
            }
            None => Ok(None),
        }
    }

    // pub fn dispatch<Args, R>(export: impl Fn(Args) -> R, args: Args) -> R {
    //     export(args)
    // }
    pub fn revert(code: u32) -> ! {
        panic!("revert with code {code}")
    }

    pub fn create(code: Option<&[u8]>, manifest: &Manifest) -> Result<CreateResult, Error> {
        todo!()
    }
}

use core::slice;
use std::ffi::c_void;

use bytes::Bytes;

#[cfg(not(target_arch = "wasm32"))]
pub use native::{create, print, read, revert, write};
#[cfg(target_arch = "wasm32")]
pub use wasm::{copy_input, create, print, read, revert, write};
