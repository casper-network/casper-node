#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    account::AccountHash,
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    contract_messages::TopicNameHash,
    system::{auction::BidAddrTag, mint::BalanceHoldAddrTag},
    EntityAddr, KeyTag, URefAddr,
};
use alloc::vec::Vec;
#[cfg(any(feature = "testing", test))]
use rand::Rng;

/// Key prefixes used for querying the global state.
#[derive(Debug, PartialEq, Eq, Clone, PartialOrd, Ord, Hash)]
pub enum KeyPrefix {
    /// Retrieves all delegator bid addresses for a given validator.
    DelegatorBidAddrsByValidator(AccountHash),
    /// Retrieves all messages for a given entity.
    MessagesByEntity(EntityAddr),
    /// Retrieves all messages for a given entity and topic.
    MessagesByEntityAndTopic(EntityAddr, TopicNameHash),
    /// Retrieves all named keys for a given entity.
    NamedKeysByEntity(EntityAddr),
    /// Retrieves all gas balance holds for a given purse.
    GasBalanceHoldsByPurse(URefAddr),
    /// Retrieves all processing balance holds for a given purse.
    ProcessingBalanceHoldsByPurse(URefAddr),
}

impl KeyPrefix {
    /// Returns a random `KeyPrefix`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..6) {
            0 => KeyPrefix::DelegatorBidAddrsByValidator(rng.gen()),
            1 => KeyPrefix::MessagesByEntity(rng.gen()),
            2 => KeyPrefix::MessagesByEntityAndTopic(rng.gen(), rng.gen()),
            3 => KeyPrefix::NamedKeysByEntity(rng.gen()),
            4 => KeyPrefix::GasBalanceHoldsByPurse(rng.gen()),
            5 => KeyPrefix::ProcessingBalanceHoldsByPurse(rng.gen()),
            _ => unreachable!(),
        }
    }
}

impl ToBytes for KeyPrefix {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut result = bytesrepr::unchecked_allocate_buffer(self);
        self.write_bytes(&mut result)?;
        Ok(result)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), crate::bytesrepr::Error> {
        match self {
            KeyPrefix::DelegatorBidAddrsByValidator(validator) => {
                writer.push(KeyTag::BidAddr as u8);
                writer.push(BidAddrTag::Delegator as u8);
                validator.write_bytes(writer)?;
            }
            KeyPrefix::MessagesByEntity(entity) => {
                writer.push(KeyTag::Message as u8);
                entity.write_bytes(writer)?;
            }
            KeyPrefix::MessagesByEntityAndTopic(entity, topic) => {
                writer.push(KeyTag::Message as u8);
                entity.write_bytes(writer)?;
                topic.write_bytes(writer)?;
            }
            KeyPrefix::NamedKeysByEntity(entity) => {
                writer.push(KeyTag::NamedKey as u8);
                entity.write_bytes(writer)?;
            }
            KeyPrefix::GasBalanceHoldsByPurse(uref) => {
                writer.push(KeyTag::BalanceHold as u8);
                writer.push(BalanceHoldAddrTag::Gas as u8);
                uref.write_bytes(writer)?;
            }
            KeyPrefix::ProcessingBalanceHoldsByPurse(uref) => {
                writer.push(KeyTag::BalanceHold as u8);
                writer.push(BalanceHoldAddrTag::Processing as u8);
                uref.write_bytes(writer)?;
            }
        }
        Ok(())
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                KeyPrefix::DelegatorBidAddrsByValidator(validator) => {
                    U8_SERIALIZED_LENGTH + validator.serialized_length()
                }
                KeyPrefix::MessagesByEntity(entity) => entity.serialized_length(),
                KeyPrefix::MessagesByEntityAndTopic(entity, topic) => {
                    entity.serialized_length() + topic.serialized_length()
                }
                KeyPrefix::NamedKeysByEntity(entity) => entity.serialized_length(),
                KeyPrefix::GasBalanceHoldsByPurse(uref) => {
                    U8_SERIALIZED_LENGTH + uref.serialized_length()
                }
                KeyPrefix::ProcessingBalanceHoldsByPurse(uref) => {
                    U8_SERIALIZED_LENGTH + uref.serialized_length()
                }
            }
    }
}

