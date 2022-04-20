use crate::types::BlockBody;

use std::convert::TryInto;

use super::{FromCapnpReader, ToCapnpBuilder};
use crate::capnp::{Error, FromCapnpBytes, ToCapnpBytes};

#[allow(dead_code)]
pub(super) mod block_body_capnp {
    include!(concat!(
        env!("OUT_DIR"),
        "/src/capnp/schemas/block_body_capnp.rs"
    ));
}

impl ToCapnpBuilder<BlockBody> for block_body_capnp::block_body::Builder<'_> {
    fn try_to_builder(&mut self, block_body: &BlockBody) -> Result<(), Error> {
        {
            let mut proposer_builder = self.reborrow().init_proposer();
            proposer_builder.try_to_builder(block_body.proposer())?;
        }
        {
            let deploy_hashes = block_body.deploy_hashes();
            let deploy_hashes_count: u32 = block_body
                .deploy_hashes()
                .len()
                .try_into()
                .map_err(|_| Error::TooManyItems)?;
            let mut list_builder = self.reborrow().init_deploy_hashes(deploy_hashes_count);
            for (index, deploy_hash) in deploy_hashes.iter().enumerate() {
                let mut msg = list_builder.reborrow().get(index as u32);
                msg.try_to_builder(deploy_hash)?;
            }
        }
        {
            let transfer_hashes = block_body.transfer_hashes();
            let transfer_hashes_count: u32 = block_body
                .transfer_hashes()
                .len()
                .try_into()
                .map_err(|_| Error::TooManyItems)?;
            let mut list_builder = self.reborrow().init_transfer_hashes(transfer_hashes_count);
            for (index, deploy_hash) in transfer_hashes.iter().enumerate() {
                let mut msg = list_builder.reborrow().get(index as u32);
                msg.try_to_builder(deploy_hash)?;
            }
        }
        Ok(())
    }
}

impl FromCapnpReader<BlockBody> for block_body_capnp::block_body::Reader<'_> {
    fn try_from_reader(&self) -> Result<BlockBody, Error> {
        let proposer = {
            let proposer_reader = self
                .get_proposer()
                .map_err(|_| Error::UnableToDeserialize)?;
            proposer_reader.try_from_reader()?
        };
        let mut deploy_hashes = vec![];
        {
            if self.has_deploy_hashes() {
                for deploy_hash_reader in self
                    .get_deploy_hashes()
                    .map_err(|_| Error::UnableToDeserialize)?
                    .iter()
                {
                    let deploy_hash = deploy_hash_reader.try_from_reader()?;
                    deploy_hashes.push(deploy_hash);
                }
            }
        }
        let mut transfer_hashes = vec![];
        {
            if self.has_transfer_hashes() {
                for transfer_hash_reader in self
                    .get_transfer_hashes()
                    .map_err(|_| Error::UnableToDeserialize)?
                    .iter()
                {
                    let deploy_hash = transfer_hash_reader.try_from_reader()?;
                    transfer_hashes.push(deploy_hash);
                }
            }
        }
        Ok(BlockBody::new(proposer, deploy_hashes, transfer_hashes))
    }
}

impl ToCapnpBytes for BlockBody {
    fn try_to_capnp_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut builder = capnp::message::Builder::new_default();
        let mut msg = builder.init_root::<block_body_capnp::block_body::Builder>();
        msg.try_to_builder(self)?;
        let mut serialized = Vec::new();
        capnp::serialize::write_message(&mut serialized, &builder)
            .map_err(|_| Error::UnableToSerialize)?;
        Ok(serialized)
    }
}

impl FromCapnpBytes for BlockBody {
    fn try_from_capnp_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let deserialized =
            capnp::serialize::read_message(bytes, capnp::message::ReaderOptions::new())
                .expect("unable to deserialize struct");

        let reader = deserialized
            .get_root::<block_body_capnp::block_body::Reader>()
            .map_err(|_| Error::UnableToDeserialize)?;
        reader.try_from_reader()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::{
        deploy_hash::tests::random_deploy_hash, public_key::tests::random_key_pair, random_byte,
    };
    use super::*;

    pub(crate) fn random_block_body() -> BlockBody {
        let (public_key, _) = random_key_pair();
        let mut deploy_hashes = vec![];
        for _ in 0..random_byte() {
            deploy_hashes.push(random_deploy_hash());
        }
        let mut transfer_hashes = vec![];
        for _ in 0..random_byte() {
            transfer_hashes.push(random_deploy_hash());
        }
        BlockBody::new(public_key, deploy_hashes, transfer_hashes)
    }

    #[test]
    fn block_body_capnp() {
        let block_body = random_block_body();
        let original = block_body.clone();
        let serialized = original.try_to_capnp_bytes().expect("serialization");
        let deserialized = BlockBody::try_from_capnp_bytes(&serialized).expect("deserialization");

        assert_eq!(original, deserialized);
    }
}
