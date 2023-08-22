use bytes::Bytes;

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
pub struct Param {
    pub(crate) name: Bytes,
    pub(crate) ty: u32,
}

#[derive(Default, Debug, Clone)]
pub struct EntryPoint {
    pub(crate) name: Bytes,
    pub(crate) params: Vec<Param>,
    pub(crate) function_index: u32,
}

#[derive(Default, Debug, Clone)]
pub struct Manifest {
    pub(crate) entrypoints: Vec<EntryPoint>,
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

    /// Calls address by transferring `value` amount of tokens, optionally calls an entry point.
    /// NOTE: Split to other trait
    fn call(&self, address: &[u8], value: u64, entry_point: u32) -> Result<CallOutcome, Error>;

    /// Create a contract.
    fn create_contract(&self, code: Bytes, manifest: Manifest) -> Result<CreateResult, Error>;
}
