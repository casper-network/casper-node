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

use super::WithdrawPurse;

/// Unbonding purse.
#[derive(PartialEq, Eq, Debug, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct UnbondingPurse {
    /// Bonding Purse
    bonding_purse: URef,
    /// Validators public key.
    validator_public_key: PublicKey,
    /// Unbonders public key.
    unbonder_public_key: PublicKey,
    /// Era in which this unbonding request was created.
    era_of_creation: EraId,
    /// Unbonding Amount.
    amount: U512,
    /// The validator public key to re-delegate to.
    new_validator: Option<PublicKey>,
}

impl UnbondingPurse {
    /// Creates [`UnbondingPurse`] instance for an unbonding request.
    pub const fn new(
        bonding_purse: URef,
        validator_public_key: PublicKey,
        unbonder_public_key: PublicKey,
        era_of_creation: EraId,
        amount: U512,
        new_validator: Option<PublicKey>,
    ) -> Self {
        Self {
            bonding_purse,
            validator_public_key,
            unbonder_public_key,
            era_of_creation,
            amount,
            new_validator,
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
    /// entrypoint this is equal to [`UnbondingPurse::validator_public_key`] and
    /// [`UnbondingPurse::is_validator`] is `true`.
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

    /// Returns the public key for the new validator.
    pub fn new_validator(&self) -> &Option<PublicKey> {
        &self.new_validator
    }

    /// Sets amount to provided value.
    pub fn with_amount(&mut self, amount: U512) {
        self.amount = amount;
    }
}

impl ToBytes for UnbondingPurse {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.extend(&self.bonding_purse.to_bytes()?);
        result.extend(&self.validator_public_key.to_bytes()?);
        result.extend(&self.unbonder_public_key.to_bytes()?);
        result.extend(&self.era_of_creation.to_bytes()?);
        result.extend(&self.amount.to_bytes()?);
        result.extend(&self.new_validator.to_bytes()?);
        Ok(result)
    }
    fn serialized_length(&self) -> usize {
        self.bonding_purse.serialized_length()
            + self.validator_public_key.serialized_length()
            + self.unbonder_public_key.serialized_length()
            + self.era_of_creation.serialized_length()
            + self.amount.serialized_length()
            + self.new_validator.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.bonding_purse.write_bytes(writer)?;
        self.validator_public_key.write_bytes(writer)?;
        self.unbonder_public_key.write_bytes(writer)?;
        self.era_of_creation.write_bytes(writer)?;
        self.amount.write_bytes(writer)?;
        self.new_validator.write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for UnbondingPurse {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (bonding_purse, remainder) = FromBytes::from_bytes(bytes)?;
        let (validator_public_key, remainder) = FromBytes::from_bytes(remainder)?;
        let (unbonder_public_key, remainder) = FromBytes::from_bytes(remainder)?;
        let (era_of_creation, remainder) = FromBytes::from_bytes(remainder)?;
        let (amount, remainder) = FromBytes::from_bytes(remainder)?;
        let (new_validator, remainder) = Option::<PublicKey>::from_bytes(remainder)?;

        Ok((
            UnbondingPurse {
                bonding_purse,
                validator_public_key,
                unbonder_public_key,
                era_of_creation,
                amount,
                new_validator,
            },
            remainder,
        ))
    }
}

impl CLTyped for UnbondingPurse {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl From<WithdrawPurse> for UnbondingPurse {
    fn from(withdraw_purse: WithdrawPurse) -> Self {
        UnbondingPurse::new(
            withdraw_purse.bonding_purse,
            withdraw_purse.validator_public_key,
            withdraw_purse.unbonder_public_key,
            withdraw_purse.era_of_creation,
            withdraw_purse.amount,
            None,
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        bytesrepr, system::auction::UnbondingPurse, AccessRights, EraId, PublicKey, SecretKey,
        URef, U512,
    };

    const BONDING_PURSE: URef = URef::new([14; 32], AccessRights::READ_ADD_WRITE);
    const ERA_OF_WITHDRAWAL: EraId = EraId::MAX;

    fn validator_public_key() -> PublicKey {
        let secret_key = SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH]).unwrap();
        PublicKey::from(&secret_key)
    }

    fn unbonder_public_key() -> PublicKey {
        let secret_key = SecretKey::ed25519_from_bytes([43; SecretKey::ED25519_LENGTH]).unwrap();
        PublicKey::from(&secret_key)
    }

    fn amount() -> U512 {
        U512::max_value() - 1
    }

    #[test]
    fn serialization_roundtrip_for_unbonding_purse() {
        let unbonding_purse = UnbondingPurse {
            bonding_purse: BONDING_PURSE,
            validator_public_key: validator_public_key(),
            unbonder_public_key: unbonder_public_key(),
            era_of_creation: ERA_OF_WITHDRAWAL,
            amount: amount(),
            new_validator: None,
        };

        bytesrepr::test_serialization_roundtrip(&unbonding_purse);
    }

    #[test]
    fn should_be_validator_condition_for_unbonding_purse() {
        let validator_unbonding_purse = UnbondingPurse::new(
            BONDING_PURSE,
            validator_public_key(),
            validator_public_key(),
            ERA_OF_WITHDRAWAL,
            amount(),
            None,
        );
        assert!(validator_unbonding_purse.is_validator());
    }

    #[test]
    fn should_be_delegator_condition_for_unbonding_purse() {
        let delegator_unbonding_purse = UnbondingPurse::new(
            BONDING_PURSE,
            validator_public_key(),
            unbonder_public_key(),
            ERA_OF_WITHDRAWAL,
            amount(),
            None,
        );
        assert!(!delegator_unbonding_purse.is_validator());
    }
}
