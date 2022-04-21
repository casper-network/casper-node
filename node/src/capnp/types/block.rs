use crate::types::Block;

use super::{FromCapnpReader, ToCapnpBuilder};
use crate::capnp::{DeserializeError, FromCapnpBytes, SerializeError, ToCapnpBytes};

#[allow(dead_code)]
pub(super) mod block_capnp {
    include!(concat!(
        env!("OUT_DIR"),
        "/src/capnp/schemas/block_capnp.rs"
    ));
}

impl ToCapnpBuilder<Block> for block_capnp::block::Builder<'_> {
    fn try_to_builder(&mut self, block: &Block) -> Result<(), SerializeError> {
        {
            let mut hash_builder = self.reborrow().init_hash();
            hash_builder.try_to_builder(block.hash())?;
        }
        {
            let mut header_builder = self.reborrow().init_header();
            header_builder.try_to_builder(block.header())?;
        }
        {
            let mut body_builder = self.reborrow().init_body();
            body_builder.try_to_builder(block.body())?;
        }
        Ok(())
    }
}

impl FromCapnpReader<Block> for block_capnp::block::Reader<'_> {
    fn try_from_reader(&self) -> Result<Block, DeserializeError> {
        let hash_reader = self.get_hash()?;
        let hash = hash_reader.try_from_reader()?;

        let header_reader = self.get_header()?;
        let header = header_reader.try_from_reader()?;

        let body_reader = self.get_body()?;
        let body = body_reader.try_from_reader()?;
        Ok(Block::new_unchecked(hash, header, body))
    }
}

impl ToCapnpBytes for Block {
    fn try_to_capnp_bytes(&self) -> Result<Vec<u8>, SerializeError> {
        let mut builder = capnp::message::Builder::new_default();
        let mut msg = builder.init_root::<block_capnp::block::Builder>();
        msg.try_to_builder(self)?;
        let mut serialized = Vec::new();
        capnp::serialize::write_message(&mut serialized, &builder)?;
        Ok(serialized)
    }
}

impl FromCapnpBytes for Block {
    fn try_from_capnp_bytes(bytes: &[u8]) -> Result<Self, DeserializeError> {
        let deserialized =
            capnp::serialize::read_message(bytes, capnp::message::ReaderOptions::new())?;

        let reader = deserialized.get_root::<block_capnp::block::Reader>()?;
        reader.try_from_reader()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{
        super::{
            block_body::tests::random_block_body,
            block_header::tests::{random_block_hash, random_block_header},
            era::tests::random_era_end,
        },
        *,
    };

    use crate::types::EraEnd;

    fn random_block(era_end: Option<EraEnd>) -> Block {
        Block::new_unchecked(
            random_block_hash(),
            random_block_header(era_end),
            random_block_body(),
        )
    }

    #[test]
    fn block_capnp() {
        let block = random_block(None);
        let original = block.clone();
        let serialized = original.try_to_capnp_bytes().expect("serialization");
        let deserialized = Block::try_from_capnp_bytes(&serialized).expect("deserialization");
        assert_eq!(original, deserialized);

        let block = random_block(Some(random_era_end()));
        let original = block.clone();
        let serialized = original.try_to_capnp_bytes().expect("serialization");
        let deserialized = Block::try_from_capnp_bytes(&serialized).expect("deserialization");
        assert_eq!(original, deserialized);
    }
}
