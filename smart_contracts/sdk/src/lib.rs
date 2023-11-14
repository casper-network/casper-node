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

#[cfg(not(target_arch = "wasm32"))]
pub mod schema_helper {
    use once_cell::sync::Lazy;

    use crate::EntryPoint;

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

    pub static mut LAZY: [Option<Export>; 1024] = [None; 1024];

    pub fn register_export(export: Export) {
        for l in unsafe { LAZY.iter_mut() } {
            if l.is_none() {
                *l = Some(export);
                return;
            }
        }
        assert!(false, "no more space");
    }

    pub fn list_exports() -> Vec<&'static Export> {
        let mut v = Vec::new();
        for export_name in unsafe { LAZY.iter() } {
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

#[derive(Debug)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Serialize, Deserialize))]
pub enum CLType {
    Bool,
    String,
    Unit,
    Any,
}

pub trait CLTyped {
    fn cl_type() -> CLType;
}

impl CLTyped for String {
    fn cl_type() -> CLType {
        CLType::String
    }
}
impl CLTyped for bool {
    fn cl_type() -> CLType {
        CLType::Bool
    }
}

impl CLTyped for () {
    fn cl_type() -> CLType {
        CLType::Unit
    }
}

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
    pub fn set(&mut self, value: T) -> io::Result<()> {
        // let mut value = Vec::new();
        // value.serialize(&mut value)?;
        let v = borsh::to_vec(&value)?;
        host::write(self.key_space, self.name.as_bytes(), 0, &v)
            .map_err(|error| io::Error::new(io::ErrorKind::Other, "todo"))?;
        Ok(())
    }
}
impl<T: BorshDeserialize> Value<T> {
    pub fn get(&self) -> io::Result<Option<T>> {
        let mut read = None; //Vec::new();
        host::read(self.key_space, self.name.as_bytes(), |size| {
            *(&mut read) = Some(Vec::new());
            reserve_vec_space(read.as_mut().unwrap(), size)
        })
        .map_err(|error| io::Error::new(io::ErrorKind::Other, "todo"))?;
        match read {
            Some(mut read) => {
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
