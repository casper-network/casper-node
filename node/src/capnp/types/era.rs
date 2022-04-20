use std::{collections::BTreeMap, convert::TryInto};

use casper_types::{EraId, PublicKey};

use super::{FromCapnpReader, ToCapnpBuilder};
use crate::{
    capnp::{DeserializeError, FromCapnpBytes, SerializeError, ToCapnpBytes},
    components::consensus::EraReport,
    types::EraEnd,
};

#[allow(dead_code)]
pub(super) mod era_capnp {
    include!(concat!(env!("OUT_DIR"), "/src/capnp/schemas/era_capnp.rs"));
}

impl ToCapnpBuilder<EraId> for era_capnp::era_id::Builder<'_> {
    fn try_to_builder(&mut self, era_id: &EraId) -> Result<(), SerializeError> {
        self.reborrow().set_id((*era_id).into());
        Ok(())
    }
}

impl FromCapnpReader<EraId> for era_capnp::era_id::Reader<'_> {
    fn try_from_reader(&self) -> Result<EraId, DeserializeError> {
        let id = self.get_id();
        Ok(id.into())
    }
}

impl ToCapnpBytes for EraId {
    fn try_to_capnp_bytes(&self) -> Result<Vec<u8>, SerializeError> {
        let mut builder = capnp::message::Builder::new_default();
        let mut msg = builder.init_root::<era_capnp::era_id::Builder>();
        msg.try_to_builder(self)?;
        let mut serialized = Vec::new();
        capnp::serialize::write_message(&mut serialized, &builder).map_err(SerializeError::from)?;
        Ok(serialized)
    }
}

impl FromCapnpBytes for EraId {
    fn try_from_capnp_bytes(bytes: &[u8]) -> Result<Self, DeserializeError> {
        let deserialized =
            capnp::serialize::read_message(bytes, capnp::message::ReaderOptions::new())
                .map_err(DeserializeError::from)?;
        let reader = deserialized
            .get_root::<era_capnp::era_id::Reader>()
            .map_err(DeserializeError::from)?;
        reader.try_from_reader()
    }
}

impl ToCapnpBuilder<EraReport<PublicKey>> for era_capnp::era_report::Builder<'_> {
    fn try_to_builder(&mut self, era_report: &EraReport<PublicKey>) -> Result<(), SerializeError> {
        {
            let equivocators = &era_report.equivocators;
            let equivocators_count: u32 = equivocators
                .len()
                .try_into()
                .map_err(|_| SerializeError::TooManyItems)?;
            let mut list_builder = self.reborrow().init_equivocators(equivocators_count);
            for (index, equivocator) in equivocators.iter().enumerate() {
                let mut msg = list_builder.reborrow().get(index as u32);
                msg.try_to_builder(equivocator)?;
            }
        }
        {
            let rewards = &era_report.rewards;
            let rewards_count: u32 = rewards
                .len()
                .try_into()
                .map_err(|_| SerializeError::TooManyItems)?;
            let mut map_builder = self.reborrow().init_rewards();
            {
                let mut entries_builder = map_builder.reborrow().init_entries(rewards_count);
                for (index, (reward_key, reward_value)) in rewards.iter().enumerate() {
                    let mut entry_builder = entries_builder.reborrow().get(index as u32);
                    {
                        let mut msg = entry_builder.reborrow().init_key();
                        msg.try_to_builder(reward_key)?;
                    }
                    entry_builder.set_value(*reward_value);
                }
            }
        }
        {
            let inactive_validators = &era_report.inactive_validators;
            let inactive_validators_count: u32 = inactive_validators
                .len()
                .try_into()
                .map_err(|_| SerializeError::TooManyItems)?;
            let mut list_builder = self
                .reborrow()
                .init_inactive_validators(inactive_validators_count);
            for (index, inactive_validator) in inactive_validators.iter().enumerate() {
                let mut msg = list_builder.reborrow().get(index as u32);
                msg.try_to_builder(inactive_validator)?;
            }
        }
        Ok(())
    }
}

