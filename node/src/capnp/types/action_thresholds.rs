use casper_types::account::{ActionThresholds, Weight};

use crate::capnp::{DeserializeError, FromCapnpBytes, SerializeError, ToCapnpBytes};

use super::{FromCapnpReader, ToCapnpBuilder};

#[allow(dead_code)]
pub(super) mod action_thresholds_capnp {
    include!(concat!(
        env!("OUT_DIR"),
        "/src/capnp/schemas/action_thresholds_capnp.rs"
    ));
}

impl ToCapnpBuilder<ActionThresholds> for action_thresholds_capnp::action_thresholds::Builder<'_> {
    fn try_to_builder(
        &mut self,
        action_thresholds: &ActionThresholds,
    ) -> Result<(), SerializeError> {
        {
            let mut deployment_builder = self.reborrow().init_deployment();
            deployment_builder.try_to_builder(action_thresholds.deployment())?;
        }
        {
            let mut key_management_builder = self.reborrow().init_key_management();
            key_management_builder.try_to_builder(action_thresholds.key_management())?;
        }
        Ok(())
    }
}

impl FromCapnpReader<ActionThresholds> for action_thresholds_capnp::action_thresholds::Reader<'_> {
    fn try_from_reader(&self) -> Result<ActionThresholds, DeserializeError> {
        let deployment = Weight::new(self.get_deployment()?.get_value());
        let key_management = Weight::new(self.get_key_management()?.get_value());
        Ok(ActionThresholds::new(deployment, key_management)?)
    }
}

impl ToCapnpBytes for ActionThresholds {
    fn try_to_capnp_bytes(&self) -> Result<Vec<u8>, SerializeError> {
        let mut builder = capnp::message::Builder::new_default();
        let mut msg = builder.init_root::<action_thresholds_capnp::action_thresholds::Builder>();
        msg.try_to_builder(self)?;
        let mut serialized = Vec::new();
        capnp::serialize::write_message(&mut serialized, &builder)?;
        Ok(serialized)
    }
}

impl FromCapnpBytes for ActionThresholds {
    fn try_from_capnp_bytes(bytes: &[u8]) -> Result<Self, DeserializeError> {
        let deserialized =
            capnp::serialize::read_message(bytes, capnp::message::ReaderOptions::new())?;

        let reader =
            deserialized.get_root::<action_thresholds_capnp::action_thresholds::Reader>()?;
        reader.try_from_reader()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use casper_types::account::{ActionThresholds, Weight};

    use crate::capnp::{types::random_byte, FromCapnpBytes, ToCapnpBytes};

    pub(crate) fn random_action_thresholds() -> ActionThresholds {
        let (deployment, key_management) = loop {
            let deployment = random_byte();
            let key_management = random_byte();
            if deployment <= key_management {
                break (deployment, key_management);
            }
        };
        ActionThresholds::new(Weight::new(deployment), Weight::new(key_management))
            .expect("should create ActionThresholds")
    }

    #[test]
    fn action_thresholds_capnp() {
        let action_thresholds = random_action_thresholds();
        let original = action_thresholds;
        let serialized = original.try_to_capnp_bytes().expect("serialization");
        let deserialized =
            ActionThresholds::try_from_capnp_bytes(&serialized).expect("deserialization");

        assert_eq!(original, deserialized);
    }
}
