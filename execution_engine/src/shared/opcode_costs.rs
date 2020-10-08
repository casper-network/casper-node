use std::collections::BTreeMap;

use datasize::DataSize;
use pwasm_utils::rules::{InstructionType, Metering, Set};
use rand::{distributions::Standard, prelude::*, Rng};
use serde::{Deserialize, Serialize};

use casper_types::bytesrepr::{self, FromBytes, ToBytes, U32_SERIALIZED_LENGTH};

const NUM_FIELDS: usize = 17;
pub const OPCODE_COSTS_SERIALIZED_LENGTH: usize = NUM_FIELDS * U32_SERIALIZED_LENGTH;

// Taken (partially) from parity-ethereum
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug, DataSize)]
pub struct OpCodeCosts {
    /// Bit operations multiplier.
    pub bit: u32,
    /// Arithmetic add operations multiplier.
    pub add: u32,
    /// Mul operations multiplier.
    pub mul: u32,
    /// Div operations multiplier.
    pub div: u32,
    /// Memory load operation multiplier.
    pub load: u32,
    /// Memory store operation multiplier.
    pub store: u32,
    /// Const operation multiplier.
    #[serde(rename = "const")]
    pub op_const: u32,
    /// Local operations multiplier.
    pub local: u32,
    /// Global operations multiplier.
    pub global: u32,
    /// Control flow operations multiplier.
    pub control_flow: u32,
    /// Integer operations multiplier.
    pub integer_comparsion: u32,
    /// Conversion operations multiplier.
    pub conversion: u32,
    /// Unreachable operation multiplier.
    pub unreachable: u32,
    /// Nop operation multiplier.
    pub nop: u32,
    /// Get current memory operation multiplier.
    pub current_memory: u32,
    /// Grow memory cost, per page (64kb)
    pub grow_memory: u32,
    /// Regular opcode cost
    pub regular: u32,
}

impl OpCodeCosts {
    pub(crate) fn to_set(&self) -> Set {
        let meterings = {
            let mut tmp = BTreeMap::new();
            tmp.insert(InstructionType::Bit, Metering::Fixed(self.bit));
            tmp.insert(InstructionType::Add, Metering::Fixed(self.add));
            tmp.insert(InstructionType::Mul, Metering::Fixed(self.mul));
            tmp.insert(InstructionType::Div, Metering::Fixed(self.div));
            tmp.insert(InstructionType::Load, Metering::Fixed(self.load));
            tmp.insert(InstructionType::Store, Metering::Fixed(self.store));
            tmp.insert(InstructionType::Const, Metering::Fixed(self.op_const));
            tmp.insert(InstructionType::Local, Metering::Fixed(self.local));
            tmp.insert(InstructionType::Global, Metering::Fixed(self.global));
            tmp.insert(
                InstructionType::ControlFlow,
                Metering::Fixed(self.control_flow),
            );
            tmp.insert(
                InstructionType::IntegerComparsion,
                Metering::Fixed(self.integer_comparsion),
            );
            tmp.insert(
                InstructionType::Conversion,
                Metering::Fixed(self.conversion),
            );
            tmp.insert(
                InstructionType::Unreachable,
                Metering::Fixed(self.unreachable),
            );
            tmp.insert(InstructionType::Nop, Metering::Fixed(self.nop));
            tmp.insert(
                InstructionType::CurrentMemory,
                Metering::Fixed(self.current_memory),
            );
            tmp.insert(
                InstructionType::GrowMemory,
                Metering::Fixed(self.grow_memory),
            );

            // Instructions Float, FloatComparsion, FloatConst, FloatConversion are omitted here
            // because we're using `with_forbidden_floats` below.

            tmp
        };
        Set::new(self.regular, meterings)
            .with_grow_cost(self.grow_memory)
            .with_forbidden_floats()
    }
}

