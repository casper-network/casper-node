use casper_types::bytesrepr::{self, FromBytes};

use crate::shared::opcode_costs::OpcodeCosts;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LegacyOpcodeCosts(OpcodeCosts);

impl From<LegacyOpcodeCosts> for OpcodeCosts {
    fn from(legacy_opcode_costs: LegacyOpcodeCosts) -> Self {
        legacy_opcode_costs.0
    }
}

impl FromBytes for LegacyOpcodeCosts {
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
        Ok((LegacyOpcodeCosts(opcode_costs), bytes))
    }
}
