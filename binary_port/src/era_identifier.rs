#[cfg(test)]
use casper_types::testing::TestRng;
#[cfg(test)]
use rand::Rng;

use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    BlockIdentifier, EraId,
};

const ERA_TAG: u8 = 0;
const BLOCK_TAG: u8 = 1;

/// Identifier for an era.
#[derive(Clone, Debug, PartialEq)]
pub enum EraIdentifier {
    Era(EraId),
    Block(BlockIdentifier),
}

impl EraIdentifier {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..2) {
            ERA_TAG => EraIdentifier::Era(EraId::random(rng)),
            BLOCK_TAG => EraIdentifier::Block(BlockIdentifier::random(rng)),
            _ => unreachable!(),
        }
    }
}

impl ToBytes for EraIdentifier {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            EraIdentifier::Era(era_id) => {
                ERA_TAG.write_bytes(writer)?;
                era_id.write_bytes(writer)
            }
            EraIdentifier::Block(block_id) => {
                BLOCK_TAG.write_bytes(writer)?;
                block_id.write_bytes(writer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                EraIdentifier::Era(era_id) => era_id.serialized_length(),
                EraIdentifier::Block(block_id) => block_id.serialized_length(),
            }
    }
}

impl FromBytes for EraIdentifier {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            ERA_TAG => {
                let (era_id, remainder) = EraId::from_bytes(remainder)?;
                Ok((EraIdentifier::Era(era_id), remainder))
            }
            BLOCK_TAG => {
                let (block_id, remainder) = BlockIdentifier::from_bytes(remainder)?;
                Ok((EraIdentifier::Block(block_id), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use casper_types::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = EraIdentifier::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