impl Default for OpCodeCosts {
    fn default() -> Self {
        OpCodeCosts {
            bit: 1,
            add: 1,
            mul: 4,
            div: 16,
            load: 2,
            store: 2,
            op_const: 2,
            local: 2,
            global: 2,
            control_flow: 2,
            integer_comparsion: 2,
            conversion: 2,
            unreachable: 2,
            nop: 0,
            current_memory: 2,
            grow_memory: 8192,
            regular: 2,
        }
    }
}

impl Distribution<OpCodeCosts> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> OpCodeCosts {
        OpCodeCosts {
            bit: rng.gen(),
            add: rng.gen(),
            mul: rng.gen(),
            div: rng.gen(),
            load: rng.gen(),
            store: rng.gen(),
            op_const: rng.gen(),
            local: rng.gen(),
            global: rng.gen(),
            control_flow: rng.gen(),
            integer_comparsion: rng.gen(),
            conversion: rng.gen(),
            unreachable: rng.gen(),
            nop: rng.gen(),
            current_memory: rng.gen(),
            grow_memory: rng.gen(),
            regular: rng.gen(),
        }
    }
}

impl ToBytes for OpCodeCosts {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);

        ret.append(&mut self.bit.to_bytes()?);
        ret.append(&mut self.add.to_bytes()?);
        ret.append(&mut self.mul.to_bytes()?);
        ret.append(&mut self.div.to_bytes()?);
        ret.append(&mut self.load.to_bytes()?);
        ret.append(&mut self.store.to_bytes()?);
        ret.append(&mut self.op_const.to_bytes()?);
        ret.append(&mut self.local.to_bytes()?);
        ret.append(&mut self.global.to_bytes()?);
        ret.append(&mut self.control_flow.to_bytes()?);
        ret.append(&mut self.integer_comparsion.to_bytes()?);
        ret.append(&mut self.conversion.to_bytes()?);
        ret.append(&mut self.unreachable.to_bytes()?);
        ret.append(&mut self.nop.to_bytes()?);
        ret.append(&mut self.current_memory.to_bytes()?);
        ret.append(&mut self.grow_memory.to_bytes()?);
        ret.append(&mut self.regular.to_bytes()?);

        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        OPCODE_COSTS_SERIALIZED_LENGTH
    }
}

impl FromBytes for OpCodeCosts {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (bit, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (add, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (mul, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (div, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (load, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (store, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (const_, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (local, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (global, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (control_flow, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (integer_comparsion, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (conversion, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (unreachable, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (nop, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (current_memory, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (grow_memory, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (regular, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let wasm_costs = OpCodeCosts {
            bit,
            add,
            mul,
            div,
            load,
            store,
            op_const: const_,
            local,
            global,
            control_flow,
            integer_comparsion,
            conversion,
            unreachable,
            nop,
            current_memory,
            grow_memory,
            regular,
        };
        Ok((wasm_costs, bytes))
    }
}

#[cfg(any(feature = "gens", test))]
pub mod gens {
    use proptest::{num, prop_compose};

    use crate::shared::opcode_costs::OpCodeCosts;

    prop_compose! {
        pub fn opcode_costs_arb()(
            bit in num::u32::ANY,
            add in num::u32::ANY,
            mul in num::u32::ANY,
            div in num::u32::ANY,
            load in num::u32::ANY,
            store in num::u32::ANY,
            op_const in num::u32::ANY,
            local in num::u32::ANY,
            global in num::u32::ANY,
            control_flow in num::u32::ANY,
            integer_comparsion in num::u32::ANY,
            conversion in num::u32::ANY,
            unreachable in num::u32::ANY,
            nop in num::u32::ANY,
            current_memory in num::u32::ANY,
            grow_memory in num::u32::ANY,
            regular in num::u32::ANY,
        ) -> OpCodeCosts {
            OpCodeCosts {
                bit,
                add,
                mul,
                div,
                load,
                store,
                op_const,
                local,
                global,
                control_flow,
                integer_comparsion,
                conversion,
                unreachable,
                nop,
                current_memory,
                grow_memory,
                regular,
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
