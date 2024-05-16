pub(crate) mod runtime;

use bytes::Bytes;
use casper_storage::{
    global_state::{self, state::StateReader},
    tracking_copy::TrackingCopyError,
};
use casper_types::{Key, StoredValue};
use vm_common::flags::EntryPointFlags;

pub(crate) type TrackingCopy<R> = casper_storage::TrackingCopy<R>;

pub trait GlobalStateReader:
    StateReader<Key, StoredValue, Error = global_state::error::Error>
{
}

impl<R: StateReader<Key, StoredValue, Error = global_state::error::Error>> GlobalStateReader for R {}

pub enum Tag {
    Bytes = 0,
}

pub enum KeySpace {
    Default = 0,
}

#[derive(Debug)]
pub enum Error {
    Foo,
    Bar,
}

pub struct Entry {
    pub tag: u64,
    pub data: Bytes,
}

#[derive(Default, Debug, Clone)]
pub(crate) struct EntryPoint {
    pub(crate) selector: u32,
    pub(crate) function_index: u32,
    pub(crate) flags: EntryPointFlags,
}

pub type Address = [u8; 32];

pub struct CreateResult {
    pub package_address: Address,
    pub contract_address: Address,
}

const STORAGE_READ_ERROR_MSG: &str = "failed to read from storage";
