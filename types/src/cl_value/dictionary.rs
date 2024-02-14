use alloc::vec::Vec;

use crate::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes},
    CLType, CLTyped, CLValue, CLValueError, Key, StoredValue,
};

/// Wraps a [`CLValue`] for storage in a dictionary.
///
/// Note that we include the dictionary [`super::super::URef`] and key used to create the
/// `Key::Dictionary` under which this value is stored.  This is to allow migration to a different
/// key representation in the future.
#[derive(Clone)]
pub struct DictionaryValue {
    /// Actual [`CLValue`] written to global state.
    cl_value: CLValue,
    /// [`URef`] seed bytes.
    seed_uref_addr: Bytes,
    /// Original key bytes.
    dictionary_item_key_bytes: Bytes,
}

impl DictionaryValue {
    /// Constructor.
    pub fn new(
        cl_value: CLValue,
        seed_uref_addr: Vec<u8>,
        dictionary_item_key_bytes: Vec<u8>,
    ) -> Self {
        Self {
            cl_value,
            seed_uref_addr: seed_uref_addr.into(),
            dictionary_item_key_bytes: dictionary_item_key_bytes.into(),
        }
    }

    /// Get a reference to the [`DictionaryValue`]'s wrapper's cl value.
    pub fn into_cl_value(self) -> CLValue {
        self.cl_value
    }
}

impl CLTyped for DictionaryValue {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl FromBytes for DictionaryValue {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (cl_value, remainder) = FromBytes::from_bytes(bytes)?;
        let (uref_addr, remainder) = FromBytes::from_bytes(remainder)?;
        let (key_bytes, remainder) = FromBytes::from_bytes(remainder)?;
        let dictionary_value = DictionaryValue {
            cl_value,
            seed_uref_addr: uref_addr,
            dictionary_item_key_bytes: key_bytes,
        };
        Ok((dictionary_value, remainder))
    }
}

impl ToBytes for DictionaryValue {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        buffer.extend(self.cl_value.to_bytes()?);
        buffer.extend(self.seed_uref_addr.to_bytes()?);
        buffer.extend(self.dictionary_item_key_bytes.to_bytes()?);
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.cl_value.serialized_length()
            + self.seed_uref_addr.serialized_length()
            + self.dictionary_item_key_bytes.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.cl_value.write_bytes(writer)?;
        self.seed_uref_addr.write_bytes(writer)?;
        self.dictionary_item_key_bytes.write_bytes(writer)?;
        Ok(())
    }
}

/// Inspects `key` argument whether it contains a dictionary variant, and checks if `stored_value`
/// contains a [`CLValue`], then it will attempt a conversion from the held clvalue into
/// [`DictionaryValue`] and returns the real [`CLValue`] held by it.
///
/// For any other combination of `key` and `stored_value` it returns its unmodified value.
pub fn handle_stored_dictionary_value(
    key: Key,
    stored_value: StoredValue,
) -> Result<StoredValue, CLValueError> {
    match (key, stored_value) {
        (Key::Dictionary(_), StoredValue::CLValue(cl_value)) => {
            let wrapped_cl_value: DictionaryValue = cl_value.into_t()?;
            let cl_value = wrapped_cl_value.into_cl_value();
            Ok(StoredValue::CLValue(cl_value))
        }
        (_, stored_value) => Ok(stored_value),
    }
}
