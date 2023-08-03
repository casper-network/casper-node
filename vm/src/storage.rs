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

/// An abstraction over a key-value storage.
pub trait Storage: Clone + Send {
    /// Write an entry to the underlying kv-storage.
    fn write(&self, key: &[u8], value: &[u8]) -> Result<(), Error>;
    /// Read an entry from the underlying kv-storage.
    fn read(&self, key: &[u8]) -> Result<Option<Bytes>, Error>;
}
