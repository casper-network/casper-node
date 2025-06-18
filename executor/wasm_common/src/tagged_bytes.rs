use crate::type_uid::{TypeUid, Uid};
use borsh::{self, BorshDeserialize, BorshSerialize};
use bytes::Bytes;

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Hash, Clone)]
pub struct TaggedBytes {
    tag: Uid,
    bytes: Bytes,
}

impl Default for TaggedBytes {
    fn default() -> Self {
        TaggedBytes {
            tag: Uid::UNTYPED,
            bytes: Bytes::new(),
        }
    }
}

impl TaggedBytes {
    /// Creates a new `TaggedBytes` with the given tag and bytes.
    pub const fn from_raw_parts(tag: Uid, bytes: Bytes) -> Self {
        TaggedBytes { tag, bytes }
    }

    /// Returns the tag of the `TaggedBytes`.
    pub fn tag(&self) -> Uid {
        self.tag
    }

    /// Returns the bytes of the `TaggedBytes`.
    pub fn bytes(&self) -> &Bytes {
        &self.bytes
    }

    /// Attempts to deserialize the bytes into a type `T` that implements `BorshDeserialize`.
    pub fn to_value<T: BorshDeserialize + TypeUid>(&self) -> borsh::io::Result<T> {
        // Ensure the tag matches the UID of the type T
        if self.tag != Uid::UNTYPED && self.tag != T::UID {
            return Err(borsh::io::Error::new(
                borsh::io::ErrorKind::InvalidData,
                "Tag does not match type UID",
            ));
        }
        let deserialized_value: T = T::deserialize(&mut self.bytes.as_ref())?;
        Ok(deserialized_value)
    }

    /// Serializes a value of type `T` into `TaggedBytes`, using the type's UID as the tag.
    pub fn from_value<T: BorshSerialize + TypeUid>(value: &T) -> borsh::io::Result<Self> {
        let bytes = borsh::to_vec(value)?;
        Ok(TaggedBytes {
            tag: T::UID,
            bytes: Bytes::from(bytes),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::type_uid::{TypeUid, Uid};

    #[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug)]
    struct TestStruct {
        value: u32,
        name: String,
    }

    impl TypeUid for TestStruct {
        const UID: Uid = Uid::from_u64(42);
    }

    #[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug)]
    struct AnotherTestStruct {
        data: Vec<u8>,
    }

    impl TypeUid for AnotherTestStruct {
        const UID: Uid = Uid::from_u64(100);
    }

    #[test]
    fn test_from_raw_parts() {
        let tag = Uid::from_u64(123);
        let bytes = Bytes::from(vec![1, 2, 3, 4]);
        let tagged_bytes = TaggedBytes::from_raw_parts(tag, bytes.clone());

        assert_eq!(tagged_bytes.tag(), tag);
        assert_eq!(tagged_bytes.bytes(), &bytes);
    }

    #[test]
    fn test_from_value_and_to_value() {
        let test_struct = TestStruct {
            value: 42,
            name: "test".to_string(),
        };

        let tagged_bytes = TaggedBytes::from_value(&test_struct).unwrap();
        assert_eq!(tagged_bytes.tag(), TestStruct::UID);

        let deserialized: TestStruct = tagged_bytes.to_value().unwrap();
        assert_eq!(deserialized, test_struct);
    }

    #[test]
    fn test_from_value_different_types() {
        let test_struct = TestStruct {
            value: 100,
            name: "hello".to_string(),
        };
        let another_struct = AnotherTestStruct {
            data: vec![5, 6, 7, 8, 9],
        };

        let tagged_bytes1 = TaggedBytes::from_value(&test_struct).unwrap();
        let tagged_bytes2 = TaggedBytes::from_value(&another_struct).unwrap();

        assert_eq!(tagged_bytes1.tag(), TestStruct::UID);
        assert_eq!(tagged_bytes2.tag(), AnotherTestStruct::UID);
        assert_ne!(tagged_bytes1.tag(), tagged_bytes2.tag());
    }

    #[test]
    fn test_empty_struct() {
        #[derive(BorshSerialize, BorshDeserialize, PartialEq, Debug)]
        struct EmptyStruct;

        impl TypeUid for EmptyStruct {
            const UID: Uid = Uid::from_u64(0);
        }

        let empty = EmptyStruct;
        let tagged_bytes = TaggedBytes::from_value(&empty).unwrap();
        let deserialized: EmptyStruct = tagged_bytes.to_value().unwrap();

        assert_eq!(deserialized, empty);
        assert_eq!(tagged_bytes.tag(), EmptyStruct::UID);
    }

    #[test]
    fn test_deserialize_wrong_type_fails() {
        let test_struct = TestStruct {
            value: 42,
            name: "test".to_string(),
        };
        let tagged_bytes = TaggedBytes::from_value(&test_struct).unwrap();

        // Attempting to deserialize as AnotherTestStruct should fail
        let result: borsh::io::Result<AnotherTestStruct> = tagged_bytes.to_value();
        assert!(result.is_err());
    }

    #[test]
    fn test_bytes_accessor() {
        let data = vec![10, 20, 30, 40];
        let bytes = Bytes::from(data.clone());
        let tag = Uid::from_u64(999);
        let tagged_bytes = TaggedBytes::from_raw_parts(tag, bytes);

        assert_eq!(tagged_bytes.bytes().as_ref(), &data);
    }
}
