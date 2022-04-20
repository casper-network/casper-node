use crate::types::{BlockHash, BlockHeader, Timestamp};
use casper_types::ProtocolVersion;

use super::{FromCapnpReader, ToCapnpBuilder};
use crate::capnp::{DeserializeError, FromCapnpBytes, SerializeError, ToCapnpBytes};

#[allow(dead_code)]
pub(super) mod block_header_capnp {
    include!(concat!(
        env!("OUT_DIR"),
        "/src/capnp/schemas/block_header_capnp.rs"
    ));
}

impl ToCapnpBuilder<BlockHash> for block_header_capnp::block_hash::Builder<'_> {
    fn try_to_builder(&mut self, block_hash: &BlockHash) -> Result<(), SerializeError> {
        let mut digest_builder = self.reborrow().init_hash();
        digest_builder.try_to_builder(block_hash.inner())?;
        Ok(())
    }
}

impl FromCapnpReader<BlockHash> for block_header_capnp::block_hash::Reader<'_> {
    fn try_from_reader(&self) -> Result<BlockHash, DeserializeError> {
        let digest_reader = self.get_hash().map_err(DeserializeError::from)?;
        let digest = digest_reader.try_from_reader()?;
        Ok(digest.into())
    }
}

impl ToCapnpBytes for BlockHash {
    fn try_to_capnp_bytes(&self) -> Result<Vec<u8>, SerializeError> {
        let mut builder = capnp::message::Builder::new_default();
        let mut msg = builder.init_root::<block_header_capnp::block_hash::Builder>();
        msg.try_to_builder(self)?;
        let mut serialized = Vec::new();
        capnp::serialize::write_message(&mut serialized, &builder).map_err(SerializeError::from)?;
        Ok(serialized)
    }
}

impl FromCapnpBytes for BlockHash {
    fn try_from_capnp_bytes(bytes: &[u8]) -> Result<Self, DeserializeError> {
        let deserialized =
            capnp::serialize::read_message(bytes, capnp::message::ReaderOptions::new())
                .map_err(DeserializeError::from)?;

        let reader = deserialized
            .get_root::<block_header_capnp::block_hash::Reader>()
            .map_err(DeserializeError::from)?;
        reader.try_from_reader()
    }
}

impl ToCapnpBuilder<BlockHeader> for block_header_capnp::block_header::Builder<'_> {
    fn try_to_builder(&mut self, block_header: &BlockHeader) -> Result<(), SerializeError> {
        {
            let mut block_hash_builder = self.reborrow().init_parent_hash();
            block_hash_builder.try_to_builder(block_header.parent_hash())?;
        }
        {
            let mut digest_builder = self.reborrow().init_state_root_hash();
            digest_builder.try_to_builder(block_header.state_root_hash())?;
        }
        {
            let mut body_hash_builder = self.reborrow().init_body_hash();
            body_hash_builder.try_to_builder(block_header.body_hash())?;
        }
        {
            self.reborrow().set_random_bit(block_header.random_bit());
        }
        {
            let mut accumulated_seed_builder = self.reborrow().init_accumulated_seed();
            accumulated_seed_builder.try_to_builder(&block_header.accumulated_seed())?;
        }
        {
            let mut era_end_builder = self.reborrow().init_era_end();
            match block_header.era_end() {
                Some(era_end) => {
                    let mut some_era_end_builder = era_end_builder.init_some_era_end();
                    some_era_end_builder.try_to_builder(era_end)?;
                }
                None => {
                    era_end_builder.set_none(());
                }
            }
        }
        {
            let mut timestamp_builder = self.reborrow().init_timestamp();
            timestamp_builder.set_inner(block_header.timestamp().millis());
        }
        {
            let mut era_id_builder = self.reborrow().init_era_id();
            era_id_builder.try_to_builder(&block_header.era_id())?;
        }
        {
            self.reborrow().set_height(block_header.height());
        }
        {
            let semver = block_header.protocol_version().value();
            let mut protocol_version_builder = self.reborrow().init_protocol_version();
            {
                let mut semver_builder = protocol_version_builder.reborrow().init_semver();
                semver_builder.set_major(semver.major);
                semver_builder.set_minor(semver.minor);
                semver_builder.set_patch(semver.patch);
            }
        }
        Ok(())
    }
}

