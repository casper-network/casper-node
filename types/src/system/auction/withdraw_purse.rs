use alloc::vec::Vec;

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    CLType, CLTyped, EraId, PublicKey, URef, U512,
};

/// A withdraw purse, a legacy structure.
#[derive(PartialEq, Eq, Debug, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct WithdrawPurse {
    /// Bonding Purse
    pub(crate) bonding_purse: URef,
    /// Validators public key.
    pub(crate) validator_public_key: PublicKey,
    /// Unbonders public key.
    pub(crate) unbonder_public_key: PublicKey,
    /// Era in which this unbonding request was created.
    pub(crate) era_of_creation: EraId,
    /// Unbonding Amount.
    pub(crate) amount: U512,
}

impl WithdrawPurse {
    /// Creates [`WithdrawPurse`] instance for an unbonding request.
    pub const fn new(
        bonding_purse: URef,
        validator_public_key: PublicKey,
        unbonder_public_key: PublicKey,
        era_of_creation: EraId,
        amount: U512,
    ) -> Self {
        Self {
            bonding_purse,
            validator_public_key,
            unbonder_public_key,
            era_of_creation,
            amount,
        }
    }

    /// Checks if given request is made by a validator by checking if public key of unbonder is same
    /// as a key owned by validator.
    pub fn is_validator(&self) -> bool {
        self.validator_public_key == self.unbonder_public_key
    }

    /// Returns bonding purse used to make this unbonding request.
    pub fn bonding_purse(&self) -> &URef {
        &self.bonding_purse
    }

    /// Returns public key of validator.
    pub fn validator_public_key(&self) -> &PublicKey {
        &self.validator_public_key
    }

    /// Returns public key of unbonder.
    ///
    /// For withdrawal requests that originated from validator's public key through `withdraw_bid`
    /// entrypoint this is equal to [`WithdrawPurse::validator_public_key`] and
    /// [`WithdrawPurse::is_validator`] is `true`.
    pub fn unbonder_public_key(&self) -> &PublicKey {
        &self.unbonder_public_key
    }

    /// Returns era which was used to create this unbonding request.
    pub fn era_of_creation(&self) -> EraId {
        self.era_of_creation
    }

    /// Returns unbonding amount.
    pub fn amount(&self) -> &U512 {
        &self.amount
    }
}

impl ToBytes for WithdrawPurse {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.extend(&self.bonding_purse.to_bytes()?);
        result.extend(&self.validator_public_key.to_bytes()?);
        result.extend(&self.unbonder_public_key.to_bytes()?);
        result.extend(&self.era_of_creation.to_bytes()?);
        result.extend(&self.amount.to_bytes()?);

        Ok(result)
    }
    fn serialized_length(&self) -> usize {
        self.bonding_purse.serialized_length()
            + self.validator_public_key.serialized_length()
            + self.unbonder_public_key.serialized_length()
            + self.era_of_creation.serialized_length()
            + self.amount.serialized_length()
    }
}

impl FromBytes for WithdrawPurse {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (bonding_purse, remainder) = FromBytes::from_bytes(bytes)?;
        let (validator_public_key, remainder) = FromBytes::from_bytes(remainder)?;
        let (unbonder_public_key, remainder) = FromBytes::from_bytes(remainder)?;
        let (era_of_creation, remainder) = FromBytes::from_bytes(remainder)?;
        let (amount, remainder) = FromBytes::from_bytes(remainder)?;

        Ok((
            WithdrawPurse {
                bonding_purse,
                validator_public_key,
                unbonder_public_key,
                era_of_creation,
                amount,
            },
            remainder,
        ))
    }
}

impl CLTyped for WithdrawPurse {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

#[cfg(test)]
mod tests {
    use crate::{bytesrepr, AccessRights, EraId, PublicKey, SecretKey, URef, U512};

    use super::WithdrawPurse;

    const BONDING_PURSE: URef = URef::new([41; 32], AccessRights::READ_ADD_WRITE);
    const ERA_OF_WITHDRAWAL: EraId = EraId::MAX;

    fn validator_public_key() -> PublicKey {
        let secret_key = SecretKey::ed25519_from_bytes([44; SecretKey::ED25519_LENGTH]).unwrap();
        PublicKey::from(&secret_key)
    }

    fn unbonder_public_key() -> PublicKey {
        let secret_key = SecretKey::ed25519_from_bytes([45; SecretKey::ED25519_LENGTH]).unwrap();
        PublicKey::from(&secret_key)
    }

    fn amount() -> U512 {
        U512::max_value() - 1
    }

    #[test]
    fn serialization_roundtrip_for_withdraw_purse() {
        let withdraw_purse = WithdrawPurse {
            bonding_purse: BONDING_PURSE,
            validator_public_key: validator_public_key(),
            unbonder_public_key: unbonder_public_key(),
            era_of_creation: ERA_OF_WITHDRAWAL,
            amount: amount(),
        };

        bytesrepr::test_serialization_roundtrip(&withdraw_purse);
    }

    #[test]
    fn should_be_validator_condition_for_withdraw_purse() {
        let validator_withdraw_purse = WithdrawPurse::new(
            BONDING_PURSE,
            validator_public_key(),
            validator_public_key(),
            ERA_OF_WITHDRAWAL,
            amount(),
        );
        assert!(validator_withdraw_purse.is_validator());
    }

    #[test]
    fn should_be_delegator_condition_for_withdraw_purse() {
        let delegator_withdraw_purse = WithdrawPurse::new(
            BONDING_PURSE,
            validator_public_key(),
            unbonder_public_key(),
            ERA_OF_WITHDRAWAL,
            amount(),
        );
        assert!(!delegator_withdraw_purse.is_validator());
    }
}
