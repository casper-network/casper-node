use casper_types::bytesrepr::{self, FromBytes, ToBytes};

#[derive(PartialEq, Eq, Debug, Clone)]
pub(crate) struct PanickingFromBytes<T>(T);

impl<T> PanickingFromBytes<T> {
    pub(crate) fn new(inner: T) -> PanickingFromBytes<T> {
        PanickingFromBytes(inner)
    }
}

impl<T> FromBytes for PanickingFromBytes<T>
where
    T: FromBytes,
{
    fn from_bytes(_: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        unreachable!("This type is expected to never deserialize.");
    }
}

impl<T> ToBytes for PanickingFromBytes<T>
where
    T: ToBytes,
{
    fn into_bytes(self) -> Result<Vec<u8>, bytesrepr::Error>
    where
        Self: Sized,
    {
        self.0.into_bytes()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.0.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}