impl FromCapnpReader<BlockHeader> for block_header_capnp::block_header::Reader<'_> {
    fn try_from_reader(&self) -> Result<BlockHeader, DeserializeError> {
        let parent_hash = {
            let parent_hash_reader = self.get_parent_hash().map_err(DeserializeError::from)?;
            parent_hash_reader.try_from_reader()?
        };
        let state_root_hash = {
            let state_root_hash_reader =
                self.get_state_root_hash().map_err(DeserializeError::from)?;
            state_root_hash_reader.try_from_reader()?
        };
        let body_hash = {
            let body_hash_builder = self.get_body_hash().map_err(DeserializeError::from)?;
            body_hash_builder.try_from_reader()?
        };
        let random_bit = self.get_random_bit();
        let accumulated_seed = {
            let accumulated_seed_reader = self
                .get_accumulated_seed()
                .map_err(DeserializeError::from)?;
            accumulated_seed_reader.try_from_reader()?
        };
        let era_end = {
            match self
                .get_era_end()
                .map_err(DeserializeError::from)?
                .which()
                .map_err(DeserializeError::from)?
            {
                block_header_capnp::maybe_era_end::Which::SomeEraEnd(reader) => match reader {
                    Ok(reader) => Some(reader.try_from_reader()?),
                    Err(e) => return Err(e.into()),
                },
                block_header_capnp::maybe_era_end::Which::None(_) => None,
            }
        };
        let timestamp: Timestamp = {
            self.get_timestamp()
                .map_err(DeserializeError::from)?
                .get_inner()
                .into()
        };
        let era_id = {
            let era_id_reader = self.get_era_id().map_err(DeserializeError::from)?;
            era_id_reader.try_from_reader()?
        };
        let height = self.get_height();
        let protocol_version = {
            let semver_reader = self
                .get_protocol_version()
                .map_err(DeserializeError::from)?
                .get_semver()
                .map_err(DeserializeError::from)?;
            ProtocolVersion::from_parts(
                semver_reader.get_major(),
                semver_reader.get_minor(),
                semver_reader.get_patch(),
            )
        };
        Ok(BlockHeader::new(
            parent_hash,
            state_root_hash,
            body_hash,
            random_bit,
            accumulated_seed,
            era_end,
            timestamp,
            era_id,
            height,
            protocol_version,
        ))
    }
}

impl ToCapnpBytes for BlockHeader {
    fn try_to_capnp_bytes(&self) -> Result<Vec<u8>, SerializeError> {
        let mut builder = capnp::message::Builder::new_default();
        let mut msg = builder.init_root::<block_header_capnp::block_header::Builder>();
        msg.try_to_builder(self)?;
        let mut serialized = Vec::new();
        capnp::serialize::write_message(&mut serialized, &builder).map_err(SerializeError::from)?;
        Ok(serialized)
    }
}

impl FromCapnpBytes for BlockHeader {
    fn try_from_capnp_bytes(bytes: &[u8]) -> Result<Self, DeserializeError> {
        let deserialized =
            capnp::serialize::read_message(bytes, capnp::message::ReaderOptions::new())
                .map_err(DeserializeError::from)?;

        let reader = deserialized
            .get_root::<block_header_capnp::block_header::Reader>()
            .map_err(DeserializeError::from)?;
        reader.try_from_reader()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{
        super::{
            digest::tests::random_digest,
            era::tests::{random_era_end, random_era_id},
            random_byte, random_u64,
        },
        *,
    };

    use crate::types::EraEnd;

    pub(crate) fn random_block_hash() -> BlockHash {
        BlockHash::new(random_digest())
    }

    pub(crate) fn random_block_header(era_end: Option<EraEnd>) -> BlockHeader {
        let parent_hash = random_block_hash();
        let state_root_hash = random_digest();
        let body_hash = random_digest();
        let random_bit = random_byte() % 2 == 0;
        let accumulated_seed = random_digest();
        let timestamp = random_u64().into();
        let era_id = random_era_id();
        let height = random_u64();
        let protocol_version = ProtocolVersion::from_parts(
            random_u64() as u32,
            random_u64() as u32,
            random_u64() as u32,
        );

        BlockHeader::new(
            parent_hash,
            state_root_hash,
            body_hash,
            random_bit,
            accumulated_seed,
            era_end,
            timestamp,
            era_id,
            height,
            protocol_version,
        )
    }

    #[test]
    fn block_hash_capnp() {
        let block_hash = random_block_hash();
        let original = block_hash.clone();
        let serialized = original.try_to_capnp_bytes().expect("serialization");
        let deserialized = BlockHash::try_from_capnp_bytes(&serialized).expect("deserialization");

        assert_eq!(original, deserialized);
    }

    #[test]
    fn block_header_capnp() {
        let block_header = random_block_header(None);
        let original = block_header.clone();
        let serialized = original.try_to_capnp_bytes().expect("serialization");
        let deserialized = BlockHeader::try_from_capnp_bytes(&serialized).expect("deserialization");
        assert_eq!(original, deserialized);

        let block_header = random_block_header(Some(random_era_end()));
        let original = block_header.clone();
        let serialized = original.try_to_capnp_bytes().expect("serialization");
        let deserialized = BlockHeader::try_from_capnp_bytes(&serialized).expect("deserialization");
        assert_eq!(original, deserialized);
    }
}
