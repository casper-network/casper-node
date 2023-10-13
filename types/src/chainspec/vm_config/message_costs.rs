#[cfg(feature = "datasize")]
use datasize::DataSize;
use num_traits::Zero;
use rand::{distributions::Standard, prelude::*, Rng};
use serde::{Deserialize, Serialize};
use std::ops::Add;

use crate::bytesrepr::{self, FromBytes, ToBytes};

/// Configuration for messages limits.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[serde(deny_unknown_fields)]
pub struct MessageCosts {
    /// Cost for emitting a message.
    pub first_message_cost: u32,
    /// Cost increase for successive messages emitted for each execution.
    pub cost_increase_per_message: u32,
}

impl MessageCosts {
    /// Return cost for emitting a message.
    pub fn first_message_cost(&self) -> u32 {
        self.first_message_cost
    }

    /// Return the ratio of cost increase for successive messages emitted for each execution.
    pub fn cost_increase_per_message(&self) -> u32 {
        self.cost_increase_per_message
    }
}

impl Default for MessageCosts {
    fn default() -> Self {
        Self {
            first_message_cost: 100,
            cost_increase_per_message: 50,
        }
    }
}

impl Zero for MessageCosts {
    fn zero() -> Self {
        MessageCosts {
            first_message_cost: 0,
            cost_increase_per_message: 0,
        }
    }

    fn is_zero(&self) -> bool {
        self.first_message_cost == 0 && self.cost_increase_per_message == 0
    }
}

impl Add for MessageCosts {
    type Output = MessageCosts;

    fn add(self, other: MessageCosts) -> MessageCosts {
        MessageCosts {
            first_message_cost: self.first_message_cost + other.first_message_cost,
            cost_increase_per_message: self.cost_increase_per_message
                + other.cost_increase_per_message,
        }
    }
}

impl ToBytes for MessageCosts {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);

        ret.append(&mut self.first_message_cost.to_bytes()?);
        ret.append(&mut self.cost_increase_per_message.to_bytes()?);

        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        self.first_message_cost.serialized_length()
            + self.cost_increase_per_message.serialized_length()
    }
}

impl FromBytes for MessageCosts {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (first_message_cost, rem) = FromBytes::from_bytes(bytes)?;
        let (cost_increase_per_message, rem) = FromBytes::from_bytes(rem)?;

        Ok((
            MessageCosts {
                first_message_cost,
                cost_increase_per_message,
            },
            rem,
        ))
    }
}

impl Distribution<MessageCosts> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> MessageCosts {
        MessageCosts {
            first_message_cost: rng.gen(),
            cost_increase_per_message: rng.gen(),
        }
    }
}

#[doc(hidden)]
#[cfg(any(feature = "gens", test))]
pub mod gens {
    use proptest::{num, prop_compose};

    use super::MessageCosts;

    prop_compose! {
        pub fn message_costs_arb()(
            first_message_cost in num::u32::ANY,
            cost_increase_per_message in num::u32::ANY,
        ) -> MessageCosts {
            MessageCosts {
                first_message_cost,
                cost_increase_per_message,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::proptest;

    use crate::bytesrepr;

    use super::gens;

    proptest! {
        #[test]
        fn should_serialize_and_deserialize_with_arbitrary_values(
            message_limits in gens::message_costs_arb()
        ) {
            bytesrepr::test_serialization_roundtrip(&message_limits);
        }
    }
}
