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

/// An abstraction over a key-value storage.
pub trait Storage: Clone + Send {
    /// Write an entry to the underlying kv-storage.
    fn write(&self, key_tag: u64, key: &[u8], value_tag: u64, value: &[u8]) -> Result<(), Error>;
    /// Read an entry from the underlying kv-storage.
    fn read(&self, key_tag: u64, key: &[u8]) -> Result<Option<Entry>, Error>;
}
