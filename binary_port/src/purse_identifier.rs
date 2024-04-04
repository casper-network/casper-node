#[cfg(test)]
use casper_types::testing::TestRng;
#[cfg(test)]
use rand::Rng;

use casper_types::{
    account::AccountHash,
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    EntityAddr, PublicKey, URef,
};

const PAYMENT_PURSE_TAG: u8 = 0;
const ACCUMULATE_PURSE_TAG: u8 = 1;
const UREF_PURSE_TAG: u8 = 2;
const PUBLIC_KEY_PURSE_TAG: u8 = 3;
const ACCOUNT_PURSE_TAG: u8 = 4;
const ENTITY_PURSE_TAG: u8 = 5;

/// Identifier for balance lookup.
#[derive(Clone, Debug, PartialEq)]
pub enum PurseIdentifier {
    Payment,
    Accumulate,
    Purse(URef),
    PublicKey(PublicKey),
    Account(AccountHash),
    Entity(EntityAddr),
}

impl PurseIdentifier {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..6) {
            PAYMENT_PURSE_TAG => PurseIdentifier::Payment,
            ACCUMULATE_PURSE_TAG => PurseIdentifier::Accumulate,
            UREF_PURSE_TAG => PurseIdentifier::Purse(rng.gen()),
            PUBLIC_KEY_PURSE_TAG => PurseIdentifier::PublicKey(PublicKey::random(rng)),
            ACCOUNT_PURSE_TAG => PurseIdentifier::Account(rng.gen()),
            ENTITY_PURSE_TAG => PurseIdentifier::Entity(rng.gen()),
            _ => unreachable!(),
        }
    }
}

impl ToBytes for PurseIdentifier {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            PurseIdentifier::Payment => PAYMENT_PURSE_TAG.write_bytes(writer),
            PurseIdentifier::Accumulate => ACCUMULATE_PURSE_TAG.write_bytes(writer),
            PurseIdentifier::Purse(uref) => {
                UREF_PURSE_TAG.write_bytes(writer)?;
                uref.write_bytes(writer)
            }
            PurseIdentifier::PublicKey(key) => {
                PUBLIC_KEY_PURSE_TAG.write_bytes(writer)?;
                key.write_bytes(writer)
            }
            PurseIdentifier::Account(account) => {
                ACCOUNT_PURSE_TAG.write_bytes(writer)?;
                account.write_bytes(writer)
            }
            PurseIdentifier::Entity(entity) => {
                ENTITY_PURSE_TAG.write_bytes(writer)?;
                entity.write_bytes(writer)
            }
        }
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                PurseIdentifier::Payment => 0,
                PurseIdentifier::Accumulate => 0,
                PurseIdentifier::Purse(uref) => uref.serialized_length(),
                PurseIdentifier::PublicKey(key) => key.serialized_length(),
                PurseIdentifier::Account(account) => account.serialized_length(),
                PurseIdentifier::Entity(entity) => entity.serialized_length(),
            }
    }
}

impl FromBytes for PurseIdentifier {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            PAYMENT_PURSE_TAG => Ok((PurseIdentifier::Payment, remainder)),
            ACCUMULATE_PURSE_TAG => Ok((PurseIdentifier::Accumulate, remainder)),
            UREF_PURSE_TAG => {
                let (uref, remainder) = URef::from_bytes(remainder)?;
                Ok((PurseIdentifier::Purse(uref), remainder))
            }
            PUBLIC_KEY_PURSE_TAG => {
                let (key, remainder) = PublicKey::from_bytes(remainder)?;
                Ok((PurseIdentifier::PublicKey(key), remainder))
            }
            ACCOUNT_PURSE_TAG => {
                let (account, remainder) = AccountHash::from_bytes(remainder)?;
                Ok((PurseIdentifier::Account(account), remainder))
            }
            ENTITY_PURSE_TAG => {
                let (entity, remainder) = EntityAddr::from_bytes(remainder)?;
                Ok((PurseIdentifier::Entity(entity), remainder))
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

        let val = PurseIdentifier::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
