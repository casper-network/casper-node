use alloc::vec::Vec;

mod public_key;
mod test;

// TODO[RC]: Add From<capnp::Error> to avoid spamming `map_err()` and prevent losing information
// about original error.
#[derive(Debug)]
pub enum Error {
    UnableToSerialize,
    UnableToDeserialize,
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
