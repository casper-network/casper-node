use std::ops::{Index, Range, RangeFrom, RangeFull, RangeTo};

use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::{CLType, CLTyped};

use super::{Error, FromBytes, ToBytes, U32_SERIALIZED_LENGTH, U8_SERIALIZED_LENGTH};

#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct Bytes(Vec<u8>);

impl Bytes {
    pub fn inner_bytes(&self) -> &Vec<u8> {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(vec: Vec<u8>) -> Self {
        Self(vec)
    }
}

impl From<Bytes> for Vec<u8> {
    fn from(bytes: Bytes) -> Self {
        bytes.0
    }
}

impl CLTyped for Bytes {
    fn cl_type() -> CLType {
        <Vec<u8>>::cl_type()
    }
}

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl ToBytes for Bytes {
    #[inline(always)]
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        super::serialize_bytes(&self.0)
        // let mut result = super::allocate_buffer(self)?;
        // result.append(&mut (self.len() as u32).to_bytes()?);
        // result.extend(&self.0);
        // Ok(result)
    }

    #[inline(always)]
    fn into_bytes(self) -> Result<Vec<u8>, Error> {
        super::serialize_bytes(&self.0)
        // let mut result = super::allocate_buffer(&self)?;
        // result.append(&mut (self.len() as u32).to_bytes()?);
        // result.append(&mut self.0);
        // Ok(result)
    }

    #[inline(always)]
    fn serialized_length(&self) -> usize {
        U32_SERIALIZED_LENGTH + self.len()
    }
}

impl FromBytes for Bytes {
    #[inline(always)]
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), super::Error> {
        let (vec, bytes) = super::deserialize_bytes(bytes)?;
        Ok((Bytes(vec), bytes))
    }

    // fn from_vec(bytes: Vec<u8>) -> Result<(Self, Vec<u8>), super::Error> {
    //     Self::from_bytes(bytes.as_slice()).map(|(x, remainder)| (x, Vec::from(remainder)))
    // }
}

impl Index<usize> for Bytes {
    type Output = u8;

    fn index(&self, index: usize) -> &u8 {
        let Bytes(ref dat) = self;
        &dat[index]
    }
}

impl Index<Range<usize>> for Bytes {
    type Output = [u8];

    fn index(&self, index: Range<usize>) -> &[u8] {
        let &Bytes(ref dat) = self;
        &dat[index]
    }
}

impl Index<RangeTo<usize>> for Bytes {
    type Output = [u8];

    fn index(&self, index: RangeTo<usize>) -> &[u8] {
        let &Bytes(ref dat) = self;
        &dat[index]
    }
}

impl Index<RangeFrom<usize>> for Bytes {
    type Output = [u8];

    fn index(&self, index: RangeFrom<usize>) -> &[u8] {
        let &Bytes(ref dat) = self;
        &dat[index]
    }
}

impl Index<RangeFull> for Bytes {
    type Output = [u8];

    fn index(&self, _: RangeFull) -> &[u8] {
        let &Bytes(ref dat) = self;
        &dat[..]
    }
}
// #[cfg(test)]
// mod tests {
//     use crate::bytesrepr::{self, Error, U32_SERIALIZED_LENGTH};

//     use super::Bytes;

//     #[test]
//     fn vec_u8_from_bytes() {
//         let data: Vec<u8> = vec![1, 2, 3, 4, 5];
//         let data_bytes = data.to_bytes().unwrap();
//         assert!(Bytes::from_bytes(&data_bytes[..U32_SERIALIZED_LENGTH / 2]).is_err());
//         assert!(Bytes::from_bytes(&data_bytes[..U32_SERIALIZED_LENGTH]).is_err());
//         assert!(Bytes::from_bytes(&data_bytes[..U32_SERIALIZED_LENGTH + 2]).is_err());
//     }

//     #[test]
//     fn should_serialize_deserialize_bytes() {
//         let data: Bytes = vec![1, 2, 3, 4, 5].into();
//         bytesrepr::test_serialization_roundtrip(&data);
//     }

//     #[test]
//     fn should_fail_to_serialize_deserialize_malicious_bytes() {
//         let data: Bytes = vec![1, 2, 3, 4, 5].into();
//         let mut serialized = data.to_bytes().expect("should serialize data");
//         serialized = serialized[..serialized.len() - 1].to_vec();
//         let res: Result<(_, &[u8]), Error> = Bytes::from_bytes(&serialized);
//         assert_eq!(res.unwrap_err(), Error::EarlyEndOfStream);
//     }

//     #[test]
//     fn should_serialize_deserialize_bytes_and_keep_rem() {
//         let data: Bytes = vec![1, 2, 3, 4, 5].into();
//         let expected_rem: Vec<u8> = vec![6, 7, 8, 9, 10];
//         let mut serialized = data.to_bytes().expect("should serialize data");
//         serialized.extend(&expected_rem);
//         let (deserialized, rem): (Bytes, &[u8]) =
//             super::deserialize_bytes(&serialized).expect("should deserialize data");
//         assert_eq!(data, deserialized);
//         assert_eq!(&rem, &expected_rem);
//     }
// }
