use alloc::vec::Vec;

mod test;

#[derive(Debug)]
pub enum Error {
    UnableToSerialize,
}

pub trait ToCapnpBytes {
    fn try_to_capnp_bytes(&self) -> Result<Vec<u8>, Error>;
}

pub trait FromCapnpBytes
where
    Self: Sized,
{
    fn try_from_capnp_bytes(bytes: &[u8]) -> Result<Self, Error>;
}
