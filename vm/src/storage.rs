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

pub enum CallOutcome {
    Succeed,
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

/// An abstraction over a key-value storage.
pub trait Storage: Clone + Send {
    /// Write an entry to the underlying kv-storage.
    fn write(&self, key_tag: u64, key: &[u8], value_tag: u64, value: &[u8]) -> Result<(), Error>;
    /// Read an entry from the underlying kv-storage.
    fn read(&self, key_tag: u64, key: &[u8]) -> Result<Option<Entry>, Error>;

    /// Get balance of entity
    fn get_balance(&self, entity_address: &[u8]) -> Result<Option<u64>, Error>;
    /// Update balance
    fn update_balance(&self, entity_address: &[u8], new_balance: u64)
        -> Result<Option<u64>, Error>;

    /// Create a contract.
    fn create_contract(&self, code: Bytes, manifest: Manifest) -> Result<CreateResult, Error>;
    fn read_contract(&self, address: &[u8]) -> Result<Option<Contract>, Error>;
    fn read_code(&self, address: &[u8]) -> Result<Bytes, Error>;
}