impl FromBytes for KeyPrefix {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        let result = match tag {
            tag if tag == KeyTag::BidAddr as u8 => {
                let (bid_addr_tag, remainder) = u8::from_bytes(remainder)?;
                match bid_addr_tag {
                    tag if tag == BidAddrTag::Delegator as u8 => {
                        let (validator, remainder) = AccountHash::from_bytes(remainder)?;
                        (
                            KeyPrefix::DelegatorBidAddrsByValidator(validator),
                            remainder,
                        )
                    }
                    _ => return Err(bytesrepr::Error::Formatting),
                }
            }
            tag if tag == KeyTag::Message as u8 => {
                let (entity, remainder) = EntityAddr::from_bytes(remainder)?;
                if remainder.is_empty() {
                    (KeyPrefix::MessagesByEntity(entity), remainder)
                } else {
                    let (topic, remainder) = TopicNameHash::from_bytes(remainder)?;
                    (
                        KeyPrefix::MessagesByEntityAndTopic(entity, topic),
                        remainder,
                    )
                }
            }
            tag if tag == KeyTag::NamedKey as u8 => {
                let (entity, remainder) = EntityAddr::from_bytes(remainder)?;
                (KeyPrefix::NamedKeysByEntity(entity), remainder)
            }
            tag if tag == KeyTag::BalanceHold as u8 => {
                let (balance_hold_addr_tag, remainder) = u8::from_bytes(remainder)?;
                let (uref, remainder) = URefAddr::from_bytes(remainder)?;
                match balance_hold_addr_tag {
                    tag if tag == BalanceHoldAddrTag::Gas as u8 => {
                        (KeyPrefix::GasBalanceHoldsByPurse(uref), remainder)
                    }
                    tag if tag == BalanceHoldAddrTag::Processing as u8 => {
                        (KeyPrefix::ProcessingBalanceHoldsByPurse(uref), remainder)
                    }
                    _ => return Err(bytesrepr::Error::Formatting),
                }
            }
            _ => return Err(bytesrepr::Error::Formatting),
        };
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        addressable_entity::NamedKeyAddr,
        contract_messages::MessageAddr,
        gens::key_prefix_arb,
        system::{auction::BidAddr, mint::BalanceHoldAddr},
        BlockTime, Key,
    };

    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn bytesrepr_roundtrip(key_prefix in key_prefix_arb()) {
            bytesrepr::test_serialization_roundtrip(&key_prefix);
        }
    }

    #[test]
    fn key_serializer_compat() {
        // This test ensures that the `KeyPrefix` deserializer is compatible with the `Key`
        // serializer. Combined with the `bytesrepr_roundtrip` test, this ensures that
        // `KeyPrefix` is binary compatible with `Key`.

        let rng = &mut TestRng::new();

        let hash1 = rng.gen();
        let hash2 = rng.gen();

        for (key, prefix) in [
            (
                Key::BidAddr(BidAddr::new_delegator_addr((hash1, hash2))),
                KeyPrefix::DelegatorBidAddrsByValidator(AccountHash::new(hash1)),
            ),
            (
                Key::Message(MessageAddr::new_message_addr(
                    EntityAddr::Account(hash1),
                    TopicNameHash::new(hash2),
                    0,
                )),
                KeyPrefix::MessagesByEntityAndTopic(
                    EntityAddr::Account(hash1),
                    TopicNameHash::new(hash2),
                ),
            ),
            (
                Key::NamedKey(NamedKeyAddr::new_named_key_entry(
                    EntityAddr::Account(hash1),
                    hash2,
                )),
                KeyPrefix::NamedKeysByEntity(EntityAddr::Account(hash1)),
            ),
            (
                Key::BalanceHold(BalanceHoldAddr::new_gas(hash1, BlockTime::new(0))),
                KeyPrefix::GasBalanceHoldsByPurse(hash1),
            ),
            (
                Key::BalanceHold(BalanceHoldAddr::new_processing(hash1, BlockTime::new(0))),
                KeyPrefix::ProcessingBalanceHoldsByPurse(hash1),
            ),
        ] {
            let key_bytes = key.to_bytes().expect("should serialize key");
            let (parsed_key_prefix, remainder) =
                KeyPrefix::from_bytes(&key_bytes).expect("should deserialize key prefix");
            assert_eq!(parsed_key_prefix, prefix, "key: {:?}", key);
            assert!(!remainder.is_empty(), "key: {:?}", key);
        }
    }
}
