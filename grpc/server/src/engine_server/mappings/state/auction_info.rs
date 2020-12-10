use std::convert::{TryFrom, TryInto};

use crate::engine_server::{
    mappings::ParsingError, state, state::SeigniorageAllocation_oneof_variants,
};

use casper_types::{
    auction::{EraInfo, SeigniorageAllocation},
    bytesrepr::{self, ToBytes},
    U512,
};

impl From<SeigniorageAllocation> for state::SeigniorageAllocation {
    fn from(allocation: SeigniorageAllocation) -> Self {
        let mut ret = state::SeigniorageAllocation::new();
        match allocation {
            SeigniorageAllocation::Validator {
                validator_public_key,
                amount,
            } => {
                let validator_public_key_bytes = validator_public_key.to_bytes().unwrap();
                let amount = amount.into();
                let mut validator = state::SeigniorageAllocation_Validator::new();
                validator.set_validator_public_key(validator_public_key_bytes);
                validator.set_amount(amount);
                ret.set_validator(validator)
            }
            SeigniorageAllocation::Delegator {
                delegator_public_key,
                validator_public_key,
                amount,
            } => {
                let delegator_public_key_bytes = delegator_public_key.to_bytes().unwrap();
                let validator_public_key_bytes = validator_public_key.to_bytes().unwrap();
                let amount = amount.into();
                let mut delegator = state::SeigniorageAllocation_Delegator::new();
                delegator.set_delegator_public_key(delegator_public_key_bytes);
                delegator.set_validator_public_key(validator_public_key_bytes);
                delegator.set_amount(amount);
                ret.set_delegator(delegator)
            }
        }
        ret
    }
}

impl TryFrom<state::SeigniorageAllocation> for SeigniorageAllocation {
    type Error = ParsingError;

    fn try_from(
        pb_seigniorage_allocation: state::SeigniorageAllocation,
    ) -> Result<Self, Self::Error> {
        let pb_variants = pb_seigniorage_allocation.variants.as_ref().ok_or_else(|| {
            ParsingError("Unable to parse Protobuf SeigniorageAllocation".to_string())
        })?;

        match pb_variants {
            SeigniorageAllocation_oneof_variants::validator(pb_validator) => {
                let validator_public_key =
                    bytesrepr::deserialize(pb_validator.get_validator_public_key().to_vec())?;
                let amount = U512::try_from(pb_validator.get_amount().to_owned())?;
                Ok(SeigniorageAllocation::validator(
                    validator_public_key,
                    amount,
                ))
            }
            SeigniorageAllocation_oneof_variants::delegator(pb_delegator) => {
                let delegator_public_key =
                    bytesrepr::deserialize(pb_delegator.get_delegator_public_key().to_vec())?;
                let validator_public_key =
                    bytesrepr::deserialize(pb_delegator.get_validator_public_key().to_vec())?;
                let amount = U512::try_from(pb_delegator.get_amount().to_owned())?;
                Ok(SeigniorageAllocation::delegator(
                    delegator_public_key,
                    validator_public_key,
                    amount,
                ))
            }
        }
    }
}

impl From<EraInfo> for state::EraInfo {
    fn from(era_info: EraInfo) -> Self {
        let mut ret = state::EraInfo::new();
        let mut pb_vec_seigniorage_allocations = Vec::new();
        for allocation in era_info.seigniorage_allocations().iter() {
            pb_vec_seigniorage_allocations.push(allocation.to_owned().into());
        }
        ret.set_seigniorage_allocations(pb_vec_seigniorage_allocations.into());
        ret
    }
}

impl TryFrom<state::EraInfo> for EraInfo {
    type Error = ParsingError;

    fn try_from(pb_era_info: state::EraInfo) -> Result<Self, Self::Error> {
        let mut seigniorage_allocations = Vec::new();
        for pb_seigniorage_allocation in pb_era_info.get_seigniorage_allocations().iter() {
            seigniorage_allocations.push(pb_seigniorage_allocation.to_owned().try_into()?);
        }
        let mut ret = EraInfo::new();
        *ret.seigniorage_allocations_mut() = seigniorage_allocations;
        Ok(ret)
    }
}
