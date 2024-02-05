pub trait CasperSchema: Contract {
    fn schema() -> Schema;
}

use std::{
    cell::RefCell,
    collections::BTreeMap,
    fmt::{self, LowerHex},
};

use bitflags::{Bits, Flags};
use serde::{self, Deserialize, Deserializer, Serialize, Serializer};
use vm_common::flags::EntryPointFlags;

use crate::{
    abi::{self, Declaration, Definition, Definitions},
    Contract,
};

pub fn serialize_bits<T, S>(data: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Flags,
    T::Bits: Serialize,
{
    data.bits().serialize(serializer)
}

pub fn deserialize_bits<'de, D, F>(deserializer: D) -> Result<F, D::Error>
where
    D: Deserializer<'de>,
    F: Flags,
    F::Bits: Deserialize<'de> + LowerHex,
{
    let raw: F::Bits = F::Bits::deserialize(deserializer)?;
    F::from_bits(raw).ok_or(serde::de::Error::custom(format!(
        "Unexpected flags value 0x{:#08x}",
        raw
    )))
}

pub mod schema_helper;

#[derive(Debug, Serialize, Deserialize)]
pub struct SchemaArgument {
    pub name: String,
    pub decl: abi::Declaration,
}

#[derive(Debug, Serialize, Deserialize)]

pub struct SchemaEntryPoint {
    pub name: String,
    pub selector: u32,
    pub arguments: Vec<SchemaArgument>,
    pub result: abi::Declaration,
    #[serde(
        serialize_with = "serialize_bits",
        deserialize_with = "deserialize_bits"
    )]
    pub flags: EntryPointFlags,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Schema {
    pub name: String,
    pub version: Option<String>,
    pub state: Declaration,
    pub definitions: Definitions,
    pub entry_points: Vec<SchemaEntryPoint>,
}

#[derive(Debug)]
pub struct EntryPoint<'a, F: Fn()> {
    pub name: &'a str,
    pub selector: u32,
    pub params: &'a [&'a str],
    pub func: F,
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