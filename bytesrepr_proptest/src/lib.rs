#[cfg(not(feature = "std"))]
extern crate alloc;

#[cfg(not(feature = "std"))]
use alloc::fmt::Debug;
#[cfg(feature = "std")]
use std::fmt::Debug;

use bytesrepr::{deserialize, FromBytes, ToBytes};

/// A property test (leveraging [proptest]) that roundtrips data using [bytesrepr].
pub fn test_serialization_roundtrip<T>(input: &T)
where
    T: Debug + ToBytes + FromBytes + PartialEq,
{
    let serialized = ToBytes::to_bytes(input).expect("Unable to serialize data");
    assert_eq!(
        serialized.len(),
        input.serialized_length(),
        "Length of serialized data according to bytesrepr impl: {},\n\
         serialized_length() yielded: {},\n\
         serialized data: {:?},\n\
         input: {:?}",
        serialized.len(),
        input.serialized_length(),
        serialized,
        input
    );
    let deserialized = deserialize::<T>(serialized).expect("Unable to deserialize data");
    assert_eq!(
        input, &deserialized,
        "Expected deserialized data: {:?}\n\
         Actual deserialized data: {:?}",
        input, deserialized
    );
}
