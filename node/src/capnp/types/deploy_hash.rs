use super::{FromCapnpReader, ToCapnpBuilder};
use crate::capnp::{Error, FromCapnpBytes, ToCapnpBytes};
use crate::types::DeployHash;

#[allow(dead_code)]
pub(super) mod deploy_hash_capnp {
    include!(concat!(
        env!("OUT_DIR"),
        "/src/capnp/schemas/deploy_hash_capnp.rs"
    ));
}

impl ToCapnpBuilder<DeployHash> for deploy_hash_capnp::deploy_hash::Builder<'_> {
    fn try_to_builder(&mut self, deploy_hash: &DeployHash) -> Result<(), Error> {
        let mut digest_builder = self.reborrow().init_digest();
        digest_builder.try_to_builder(deploy_hash.inner())?;
        Ok(())
    }
}

impl FromCapnpReader<DeployHash> for deploy_hash_capnp::deploy_hash::Reader<'_> {
    fn try_from_reader(&self) -> Result<DeployHash, Error> {
        let digest_reader = self.get_digest().map_err(|_| Error::UnableToDeserialize)?;
        let digest = digest_reader.try_from_reader()?;
        Ok(digest.into())
    }
}

impl ToCapnpBytes for DeployHash {
    fn try_to_capnp_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut builder = capnp::message::Builder::new_default();
        let mut msg = builder.init_root::<deploy_hash_capnp::deploy_hash::Builder>();
        msg.try_to_builder(self)?;
        let mut serialized = Vec::new();
        capnp::serialize::write_message(&mut serialized, &builder)
            .map_err(|_| Error::UnableToSerialize)?;
        Ok(serialized)
    }
}

impl FromCapnpBytes for DeployHash {
    fn try_from_capnp_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let deserialized =
            capnp::serialize::read_message(bytes, capnp::message::ReaderOptions::new())
                .expect("unable to deserialize struct");

        let reader = deserialized
            .get_root::<deploy_hash_capnp::deploy_hash::Reader>()
            .map_err(|_| Error::UnableToDeserialize)?;
        reader.try_from_reader()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::digest::tests::random_digest;
    use super::*;

    pub(crate) fn random_deploy_hash() -> DeployHash {
        DeployHash::new(random_digest())
    }

    #[test]
    fn deploy_hash_capnp() {
        let deploy_hash = random_deploy_hash();
        let original = deploy_hash.clone();
        let serialized = original.try_to_capnp_bytes().expect("serialization");
        let deserialized = DeployHash::try_from_capnp_bytes(&serialized).expect("deserialization");

        assert_eq!(original, deserialized);
    }
}
