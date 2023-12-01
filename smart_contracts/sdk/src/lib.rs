// #![feature(wasm_import_memory)]
// #[linkage = "--import-memory"]

pub mod host;

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

#[cfg(not(target_arch = "wasm32"))]
pub mod schema_helper {
    #[derive(Copy, Clone)]
    pub struct Export {
        pub name: &'static str,
        pub fptr: fn(&[u8]) -> (),
    }

    impl std::fmt::Debug for Export {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Export").field("name", &self.name).finish()
        }
    }

    pub static mut LAZY_EXPORTS: [Option<Export>; 1024] = [None; 1024];

    pub fn register_export(export: Export) {
        for l in unsafe { LAZY_EXPORTS.iter_mut() } {
            if l.is_none() {
                *l = Some(export);
                return;
            }
        }
        assert!(false, "no more space");
    }

    pub fn list_exports() -> Vec<&'static Export> {
        let mut v = Vec::new();
        for export_name in unsafe { LAZY_EXPORTS.iter() } {
            if let Some(export_name) = export_name {
                v.push(export_name);
            }
        }
        v
    }

    pub fn dispatch(name: &str, input: &[u8]) {
        for export in list_exports() {
            if export.name == name {
                (export.fptr)(input);
                return;
            }
        }
        panic!("not found");
    }
}

const BOOL_TYPE_ID: u32 = 0;
const STRING_TYPE_ID: u32 = 1;
const UNIT_TYPE_ID: u32 = 2;
const ANY_TYPE_ID: u32 = 3;
const U32_TYPE_ID: u32 = 4;

#[repr(u32)]
#[derive(Debug)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Serialize, Deserialize))]
pub enum CLType {
    Bool,
    String,
    Unit,
    Any,
    U32,
}

pub trait CLTyped {
    const TYPE_ID: u32;
    fn cl_type() -> CLType;
}

impl CLTyped for String {
    const TYPE_ID: u32 = STRING_TYPE_ID;
    fn cl_type() -> CLType {
        CLType::String
    }
}
impl CLTyped for bool {
    const TYPE_ID: u32 = BOOL_TYPE_ID;
    fn cl_type() -> CLType {
        CLType::Bool
    }
}

impl CLTyped for () {
    const TYPE_ID: u32 = UNIT_TYPE_ID;
    fn cl_type() -> CLType {
        CLType::Unit
    }
}

impl CLTyped for u32 {
    const TYPE_ID: u32 = U32_TYPE_ID;
    fn cl_type() -> CLType {
        CLType::U32
    }
}

use host::{CreateResult, Error};
#[cfg(not(target_arch = "wasm32"))]
use serde::{Deserialize, Serialize};
#[derive(Debug)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Serialize, Deserialize))]
pub struct SchemaArgument {
    pub name: &'static str,
    pub ty: CLType,
}

#[derive(Debug)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Serialize, Deserialize))]

pub struct SchemaEntryPoint {
    pub name: &'static str,
    pub arguments: Vec<SchemaArgument>,
}

#[derive(Debug)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Serialize, Deserialize))]
pub struct SchemaData {
    pub name: &'static str,
    pub ty: CLType,
}

#[derive(Debug)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Serialize, Deserialize))]
pub struct Schema {
    pub name: &'static str,
    pub data: Vec<SchemaData>,
    pub entry_points: Vec<SchemaEntryPoint>,
    // pub exports: Vec<&'static str>,
    // pub exports: Vec<SchemaEntryPoint>,
}

#[derive(Debug)]
pub struct Value<T> {
    name: &'static str,
    key_space: u64,
    _marker: PhantomData<T>,
}

impl<T: CLTyped> CLTyped for Value<T> {
    const TYPE_ID: u32 = T::TYPE_ID;
    fn cl_type() -> CLType {
        T::cl_type()
    }
}

pub fn reserve_vec_space(vec: &mut Vec<u8>, size: usize) -> Option<ptr::NonNull<u8>> {
    *vec = Vec::with_capacity(size);
    unsafe {
        vec.set_len(size);
    }
    NonNull::new(vec.as_mut_ptr())
}

impl<T> Value<T> {
    pub fn new(name: &'static str, key_space: u64) -> Self {
        Self {
            name,
            key_space,
            _marker: PhantomData,
        }
    }
}

impl<T: BorshSerialize> Value<T> {
    pub fn write(&mut self, value: T) -> io::Result<()> {
        let v = borsh::to_vec(&value)?;
        host::casper_write(self.key_space, self.name.as_bytes(), 0, &v)
            .map_err(|_error| io::Error::new(io::ErrorKind::Other, "todo"))?;
        Ok(())
    }
}
impl<T: BorshDeserialize> Value<T> {
    pub fn read(&self) -> io::Result<Option<T>> {
        let mut read = None;
        host::casper_read(self.key_space, self.name.as_bytes(), |size| {
            *(&mut read) = Some(Vec::new());
            reserve_vec_space(read.as_mut().unwrap(), size)
        })
        .map_err(|_error| io::Error::new(io::ErrorKind::Other, "todo"))?;
        match read {
            Some(read) => {
                let value = T::deserialize(&mut read.as_slice())?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }
}

pub trait Contract {
    fn new() -> Self;
    fn name() -> &'static str;
    fn schema() -> Schema;
    fn create() -> Result<CreateResult, Error>;
}

#[derive(Debug)]
pub enum Access {
    Private,
    Public,
}

#[derive(Debug)]
pub struct EntryPoint<'a, F: Fn()> {
    pub name: &'a str,
    pub params: &'a [(&'a str, CLType)],
    // pub access: Access,
    // fptr: extern "C" fn() -> (),
    pub func: F,
}

#[derive(Debug)]
pub enum ApiError {
    Error1,
    Error2,
    MissingArgument,
    Io(io::Error),
}

thread_local! {
    pub static DISPATCHER: RefCell<BTreeMap<String, extern "C" fn()>> = Default::default();
}

#[no_mangle]
pub unsafe fn register_func(name: &str, f: extern "C" fn() -> ()) {
    println!("registering function {}", name);
    DISPATCHER.with(|foo| foo.borrow_mut().insert(name.to_string(), f));
}

pub fn register_entrypoint<'a, F: fmt::Debug + Fn()>(entrypoint: EntryPoint<'a, F>) {
    dbg!(entrypoint);
    // dbg!(&entrypoint);
    // unsafe {
    //     register_func(entrypoint.name, entrypoint.fptr);
    // }
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
        $crate::host::revert(None)
    };
    ($arg:expr) => {{
        let value = $arg;
        let data = borsh::to_vec(&value).expect("Revert value should serialize");
        casper_sdk::host::revert(Some(data.as_slice()));
        value
    }};
}
