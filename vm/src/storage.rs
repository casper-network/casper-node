use bytes::Bytes;
use vm_common::flags::EntryPointFlags;

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

#[derive(Default, Debug, Clone)]
pub struct Manifest {
    pub(crate) entrypoints: Vec<EntryPoint>,
}

#[derive(Default, Debug, Clone)]
pub struct Contract {
    pub code_hash: Address,
    pub manifest: Manifest,
}

#[derive(Default, Debug, Clone)]
pub struct Package {
    pub versions: Vec<Address>,
}

pub type Address = [u8; 32];

pub struct CreateResult {
    pub package_address: Address,
    pub contract_address: Address,
}
