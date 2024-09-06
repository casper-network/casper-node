mod field;
use alloc::vec::Vec;
use field::Field;

use crate::bytesrepr::{
    self, Bytes, Error, FromBytes, ToBytes, U32_SERIALIZED_LENGTH, U8_SERIALIZED_LENGTH,
};

pub struct CalltableSerializationEnvelopeBuilder {
    fields: Vec<Field>,
    expected_payload_sizes: Vec<usize>,
    bytes: Vec<u8>,
    current_field_index: usize,
    current_offset: usize,
}

impl CalltableSerializationEnvelopeBuilder {
    pub fn new(
        expected_payload_sizes: Vec<usize>,
    ) -> Result<CalltableSerializationEnvelopeBuilder, Error> {
        let number_of_fields = expected_payload_sizes.len();
        let fields_size = Field::serialized_vec_size(number_of_fields);
        let bytes_of_payload_size = expected_payload_sizes.iter().sum::<usize>();
        let payload_and_vec_overhead: usize = U32_SERIALIZED_LENGTH + bytes_of_payload_size; // u32 for the overhead of serializing a vec
        let mut payload_buffer =
            bytesrepr::allocate_buffer_for_size(fields_size + payload_and_vec_overhead)?;
        payload_buffer.extend(vec![0; fields_size]); // Making room for the call table
        payload_buffer.extend((bytes_of_payload_size as u32).to_bytes()?); // Writing down number of bytes that are in the payload
        Ok(CalltableSerializationEnvelopeBuilder {
            fields: Vec::with_capacity(number_of_fields),
            expected_payload_sizes,
            bytes: payload_buffer,
            current_field_index: 0,
            current_offset: 0,
        })
    }

    pub fn add_field<T: ToBytes + ?Sized>(
        mut self,
        field_index: u16,
        value: &T,
    ) -> Result<Self, Error> {
        let current_field_index = self.current_field_index;
        if current_field_index >= self.expected_payload_sizes.len() {
            //We wrote more fields than expected
            return Err(Error::NotRepresentable);
        }
        let fields = &mut self.fields;
        if current_field_index > 0 && fields[current_field_index - 1].index >= field_index {
            //Need to make sure we write fields in ascending order of tab index
            return Err(Error::NotRepresentable);
        }
        let size = self.expected_payload_sizes[current_field_index];
        let bytes_before = self.bytes.len();
        value.write_bytes(&mut self.bytes)?;
        fields.push(Field::new(field_index, self.current_offset as u32));
        self.current_field_index += 1;
        self.current_offset += size;
        let bytes_after = self.bytes.len();
        let wrote_bytes = bytes_after - bytes_before;
        if wrote_bytes == 0 {
            //We don't allow writing empty fields
            return Err(Error::NotRepresentable);
        }
        if wrote_bytes != size {
            //The written field occupied different amount of bytes than declared
            return Err(Error::NotRepresentable);
        }
        Ok(self)
    }

    pub fn binary_payload_bytes(mut self) -> Result<Vec<u8>, Error> {
        if self.current_field_index != self.expected_payload_sizes.len() {
            //We didn't write all the fields we expected
            return Err(Error::NotRepresentable);
        }
        let write_at_slice = &mut self.bytes[0..];
        let calltable_bytes = self.fields.to_bytes()?;
        for (pos, byte) in calltable_bytes.into_iter().enumerate() {
            write_at_slice[pos] = byte;
        }
        Ok(self.bytes)
    }
}

pub struct CalltableSerializationEnvelope {
    fields: Vec<Field>,
    bytes: Bytes,
}

impl CalltableSerializationEnvelope {
    pub fn estimate_size(field_sizes: Vec<usize>) -> usize {
        let number_of_fields = field_sizes.len();
        let payload_in_bytes: usize = field_sizes.iter().sum();
        let mut size = U32_SERIALIZED_LENGTH + U32_SERIALIZED_LENGTH; // Overhead of the fields vec and bytes vec
        size += number_of_fields * Field::serialized_length();
        size += payload_in_bytes * U8_SERIALIZED_LENGTH;
        size
    }

