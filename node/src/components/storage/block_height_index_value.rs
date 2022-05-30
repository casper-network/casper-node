use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    EraId, ProtocolVersion,
};

use crate::types::BlockHash;

/// The value stored under a given block height in the block height index DB.
#[derive(PartialEq, Eq, Debug)]
pub(super) struct BlockHeightIndexValue {
    pub(super) block_hash: BlockHash,
    pub(super) era_id: EraId,
    pub(super) protocol_version: ProtocolVersion,
}

impl ToBytes for BlockHeightIndexValue {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.block_hash.write_bytes(writer)?;
        self.era_id.write_bytes(writer)?;
        self.protocol_version.write_bytes(writer)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.block_hash.serialized_length()
            + self.era_id.serialized_length()
            + self.protocol_version.serialized_length()
    }
}

impl FromBytes for BlockHeightIndexValue {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (block_hash, remainder) = BlockHash::from_bytes(bytes)?;
        let (era_id, remainder) = EraId::from_bytes(remainder)?;
        let (protocol_version, remainder) = ProtocolVersion::from_bytes(remainder)?;
        let value = BlockHeightIndexValue {
            block_hash,
            era_id,
            protocol_version,
        };
        Ok((value, remainder))
    }
}

#[cfg(test)]
mod tests {
    use rand::Rng;

    use casper_types::testing::TestRng;

    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let mut rng = TestRng::new();
        let value = BlockHeightIndexValue {
            block_hash: BlockHash::random(&mut rng),
            era_id: EraId::new(rng.gen()),
            protocol_version: ProtocolVersion::from_parts(rng.gen(), rng.gen(), rng.gen()),
        };
        bytesrepr::test_serialization_roundtrip(&value);
    }
}
