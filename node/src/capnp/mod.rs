use capnp::Error as CapnpError;

mod types;

#[derive(Debug)]
pub enum SerializeError {
    Capnp(CapnpError),
    TooManyItems,
}

impl From<CapnpError> for SerializeError {
    fn from(err: CapnpError) -> Self {
        Self::Capnp(err)
    }
}

#[derive(Debug)]
pub enum DeserializeError {
    Capnp(CapnpError),
    NotInSchema(capnp::NotInSchema),
    Types(casper_types::Error),
}

impl From<CapnpError> for DeserializeError {
    fn from(err: CapnpError) -> Self {
        Self::Capnp(err)
    }
}

impl From<casper_types::Error> for DeserializeError {
    fn from(err: casper_types::Error) -> Self {
        Self::Types(err)
    }
}

impl From<capnp::NotInSchema> for DeserializeError {
    fn from(err: capnp::NotInSchema) -> Self {
        Self::NotInSchema(err)
    }
}

pub trait ToCapnpBytes {
    fn try_to_capnp_bytes(&self) -> Result<Vec<u8>, SerializeError>;
}

pub trait FromCapnpBytes
where
    Self: Sized,
{
    fn try_from_capnp_bytes(bytes: &[u8]) -> Result<Self, DeserializeError>;
}
