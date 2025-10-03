use bytes::Bytes;

use crate::{
    compat::types::cl_type::{CLType, CLTyped},
    serializers::borsh::{io, BorshDeserialize, BorshSerialize},
};

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Debug, BorshSerialize, BorshDeserialize)]
pub struct CLValue {
    bytes: Bytes,
    cl_type: CLType,
}

impl CLValue {
    pub const UNIT: CLValue = CLValue::from_components(CLType::Unit, Bytes::new());

    pub const fn from_components(cl_type: CLType, bytes: Bytes) -> Self {
        CLValue { bytes, cl_type }
    }

    pub fn from_t<T: CLTyped + BorshSerialize>(value: T) -> io::Result<Self> {
        let cl_type = T::cl_type();
        let bytes = borsh::to_vec(&value)?.into();
        Ok(CLValue { bytes, cl_type })
    }

    pub fn to_t<T: CLTyped + BorshDeserialize>(&self) -> io::Result<T> {
        if self.cl_type != T::cl_type() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("Expected {:?} but found {:?}.", T::cl_type(), self.cl_type),
            ));
        }

        borsh::from_slice(&self.bytes)
    }

    pub fn into_t<T: CLTyped + BorshDeserialize>(self) -> io::Result<T> {
        if self.cl_type != T::cl_type() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("Expected {:?} but found {:?}.", T::cl_type(), self.cl_type),
            ));
        }

        borsh::from_slice(&self.bytes)
    }

    pub fn cl_type(&self) -> &CLType {
        &self.cl_type
    }

    pub fn inner_bytes(&self) -> &[u8] {
        self.bytes.as_ref()
    }
}
