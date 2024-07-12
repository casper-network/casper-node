pub trait CasperSchema {
    fn schema() -> Schema;
}

use std::fmt::LowerHex;

use bitflags::Flags;
use casper_executor_wasm_common::flags::EntryPointFlags;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::abi::{self, Declaration, Definitions};

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

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct SchemaArgument {
    pub name: String,
    pub decl: abi::Declaration,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone)]

pub struct SchemaEntryPoint {
    pub name: String,
    pub selector: Option<u32>,
    pub arguments: Vec<SchemaArgument>,
    pub result: abi::Declaration,
    #[serde(
        serialize_with = "serialize_bits",
        deserialize_with = "deserialize_bits"
    )]
    pub flags: EntryPointFlags,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone)]
#[serde(tag = "type")]
pub enum SchemaType {
    /// Contract schemas contain a state structure that we want to mark in the schema.
    Contract { state: Declaration },
    /// Schemas of interface type does not contain state.
    Interface,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct Schema {
    pub name: String,
    pub version: Option<String>,
    #[serde(rename = "type")]
    pub type_: SchemaType,
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

#[cfg(not(target_family = "wasm"))]
use core::cell::RefCell;
#[cfg(not(target_family = "wasm"))]
use std::collections::BTreeMap;

#[cfg(not(target_family = "wasm"))]
thread_local! {
    pub static DISPATCHER: RefCell<BTreeMap<String, extern "C" fn()>> = Default::default();
}

// #[cfg(not(target_family = "wasm"))]
// #[no_mangle]
// pub unsafe fn register_func(name: &str, f: extern "C" fn() -> ()) {
//     println!("registering function {}", name);
//     DISPATCHER.with(|foo| foo.borrow_mut().insert(name.to_string(), f));
// }
