//! Support for Wasm opcode costs.
use std::collections::BTreeMap;

use datasize::DataSize;
use pwasm_utils::rules::{InstructionType, Metering, Set};
use rand::{distributions::Standard, prelude::*, Rng};
use serde::{Deserialize, Serialize};

use casper_types::bytesrepr::{self, FromBytes, ToBytes, U32_SERIALIZED_LENGTH};

/// Default cost of the `bit` Wasm opcode.
pub const DEFAULT_BIT_COST: u32 = 300;
/// Default cost of the `add` Wasm opcode.
pub const DEFAULT_ADD_COST: u32 = 210;
/// Default cost of the `mul` Wasm opcode.
pub const DEFAULT_MUL_COST: u32 = 240;
/// Default cost of the `div` Wasm opcode.
pub const DEFAULT_DIV_COST: u32 = 320;
/// Default cost of the `load` Wasm opcode.
pub const DEFAULT_LOAD_COST: u32 = 2_500;
/// Default cost of the `store` Wasm opcode.
pub const DEFAULT_STORE_COST: u32 = 4_700;
/// Default cost of the `const` Wasm opcode.
pub const DEFAULT_CONST_COST: u32 = 110;
/// Default cost of the `local` Wasm opcode.
pub const DEFAULT_LOCAL_COST: u32 = 390;
/// Default cost of the `global` Wasm opcode.
pub const DEFAULT_GLOBAL_COST: u32 = 390;
/// Default cost of the `control_flow` Wasm opcode.
pub const DEFAULT_CONTROL_FLOW_COST: u32 = 440000;
/// Default cost of the `integer_comparison` Wasm opcode.
pub const DEFAULT_INTEGER_COMPARISON_COST: u32 = 250;
/// Default cost of the `conversion` Wasm opcode.
pub const DEFAULT_CONVERSION_COST: u32 = 420;
/// Default cost of the `unreachable` Wasm opcode.
pub const DEFAULT_UNREACHABLE_COST: u32 = 270;
/// Default cost of the `nop` Wasm opcode.
// TODO: This value is not researched.
pub const DEFAULT_NOP_COST: u32 = 200;
/// Default cost of the `current_memory` Wasm opcode.
pub const DEFAULT_CURRENT_MEMORY_COST: u32 = 290;
/// Default cost of the `grow_memory` Wasm opcode.
pub const DEFAULT_GROW_MEMORY_COST: u32 = 240_000;
/// Default cost of the `regular` Wasm opcode.
pub const DEFAULT_REGULAR_COST: u32 = 210;

const NUM_FIELDS: usize = 17;
const OPCODE_COSTS_SERIALIZED_LENGTH: usize = NUM_FIELDS * U32_SERIALIZED_LENGTH;

/// Definition of a cost table for Wasm opcodes.
///
/// This is taken (partially) from parity-ethereum.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug, DataSize)]
pub struct OpcodeCosts {
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
    pub integer_comparison: u32,
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

impl OpcodeCosts {
    /// Creates a set of charging rules for the Wasm executor.
    pub(crate) fn to_set(self) -> Set {
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
                InstructionType::IntegerComparison,
                Metering::Fixed(self.integer_comparison),
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

            // Instructions Float, FloatComparison, FloatConst, FloatConversion are omitted here
            // because we're using `with_forbidden_floats` below.

            tmp
        };
        Set::new(self.regular, meterings)
            .with_grow_cost(self.grow_memory)
            .with_forbidden_floats()
    }
}

impl Default for OpcodeCosts {
    fn default() -> Self {
        OpcodeCosts {
            bit: DEFAULT_BIT_COST,
            add: DEFAULT_ADD_COST,
            mul: DEFAULT_MUL_COST,
            div: DEFAULT_DIV_COST,
            load: DEFAULT_LOAD_COST,
            store: DEFAULT_STORE_COST,
            op_const: DEFAULT_CONST_COST,
            local: DEFAULT_LOCAL_COST,
            global: DEFAULT_GLOBAL_COST,
            control_flow: DEFAULT_CONTROL_FLOW_COST,
            integer_comparison: DEFAULT_INTEGER_COMPARISON_COST,
            conversion: DEFAULT_CONVERSION_COST,
            unreachable: DEFAULT_UNREACHABLE_COST,
            nop: DEFAULT_NOP_COST,
            current_memory: DEFAULT_CURRENT_MEMORY_COST,
            grow_memory: DEFAULT_GROW_MEMORY_COST,
            regular: DEFAULT_REGULAR_COST,
        }
    }
}

impl Distribution<OpcodeCosts> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> OpcodeCosts {
        OpcodeCosts {
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
            integer_comparison: rng.gen(),
            conversion: rng.gen(),
            unreachable: rng.gen(),
            nop: rng.gen(),
            current_memory: rng.gen(),
            grow_memory: rng.gen(),
            regular: rng.gen(),
        }
    }
}

impl ToBytes for OpcodeCosts {
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
        ret.append(&mut self.integer_comparison.to_bytes()?);
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

impl FromBytes for OpcodeCosts {
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
        let (integer_comparison, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (conversion, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (unreachable, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (nop, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (current_memory, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (grow_memory, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (regular, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let opcode_costs = OpcodeCosts {
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
            integer_comparison,
            conversion,
            unreachable,
            nop,
            current_memory,
            grow_memory,
            regular,
        };
        Ok((opcode_costs, bytes))
    }
}

#[doc(hidden)]
#[cfg(any(feature = "gens", test))]
pub mod gens {
    use proptest::{num, prop_compose};

    use crate::shared::opcode_costs::OpcodeCosts;

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
            integer_comparison in num::u32::ANY,
            conversion in num::u32::ANY,
            unreachable in num::u32::ANY,
            nop in num::u32::ANY,
            current_memory in num::u32::ANY,
            grow_memory in num::u32::ANY,
            regular in num::u32::ANY,
        ) -> OpcodeCosts {
            OpcodeCosts {
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
                integer_comparison,
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
