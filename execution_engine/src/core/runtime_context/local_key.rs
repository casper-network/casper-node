use casper_types::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes},
    CLType, CLTyped, CLValue, CLValueError, Key,
};

use crate::shared::stored_value::StoredValue;

/// Wraps a [`CLValue`] and extends it with a seed [`URef`] and original key bytes.
#[derive(Clone)]
pub struct LocalKeyValue {
    /// Actual [`CLValue`] written to global state.
    cl_value: CLValue,
    /// Original key bytes.
    key_bytes: Bytes,
    /// [`URef`] seed bytes.
    seed_address: Bytes,
}

impl LocalKeyValue {
    pub fn new(cl_value: CLValue, key_bytes: Vec<u8>, seed_address: Vec<u8>) -> Self {
        Self {
            cl_value,
            key_bytes: key_bytes.into(),
            seed_address: seed_address.into(),
        }
    }

    /// Get a reference to the local key wrapper's cl value.
    pub fn into_cl_value(self) -> CLValue {
        self.cl_value
    }
}

impl CLTyped for LocalKeyValue {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl FromBytes for LocalKeyValue {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (cl_value, remainder) = FromBytes::from_bytes(bytes)?;
        let (key_bytes, remainder) = FromBytes::from_bytes(remainder)?;
        let (seed_address, remainder) = FromBytes::from_bytes(remainder)?;
        let local_key_value = LocalKeyValue {
            cl_value,
            key_bytes,
            seed_address,
        };
        Ok((local_key_value, remainder))
    }
}

impl ToBytes for LocalKeyValue {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.cl_value.to_bytes()?);
        buffer.extend(self.key_bytes.to_bytes()?);
        buffer.extend(self.seed_address.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.cl_value.serialized_length()
            + self.key_bytes.serialized_length()
            + self.seed_address.serialized_length()
    }
}

/// Inspects `key` argument whether it contains a local variant, and checks if `stored_value`
/// contains a [`CLValue`], then it will attempt a conversion from the held clvalue into
/// [`LocalKeyValue`] and returns the real [`CLValue`] held by it.
///
/// For any other combination of `key` and `stored_value` it returns its unmodified value.
pub fn monkey_patch(key: Key, stored_value: StoredValue) -> Result<StoredValue, CLValueError> {
    match (key, stored_value) {
        (Key::Local(_), StoredValue::CLValue(cl_value)) => {
            let wrapped_cl_value: LocalKeyValue = cl_value.into_t()?;
            let cl_value = wrapped_cl_value.into_cl_value();
            Ok(StoredValue::CLValue(cl_value))
        }
        (_, stored_value) => Ok(stored_value),
    }
}

/// Wraps a [`StoredValue`] into [`LocalKeyValue`] only if it contains a [`CLValue`] variant.
///
/// Used only for testing purposes.
#[cfg(test)]
pub fn monkey_patch_into(key: Key, stored_value: StoredValue) -> Result<StoredValue, CLValueError> {
    match (key, stored_value) {
        (Key::Local(_), StoredValue::CLValue(cl_value)) => {
            let wrapped_local_key = LocalKeyValue::new(cl_value, vec![0; 32], vec![255; 32]);
            let wrapped_cl_value = CLValue::from_t(wrapped_local_key)?;
            Ok(StoredValue::CLValue(wrapped_cl_value))
        }
        (_, stored_value) => Ok(stored_value),
    }
}