impl FromCapnpReader<EraReport<PublicKey>> for era_capnp::era_report::Reader<'_> {
    fn try_from_reader(&self) -> Result<EraReport<PublicKey>, DeserializeError> {
        let mut equivocators = vec![];
        {
            if self.has_equivocators() {
                for equivocators_reader in self
                    .get_equivocators()
                    .map_err(DeserializeError::from)?
                    .iter()
                {
                    let equivocator = equivocators_reader.try_from_reader()?;
                    equivocators.push(equivocator);
                }
            }
        }
        let mut rewards = BTreeMap::new();
        {
            if self.has_rewards() {
                let entries_reader = self.get_rewards().map_err(DeserializeError::from)?;
                for entry_reader in entries_reader
                    .get_entries()
                    .map_err(DeserializeError::from)?
                    .iter()
                {
                    let key = entry_reader
                        .get_key()
                        .map_err(DeserializeError::from)?
                        .try_from_reader()?;
                    let value = entry_reader.get_value();
                    rewards.insert(key, value);
                }
            }
        }
        let mut inactive_validators = vec![];
        {
            if self.has_inactive_validators() {
                for inactive_validators_reader in self
                    .get_inactive_validators()
                    .map_err(DeserializeError::from)?
                    .iter()
                {
                    let inactive_validator = inactive_validators_reader.try_from_reader()?;
                    inactive_validators.push(inactive_validator);
                }
            }
        }
        Ok(EraReport {
            equivocators,
            rewards,
            inactive_validators,
        })
    }
}

impl ToCapnpBytes for EraReport<PublicKey> {
    fn try_to_capnp_bytes(&self) -> Result<Vec<u8>, SerializeError> {
        let mut builder = capnp::message::Builder::new_default();
        let mut msg = builder.init_root::<era_capnp::era_report::Builder>();
        msg.try_to_builder(self)?;
        let mut serialized = Vec::new();
        capnp::serialize::write_message(&mut serialized, &builder).map_err(SerializeError::from)?;
        Ok(serialized)
    }
}

impl FromCapnpBytes for EraReport<PublicKey> {
    fn try_from_capnp_bytes(bytes: &[u8]) -> Result<Self, DeserializeError> {
        let deserialized =
            capnp::serialize::read_message(bytes, capnp::message::ReaderOptions::new())
                .map_err(DeserializeError::from)?;
        let reader = deserialized
            .get_root::<era_capnp::era_report::Reader>()
            .map_err(DeserializeError::from)?;
        reader.try_from_reader()
    }
}

impl ToCapnpBuilder<EraEnd> for era_capnp::era_end::Builder<'_> {
    fn try_to_builder(&mut self, era_end: &EraEnd) -> Result<(), SerializeError> {
        {
            let era_report = era_end.era_report();
            let mut era_report_builder = self.reborrow().init_era_report();
            era_report_builder.reborrow().try_to_builder(era_report)?;
        }
        {
            let weights = era_end.next_era_validator_weights();
            let weights_count: u32 = weights
                .len()
                .try_into()
                .map_err(|_| SerializeError::TooManyItems)?;
            let mut map_builder = self.reborrow().init_next_era_validator_weights();
            {
                let mut entries_builder = map_builder.reborrow().init_entries(weights_count);
                for (index, (key, value)) in weights.iter().enumerate() {
                    let mut entry_builder = entries_builder.reborrow().get(index as u32);
                    {
                        let mut msg = entry_builder.reborrow().init_key();
                        msg.try_to_builder(key)?;
                    }
                    {
                        let mut msg = entry_builder.reborrow().init_value();
                        msg.try_to_builder(value)?;
                    }
                }
            }
        }
        Ok(())
    }
}

