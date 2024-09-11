use crate::bytesrepr::{
    self, Error, FromBytes, ToBytes, U16_SERIALIZED_LENGTH, U32_SERIALIZED_LENGTH,
};
use alloc::vec::Vec;

#[derive(Eq, PartialEq, Debug)]
pub(crate) struct Field {
    pub index: u16,
    pub offset: u32,
}

impl Field {
    pub(crate) fn new(index: u16, offset: u32) -> Self {
        Field { index, offset }
    }
}

impl ToBytes for Field {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), Error> {
        self.index.write_bytes(writer)?;
        self.offset.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        U16_SERIALIZED_LENGTH + U32_SERIALIZED_LENGTH
    }
}

impl FromBytes for Field {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (index, remainder) = u16::from_bytes(bytes)?;
        let (offset, remainder) = u32::from_bytes(remainder)?;
        Ok((Field::new(index, offset), remainder))
    }
}

impl Field {
    pub fn serialized_vec_size(number_of_fields: usize) -> usize {
        let mut size = U32_SERIALIZED_LENGTH; // Overhead of the vec itself
        size += number_of_fields * Field::serialized_length();
        size
    }

    pub fn serialized_length() -> usize {
        U16_SERIALIZED_LENGTH + U32_SERIALIZED_LENGTH
    }
}
