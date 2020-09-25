use std::collections::BTreeMap;

use pwasm_utils::rules::{InstructionType, Metering, Set};
use rand::{distributions::Standard, prelude::*, Rng};
use serde::{Deserialize, Serialize};

use casper_types::bytesrepr::{self, FromBytes, ToBytes, U32_SERIALIZED_LENGTH};

const NUM_FIELDS: usize = 7;
pub const OPCODE_COSTS_SERIALIZED_LENGTH: usize = NUM_FIELDS * U32_SERIALIZED_LENGTH;

// Taken (partially) from parity-ethereum
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct OpCodeCosts {
    /// Default opcode cost
    pub regular: u32,
    /// Div operations multiplier.
    pub div: u32,
    /// Mul operations multiplier.
    pub mul: u32,
    /// Memory (load/store) operations multiplier.
    pub mem: u32,
    /// Grow memory cost, per page (64kb)
    pub grow_mem: u32,
    /// Cost of wasm opcode is calculated as TABLE_ENTRY_COST * `opcodes_mul` /
    /// `opcodes_div`
    pub opcodes_mul: u32,
    /// Cost of wasm opcode is calculated as TABLE_ENTRY_COST * `opcodes_mul` /
    /// `opcodes_div`
    pub opcodes_div: u32,
}

impl OpCodeCosts {
    pub(crate) fn to_set(&self) -> Set {
        let meterings = {
            let mut tmp = BTreeMap::new();
            tmp.insert(InstructionType::Load, Metering::Fixed(self.mem));
            tmp.insert(InstructionType::Store, Metering::Fixed(self.mem));
            tmp.insert(InstructionType::Div, Metering::Fixed(self.div));
            tmp.insert(InstructionType::Mul, Metering::Fixed(self.mul));
            tmp
        };
        Set::new(self.regular, meterings)
            .with_grow_cost(self.grow_mem)
            .with_forbidden_floats()
    }
}

impl Default for OpCodeCosts {
    fn default() -> Self {
        OpCodeCosts {
            regular: 1,
            div: 16,
            mul: 4,
            mem: 2,
            grow_mem: 8192,
            opcodes_mul: 3,
            opcodes_div: 8,
        }
    }
}

impl Distribution<OpCodeCosts> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> OpCodeCosts {
        OpCodeCosts {
            regular: rng.gen(),
            div: rng.gen(),
            mul: rng.gen(),
            mem: rng.gen(),
            grow_mem: rng.gen(),
            opcodes_mul: rng.gen(),
            opcodes_div: rng.gen(),
        }
    }
}

impl ToBytes for OpCodeCosts {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);
        ret.append(&mut self.regular.to_bytes()?);
        ret.append(&mut self.div.to_bytes()?);
        ret.append(&mut self.mul.to_bytes()?);
        ret.append(&mut self.mem.to_bytes()?);
        ret.append(&mut self.grow_mem.to_bytes()?);
        ret.append(&mut self.opcodes_mul.to_bytes()?);
        ret.append(&mut self.opcodes_div.to_bytes()?);
        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        OPCODE_COSTS_SERIALIZED_LENGTH
    }
}

impl FromBytes for OpCodeCosts {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (regular, rem): (u32, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (div, rem): (u32, &[u8]) = FromBytes::from_bytes(rem)?;
        let (mul, rem): (u32, &[u8]) = FromBytes::from_bytes(rem)?;
        let (mem, rem): (u32, &[u8]) = FromBytes::from_bytes(rem)?;
        let (grow_mem, rem): (u32, &[u8]) = FromBytes::from_bytes(rem)?;
        let (opcodes_mul, rem): (u32, &[u8]) = FromBytes::from_bytes(rem)?;
        let (opcodes_div, rem): (u32, &[u8]) = FromBytes::from_bytes(rem)?;
        let wasm_costs = OpCodeCosts {
            regular,
            div,
            mul,
            mem,
            grow_mem,
            opcodes_mul,
            opcodes_div,
        };
        Ok((wasm_costs, rem))
    }
}

#[cfg(any(feature = "gens", test))]
pub mod gens {
    use proptest::{num, prop_compose};

    use crate::shared::opcode_costs::OpCodeCosts;

    prop_compose! {
        pub fn opcode_costs_arb()(
            regular in num::u32::ANY,
            div in num::u32::ANY,
            mul in num::u32::ANY,
            mem in num::u32::ANY,
            grow_mem in num::u32::ANY,
            opcodes_mul in num::u32::ANY,
            opcodes_div in num::u32::ANY,
        ) -> OpCodeCosts {
            OpCodeCosts {
                regular,
                div,
                mul,
                mem,
                grow_mem,
                opcodes_mul,
                opcodes_div,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::proptest;

    use casper_types::bytesrepr;

    use super::gens;

    proptest! {
        #[test]
        fn should_serialize_and_deserialize_with_arbitrary_values(
            opcode_costs in gens::opcode_costs_arb()
        ) {
            bytesrepr::test_serialization_roundtrip(&opcode_costs);
        }
    }
}