impl FromCapnpReader<EraEnd> for era_capnp::era_end::Reader<'_> {
    fn try_from_reader(&self) -> Result<EraEnd, DeserializeError> {
        let era_report = {
            let era_report_reader = self.get_era_report().map_err(DeserializeError::from)?;
            era_report_reader.try_from_reader()?
        };
        let mut weights = BTreeMap::new();
        {
            if self.has_next_era_validator_weights() {
                let entries_reader = self
                    .get_next_era_validator_weights()
                    .map_err(DeserializeError::from)?;
                for entry_reader in entries_reader
                    .get_entries()
                    .map_err(DeserializeError::from)?
                    .iter()
                {
                    let key = entry_reader
                        .get_key()
                        .map_err(DeserializeError::from)?
                        .try_from_reader()?;
                    let value = entry_reader
                        .get_value()
                        .map_err(DeserializeError::from)?
                        .try_from_reader()?;
                    weights.insert(key, value);
                }
            }
        }
        Ok(EraEnd::new(era_report, weights))
    }
}

impl ToCapnpBytes for EraEnd {
    fn try_to_capnp_bytes(&self) -> Result<Vec<u8>, SerializeError> {
        let mut builder = capnp::message::Builder::new_default();
        let mut msg = builder.init_root::<era_capnp::era_end::Builder>();
        msg.try_to_builder(self)?;
        let mut serialized = Vec::new();
        capnp::serialize::write_message(&mut serialized, &builder).map_err(SerializeError::from)?;
        Ok(serialized)
    }
}

impl FromCapnpBytes for EraEnd {
    fn try_from_capnp_bytes(bytes: &[u8]) -> Result<Self, DeserializeError> {
        let deserialized =
            capnp::serialize::read_message(bytes, capnp::message::ReaderOptions::new())
                .map_err(DeserializeError::from)?;
        let reader = deserialized
            .get_root::<era_capnp::era_end::Reader>()
            .map_err(DeserializeError::from)?;
        reader.try_from_reader()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::common::tests::random_u512;
    use super::super::public_key::tests::random_key_pair;
    use super::super::{random_byte, random_u64};
    use super::*;

    use casper_types::{PublicKey, U512};

    use crate::{
        capnp::{FromCapnpBytes, ToCapnpBytes},
        components::consensus::EraReport,
    };

    pub(crate) fn random_era_id() -> EraId {
        random_u64().into()
    }

    fn random_era_report() -> EraReport<PublicKey> {
        let mut equivocators = vec![];
        for _ in 0..random_byte() {
            let (public_key, _) = random_key_pair();
            equivocators.push(public_key);
        }

        let mut inactive_validators = vec![];
        for _ in 0..random_byte() {
            let (public_key, _) = random_key_pair();
            inactive_validators.push(public_key);
        }

        let mut rewards: BTreeMap<PublicKey, u64> = BTreeMap::new();
        for _ in 0..random_byte() {
            let (public_key, _) = random_key_pair();
            rewards.insert(public_key, random_u64());
        }

        EraReport {
            equivocators,
            rewards,
            inactive_validators,
        }
    }

    pub(crate) fn random_era_end() -> EraEnd {
        let mut next_era_validator_weights: BTreeMap<PublicKey, U512> = BTreeMap::new();
        for _ in 0..random_byte() {
            let (public_key, _) = random_key_pair();
            next_era_validator_weights.insert(public_key, random_u512());
        }
        EraEnd::new(random_era_report(), next_era_validator_weights)
    }

    #[test]
    fn era_id_capnp() {
        let original = random_era_id();
        let serialized = original.try_to_capnp_bytes().expect("serialization");
        let deserialized = EraId::try_from_capnp_bytes(&serialized).expect("deserialization");

        assert_eq!(original, deserialized);
    }

    #[test]
    fn era_report_capnp() {
        let original = random_era_report();
        let serialized = original.try_to_capnp_bytes().expect("serialization");
        let deserialized = EraReport::try_from_capnp_bytes(&serialized).expect("deserialization");

        assert_eq!(original, deserialized);
    }

    #[test]
    fn era_end_capnp() {
        let original = random_era_end();
        let serialized = original.try_to_capnp_bytes().expect("serialization");
        let deserialized = EraEnd::try_from_capnp_bytes(&serialized).expect("deserialization");

        assert_eq!(original, deserialized);
    }
}