    pub fn start_consuming(&self) -> Result<Option<CalltableFieldsIterator>, Error> {
        if self.fields.is_empty() {
            return Ok(None);
        }
        let field = &self.fields[0];
        let expected_size = if self.fields.len() == 1 {
            self.bytes.len()
        } else {
            self.fields[1].offset as usize
        };
        Ok(Some(CalltableFieldsIterator {
            index_in_fields_vec: 0,
            expected_size,
            field,
            bytes: &self.bytes,
            parent: self,
        }))
    }

    pub fn from_bytes(
        max_expected_fields: u32,
        input_bytes: &[u8],
    ) -> Result<(CalltableSerializationEnvelope, &[u8]), Error> {
        if input_bytes.len() < U32_SERIALIZED_LENGTH {
            //The first "thing" in the bytes of the payload should be a `fields` vector. We want to
            // check the number of entries in that vector to avoid field pumping. If the
            // payload doesn't have u32 size of bytes in it, then it's malformed.
            return Err(Error::Formatting);
        }
        let (number_of_fields, _) = u32::from_bytes(input_bytes)?;
        if number_of_fields > max_expected_fields {
            return Err(Error::Formatting);
        }
        let (fields, remainder) = Vec::<Field>::from_bytes(input_bytes)?;
        let (bytes, remainder) = Bytes::from_bytes(remainder)?;
        Ok((CalltableSerializationEnvelope { fields, bytes }, remainder))
    }
}

pub struct CalltableFieldsIterator<'a> {
    index_in_fields_vec: usize,
    expected_size: usize,
    field: &'a Field,
    bytes: &'a [u8],
    parent: &'a CalltableSerializationEnvelope,
}

impl<'a> CalltableFieldsIterator<'a> {
    pub fn verify_index(&self, expected_index: u16) -> Result<(), Error> {
        let field = self.field;
        if field.index != expected_index {
            return Err(Error::Formatting);
        }
        Ok(())
    }

    pub fn deserialize_and_maybe_next<T: FromBytes>(
        &self,
    ) -> Result<(T, Option<CalltableFieldsIterator>), Error> {
        let (t, maybe_window) = self.step()?;
        Ok((t, maybe_window))
    }

