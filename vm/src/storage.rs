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

pub trait Storage {
    fn write(&mut self, key: &[u8], value: &[u8]) -> Result<(), Error>;
    fn read(&mut self, key: &[u8]) -> Result<Bytes, Error>;
}