    fn step<T: FromBytes>(&self) -> Result<(T, Option<CalltableFieldsIterator>), Error> {
        let (t, remainder) = T::from_bytes(self.bytes)?;
        let parent_fields = &self.parent.fields;
        let parent_fields_len = parent_fields.len();
        let is_last_field = self.index_in_fields_vec == parent_fields_len - 1;
        if remainder.len() + self.expected_size != self.bytes.len() {
            //The field occupied different amount of bytes than expected
            return Err(Error::Formatting);
        }
        if !is_last_field {
            let next_field_index = self.index_in_fields_vec + 1;
            let next_field = &parent_fields[next_field_index]; // We already checked that this index exists
            let is_next_field_last = next_field_index == parent_fields_len - 1;
            let expected_size = if is_next_field_last {
                remainder.len()
            } else {
                (parent_fields[next_field_index + 1].offset
                    - parent_fields[next_field_index].offset) as usize
            };
            let next_window = CalltableFieldsIterator {
                index_in_fields_vec: next_field_index,
                expected_size,
                field: next_field,
                bytes: remainder,
                parent: self.parent,
            };
            Ok((t, Some(next_window)))
        } else {
            if !remainder.is_empty() {
                //The payload of BinaryPayload should contain only the serialized, there should be
                // no trailing bytes after consuming all the fields.
                return Err(Error::Formatting);
            }
            Ok((t, None))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CalltableSerializationEnvelope, CalltableSerializationEnvelopeBuilder, Field};
    use crate::bytesrepr::*;

    #[test]
    fn binary_payload_should_serialize_fields() {
        let binary_payload = CalltableSerializationEnvelopeBuilder::new(vec![
            U8_SERIALIZED_LENGTH,
            U32_SERIALIZED_LENGTH,
            U16_SERIALIZED_LENGTH,
        ])
        .unwrap();
        let bytes = binary_payload
            .add_field(0, &(254_u8))
            .unwrap()
            .add_field(1, &(u32::MAX))
            .unwrap()
            .add_field(2, &(555_u16))
            .unwrap()
            .binary_payload_bytes()
            .unwrap();
        let (payload, remainder) = CalltableSerializationEnvelope::from_bytes(3, &bytes).unwrap();
        assert!(remainder.is_empty());
        assert_eq!(
            payload.fields,
            vec![Field::new(0, 0), Field::new(1, 1), Field::new(2, 5)]
        );
        let bytes = &payload.bytes;
        let (first_value, remainder) = u8::from_bytes(bytes).unwrap();
        let (second_value, remainder) = u32::from_bytes(remainder).unwrap();
        let (third_value, remainder) = u16::from_bytes(remainder).unwrap();
        assert!(remainder.is_empty());
        assert_eq!(first_value, 254);
        assert_eq!(second_value, u32::MAX);
        assert_eq!(third_value, 555);
    }

    #[test]
    fn binary_payload_should_fail_to_deserialzie_if_more_then_expected_fields() {
        let binary_payload = CalltableSerializationEnvelopeBuilder::new(vec![
            U8_SERIALIZED_LENGTH,
            U32_SERIALIZED_LENGTH,
            U16_SERIALIZED_LENGTH,
        ])
        .unwrap();
        let bytes = binary_payload
            .add_field(0, &(254_u8))
            .unwrap()
            .add_field(1, &(u32::MAX))
            .unwrap()
            .add_field(2, &(555_u16))
            .unwrap()
            .binary_payload_bytes()
            .unwrap();
        let res = CalltableSerializationEnvelope::from_bytes(2, &bytes);
        assert!(res.is_err())
    }

    #[test]
    fn binary_payload_should_fail_to_serialize_if_field_indexes_not_unique() {
        let binary_payload = CalltableSerializationEnvelopeBuilder::new(vec![
            U8_SERIALIZED_LENGTH,
            U32_SERIALIZED_LENGTH,
            U16_SERIALIZED_LENGTH,
        ])
        .unwrap();
        let res = binary_payload
            .add_field(0, &(254_u8))
            .unwrap()
            .add_field(0, &(u32::MAX));
        assert!(res.is_err())
    }

    #[test]
    fn binary_payload_should_fail_to_serialize_if_field_indexes_not_sequential() {
        let binary_payload = CalltableSerializationEnvelopeBuilder::new(vec![
            U8_SERIALIZED_LENGTH,
            U32_SERIALIZED_LENGTH,
            U16_SERIALIZED_LENGTH,
        ])
        .unwrap();
        let res = binary_payload
            .add_field(1, &(254_u8))
            .unwrap()
            .add_field(0, &(u32::MAX));
        assert!(res.is_err())
    }

    #[test]
    fn binary_payload_should_fail_to_serialize_if_proposed_fields_size_is_smaller_than_declaration()
    {
        let binary_payload = CalltableSerializationEnvelopeBuilder::new(vec![
            U8_SERIALIZED_LENGTH,
            U32_SERIALIZED_LENGTH,
            U16_SERIALIZED_LENGTH,
        ])
        .unwrap();
        let res = binary_payload
            .add_field(0, &(254_u8))
            .unwrap()
            .add_field(1, &(u16::MAX));
        assert!(res.is_err())
    }

    #[test]
    fn binary_payload_should_fail_to_serialize_if_proposed_fields_size_is_greater_than_declaration()
    {
        let binary_payload = CalltableSerializationEnvelopeBuilder::new(vec![
            U8_SERIALIZED_LENGTH,
            U16_SERIALIZED_LENGTH,
        ])
        .unwrap();
        let res = binary_payload
            .add_field(0, &(254_u8))
            .unwrap()
            .add_field(1, &(u32::MAX));
        assert!(res.is_err())
    }

    #[test]
    fn binary_payload_should_fail_to_serialize_if_proposed_a_field_with_zero_bytes() {
        struct FunkyType {}
        impl ToBytes for FunkyType {
            fn to_bytes(&self) -> Result<Vec<u8>, Error> {
                Ok(Vec::new())
            }

            fn serialized_length(&self) -> usize {
                0
            }
        }
        let funky_instance = FunkyType {};
        let binary_payload = CalltableSerializationEnvelopeBuilder::new(vec![
            U8_SERIALIZED_LENGTH,
            funky_instance.serialized_length(),
        ])
        .unwrap();
        let res = binary_payload
            .add_field(0, &(254_u8))
            .unwrap()
            .add_field(1, &(funky_instance));
        assert!(res.is_err())
    }
}
