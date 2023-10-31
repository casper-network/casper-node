//! Support for Wasm opcode costs.
use std::{convert::TryInto, num::NonZeroU32};

use casper_wasm::elements::Instruction;
use casper_wasm_utils::rules::{MemoryGrowCost, Rules};
use datasize::DataSize;
use rand::{distributions::Standard, prelude::*, Rng};
use serde::{Deserialize, Serialize};

use casper_types::bytesrepr::{self, FromBytes, ToBytes};

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
/// Default cost of the `block` Wasm opcode.
pub const DEFAULT_CONTROL_FLOW_BLOCK_OPCODE: u32 = 440;
/// Default cost of the `loop` Wasm opcode.
pub const DEFAULT_CONTROL_FLOW_LOOP_OPCODE: u32 = 440;
/// Default cost of the `if` Wasm opcode.
pub const DEFAULT_CONTROL_FLOW_IF_OPCODE: u32 = 440;
/// Default cost of the `else` Wasm opcode.
pub const DEFAULT_CONTROL_FLOW_ELSE_OPCODE: u32 = 440;
/// Default cost of the `end` Wasm opcode.
pub const DEFAULT_CONTROL_FLOW_END_OPCODE: u32 = 440;
/// Default cost of the `br` Wasm opcode.
pub const DEFAULT_CONTROL_FLOW_BR_OPCODE: u32 = 35_000;
/// Default cost of the `br_if` Wasm opcode.
pub const DEFAULT_CONTROL_FLOW_BR_IF_OPCODE: u32 = 35_000;
/// Default cost of the `return` Wasm opcode.
pub const DEFAULT_CONTROL_FLOW_RETURN_OPCODE: u32 = 440;
/// Default cost of the `select` Wasm opcode.
pub const DEFAULT_CONTROL_FLOW_SELECT_OPCODE: u32 = 440;
/// Default cost of the `call` Wasm opcode.
pub const DEFAULT_CONTROL_FLOW_CALL_OPCODE: u32 = 68_000;
/// Default cost of the `call_indirect` Wasm opcode.
pub const DEFAULT_CONTROL_FLOW_CALL_INDIRECT_OPCODE: u32 = 68_000;
/// Default cost of the `drop` Wasm opcode.
pub const DEFAULT_CONTROL_FLOW_DROP_OPCODE: u32 = 440;
/// Default fixed cost of the `br_table` Wasm opcode.
pub const DEFAULT_CONTROL_FLOW_BR_TABLE_OPCODE: u32 = 35_000;
/// Default multiplier for the size of targets in `br_table` Wasm opcode.
pub const DEFAULT_CONTROL_FLOW_BR_TABLE_MULTIPLIER: u32 = 100;

/// Definition of a cost table for a Wasm `br_table` opcode.
///
/// Charge of a `br_table` opcode is calculated as follows:
///
/// ```text
/// cost + (len(br_table.targets) * size_multiplier)
/// ```
// This is done to encourage users to avoid writing code with very long `br_table`s.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug, DataSize)]
#[serde(deny_unknown_fields)]
pub struct BrTableCost {
    /// Fixed cost charge for `br_table` opcode.
    pub cost: u32,
    /// Multiplier for size of target labels in the `br_table` opcode.
    pub size_multiplier: u32,
}

impl Default for BrTableCost {
    fn default() -> Self {
        Self {
            cost: DEFAULT_CONTROL_FLOW_BR_TABLE_OPCODE,
            size_multiplier: DEFAULT_CONTROL_FLOW_BR_TABLE_MULTIPLIER,
        }
    }
}

impl Distribution<BrTableCost> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> BrTableCost {
        BrTableCost {
            cost: rng.gen(),
            size_multiplier: rng.gen(),
        }
    }
}

impl ToBytes for BrTableCost {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let Self {
            cost,
            size_multiplier,
        } = self;

        let mut ret = bytesrepr::unchecked_allocate_buffer(self);

        ret.append(&mut cost.to_bytes()?);
        ret.append(&mut size_multiplier.to_bytes()?);

        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        let Self {
            cost,
            size_multiplier,
        } = self;

        cost.serialized_length() + size_multiplier.serialized_length()
    }
}

impl FromBytes for BrTableCost {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (cost, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (size_multiplier, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        Ok((
            Self {
                cost,
                size_multiplier,
            },
            bytes,
        ))
    }
}

/// Definition of a cost table for a Wasm control flow opcodes.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug, DataSize)]
#[serde(deny_unknown_fields)]
pub struct ControlFlowCosts {
    /// Cost for `block` opcode.
    pub block: u32,
    /// Cost for `loop` opcode.
    #[serde(rename = "loop")]
    pub op_loop: u32,
    /// Cost for `if` opcode.
    #[serde(rename = "if")]
    pub op_if: u32,
    /// Cost for `else` opcode.
    #[serde(rename = "else")]
    pub op_else: u32,
    /// Cost for `end` opcode.
    pub end: u32,
    /// Cost for `br` opcode.
    pub br: u32,
    /// Cost for `br_if` opcode.
    pub br_if: u32,
    /// Cost for `return` opcode.
    #[serde(rename = "return")]
    pub op_return: u32,
    /// Cost for `call` opcode.
    pub call: u32,
    /// Cost for `call_indirect` opcode.
    pub call_indirect: u32,
    /// Cost for `drop` opcode.
    pub drop: u32,
    /// Cost for `select` opcode.
    pub select: u32,
    /// Cost for `br_table` opcode.
    pub br_table: BrTableCost,
}

impl Default for ControlFlowCosts {
    fn default() -> Self {
        Self {
            block: DEFAULT_CONTROL_FLOW_BLOCK_OPCODE,
            op_loop: DEFAULT_CONTROL_FLOW_LOOP_OPCODE,
            op_if: DEFAULT_CONTROL_FLOW_IF_OPCODE,
            op_else: DEFAULT_CONTROL_FLOW_ELSE_OPCODE,
            end: DEFAULT_CONTROL_FLOW_END_OPCODE,
            br: DEFAULT_CONTROL_FLOW_BR_OPCODE,
            br_if: DEFAULT_CONTROL_FLOW_BR_IF_OPCODE,
            op_return: DEFAULT_CONTROL_FLOW_RETURN_OPCODE,
            call: DEFAULT_CONTROL_FLOW_CALL_OPCODE,
            call_indirect: DEFAULT_CONTROL_FLOW_CALL_INDIRECT_OPCODE,
            drop: DEFAULT_CONTROL_FLOW_DROP_OPCODE,
            select: DEFAULT_CONTROL_FLOW_SELECT_OPCODE,
            br_table: Default::default(),
        }
    }
}

impl ToBytes for ControlFlowCosts {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);

        let Self {
            block,
            op_loop,
            op_if,
            op_else,
            end,
            br,
            br_if,
            op_return,
            call,
            call_indirect,
            drop,
            select,
            br_table,
        } = self;
        ret.append(&mut block.to_bytes()?);
        ret.append(&mut op_loop.to_bytes()?);
        ret.append(&mut op_if.to_bytes()?);
        ret.append(&mut op_else.to_bytes()?);
        ret.append(&mut end.to_bytes()?);
        ret.append(&mut br.to_bytes()?);
        ret.append(&mut br_if.to_bytes()?);
        ret.append(&mut op_return.to_bytes()?);
        ret.append(&mut call.to_bytes()?);
        ret.append(&mut call_indirect.to_bytes()?);
        ret.append(&mut drop.to_bytes()?);
        ret.append(&mut select.to_bytes()?);
        ret.append(&mut br_table.to_bytes()?);

        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        let Self {
            block,
            op_loop,
            op_if,
            op_else,
            end,
            br,
            br_if,
            op_return,
            call,
            call_indirect,
            drop,
            select,
            br_table,
        } = self;
        block.serialized_length()
            + op_loop.serialized_length()
            + op_if.serialized_length()
            + op_else.serialized_length()
            + end.serialized_length()
            + br.serialized_length()
            + br_if.serialized_length()
            + op_return.serialized_length()
            + call.serialized_length()
            + call_indirect.serialized_length()
            + drop.serialized_length()
            + select.serialized_length()
            + br_table.serialized_length()
    }
}

impl FromBytes for ControlFlowCosts {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (block, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (op_loop, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (op_if, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (op_else, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (end, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (br, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (br_if, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (op_return, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (call, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (call_indirect, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (drop, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (select, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (br_table, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;

        let control_flow_cost = ControlFlowCosts {
            block,
            op_loop,
            op_if,
            op_else,
            end,
            br,
            br_if,
            op_return,
            call,
            call_indirect,
            drop,
            select,
            br_table,
        };
        Ok((control_flow_cost, bytes))
    }
}

impl Distribution<ControlFlowCosts> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> ControlFlowCosts {
        ControlFlowCosts {
            block: rng.gen(),
            op_loop: rng.gen(),
            op_if: rng.gen(),
            op_else: rng.gen(),
            end: rng.gen(),
            br: rng.gen(),
            br_if: rng.gen(),
            op_return: rng.gen(),
            call: rng.gen(),
            call_indirect: rng.gen(),
            drop: rng.gen(),
            select: rng.gen(),
            br_table: rng.gen(),
        }
    }
}

/// Definition of a cost table for Wasm opcodes.
///
/// This is taken (partially) from parity-ethereum.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug, DataSize)]
#[serde(deny_unknown_fields)]
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
    /// Control flow operations multiplier.
    pub control_flow: ControlFlowCosts,
}

impl Rules for OpcodeCosts {
    fn instruction_cost(&self, instruction: &Instruction) -> Option<u32> {
        match instruction {
            Instruction::Unreachable => Some(self.unreachable),
            Instruction::Nop => Some(self.nop),

            // Control flow class of opcodes is charged for each of the opcode individually.
            Instruction::Block(_) => Some(self.control_flow.block),
            Instruction::Loop(_) => Some(self.control_flow.op_loop),
            Instruction::If(_) => Some(self.control_flow.op_if),
            Instruction::Else => Some(self.control_flow.op_else),
            Instruction::End => Some(self.control_flow.end),
            Instruction::Br(_) => Some(self.control_flow.br),
            Instruction::BrIf(_) => Some(self.control_flow.br_if),
            Instruction::BrTable(br_table_data) => {
                // If we're unable to fit table size in `u32` to measure the cost, then such wasm
                // would be rejected. This is unlikely scenario as we impose a limit
                // for the amount of targets a `br_table` opcode can contain.
                let br_table_size: u32 = br_table_data.table.len().try_into().ok()?;

                let br_table_cost = self.control_flow.br_table.cost;

                let table_size_part =
                    br_table_size.checked_mul(self.control_flow.br_table.size_multiplier)?;

                let br_table_cost = br_table_cost.checked_add(table_size_part)?;
                Some(br_table_cost)
            }
            Instruction::Return => Some(self.control_flow.op_return),
            Instruction::Call(_) => Some(self.control_flow.call),
            Instruction::CallIndirect(_, _) => Some(self.control_flow.call_indirect),
            Instruction::Drop => Some(self.control_flow.drop),
            Instruction::Select => Some(self.control_flow.select),

            Instruction::GetLocal(_) | Instruction::SetLocal(_) | Instruction::TeeLocal(_) => {
                Some(self.local)
            }
            Instruction::GetGlobal(_) | Instruction::SetGlobal(_) => Some(self.global),

            Instruction::I32Load(_, _)
            | Instruction::I64Load(_, _)
            | Instruction::F32Load(_, _)
            | Instruction::F64Load(_, _)
            | Instruction::I32Load8S(_, _)
            | Instruction::I32Load8U(_, _)
            | Instruction::I32Load16S(_, _)
            | Instruction::I32Load16U(_, _)
            | Instruction::I64Load8S(_, _)
            | Instruction::I64Load8U(_, _)
            | Instruction::I64Load16S(_, _)
            | Instruction::I64Load16U(_, _)
            | Instruction::I64Load32S(_, _)
            | Instruction::I64Load32U(_, _) => Some(self.load),

            Instruction::I32Store(_, _)
            | Instruction::I64Store(_, _)
            | Instruction::F32Store(_, _)
            | Instruction::F64Store(_, _)
            | Instruction::I32Store8(_, _)
            | Instruction::I32Store16(_, _)
            | Instruction::I64Store8(_, _)
            | Instruction::I64Store16(_, _)
            | Instruction::I64Store32(_, _) => Some(self.store),

            Instruction::CurrentMemory(_) => Some(self.current_memory),
            Instruction::GrowMemory(_) => Some(self.grow_memory),

            Instruction::I32Const(_) | Instruction::I64Const(_) => Some(self.op_const),

            Instruction::F32Const(_) | Instruction::F64Const(_) => None, // float_const

            Instruction::I32Eqz
            | Instruction::I32Eq
            | Instruction::I32Ne
            | Instruction::I32LtS
            | Instruction::I32LtU
            | Instruction::I32GtS
            | Instruction::I32GtU
            | Instruction::I32LeS
            | Instruction::I32LeU
            | Instruction::I32GeS
            | Instruction::I32GeU
            | Instruction::I64Eqz
            | Instruction::I64Eq
            | Instruction::I64Ne
            | Instruction::I64LtS
            | Instruction::I64LtU
            | Instruction::I64GtS
            | Instruction::I64GtU
            | Instruction::I64LeS
            | Instruction::I64LeU
            | Instruction::I64GeS
            | Instruction::I64GeU => Some(self.integer_comparison),

            Instruction::F32Eq
            | Instruction::F32Ne
            | Instruction::F32Lt
            | Instruction::F32Gt
            | Instruction::F32Le
            | Instruction::F32Ge
            | Instruction::F64Eq
            | Instruction::F64Ne
            | Instruction::F64Lt
            | Instruction::F64Gt
            | Instruction::F64Le
            | Instruction::F64Ge => None, // Unsupported comparison operators for floats.

            Instruction::I32Clz | Instruction::I32Ctz | Instruction::I32Popcnt => Some(self.bit),

            Instruction::I32Add | Instruction::I32Sub => Some(self.add),

            Instruction::I32Mul => Some(self.mul),

            Instruction::I32DivS
            | Instruction::I32DivU
            | Instruction::I32RemS
            | Instruction::I32RemU => Some(self.div),

            Instruction::I32And
            | Instruction::I32Or
            | Instruction::I32Xor
            | Instruction::I32Shl
            | Instruction::I32ShrS
            | Instruction::I32ShrU
            | Instruction::I32Rotl
            | Instruction::I32Rotr
            | Instruction::I64Clz
            | Instruction::I64Ctz
            | Instruction::I64Popcnt => Some(self.bit),

            Instruction::I64Add | Instruction::I64Sub => Some(self.add),
            Instruction::I64Mul => Some(self.mul),

            Instruction::I64DivS
            | Instruction::I64DivU
            | Instruction::I64RemS
            | Instruction::I64RemU => Some(self.div),

            Instruction::I64And
            | Instruction::I64Or
            | Instruction::I64Xor
            | Instruction::I64Shl
            | Instruction::I64ShrS
            | Instruction::I64ShrU
            | Instruction::I64Rotl
            | Instruction::I64Rotr => Some(self.bit),

            Instruction::F32Abs
            | Instruction::F32Neg
            | Instruction::F32Ceil
            | Instruction::F32Floor
            | Instruction::F32Trunc
            | Instruction::F32Nearest
            | Instruction::F32Sqrt
            | Instruction::F32Add
            | Instruction::F32Sub
            | Instruction::F32Mul
            | Instruction::F32Div
            | Instruction::F32Min
            | Instruction::F32Max
            | Instruction::F32Copysign
            | Instruction::F64Abs
            | Instruction::F64Neg
            | Instruction::F64Ceil
            | Instruction::F64Floor
            | Instruction::F64Trunc
            | Instruction::F64Nearest
            | Instruction::F64Sqrt
            | Instruction::F64Add
            | Instruction::F64Sub
            | Instruction::F64Mul
            | Instruction::F64Div
            | Instruction::F64Min
            | Instruction::F64Max
            | Instruction::F64Copysign => None, // Unsupported math operators for floats.

            Instruction::I32WrapI64 | Instruction::I64ExtendSI32 | Instruction::I64ExtendUI32 => {
                Some(self.conversion)
            }

            Instruction::I32TruncSF32
            | Instruction::I32TruncUF32
            | Instruction::I32TruncSF64
            | Instruction::I32TruncUF64
            | Instruction::I64TruncSF32
            | Instruction::I64TruncUF32
            | Instruction::I64TruncSF64
            | Instruction::I64TruncUF64
            | Instruction::F32ConvertSI32
            | Instruction::F32ConvertUI32
            | Instruction::F32ConvertSI64
            | Instruction::F32ConvertUI64
            | Instruction::F32DemoteF64
            | Instruction::F64ConvertSI32
            | Instruction::F64ConvertUI32
            | Instruction::F64ConvertSI64
            | Instruction::F64ConvertUI64
            | Instruction::F64PromoteF32 => None, // Unsupported conversion operators for floats.

            Instruction::I32ReinterpretF32
            | Instruction::I64ReinterpretF64
            | Instruction::F32ReinterpretI32
            | Instruction::F64ReinterpretI64 => None, /* Unsupported reinterpretation operators
                                                       * for floats. */
        }
    }

    fn memory_grow_cost(&self) -> Option<MemoryGrowCost> {
        NonZeroU32::new(self.grow_memory).map(MemoryGrowCost::Linear)
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
            integer_comparison: DEFAULT_INTEGER_COMPARISON_COST,
            conversion: DEFAULT_CONVERSION_COST,
            unreachable: DEFAULT_UNREACHABLE_COST,
            nop: DEFAULT_NOP_COST,
            current_memory: DEFAULT_CURRENT_MEMORY_COST,
            grow_memory: DEFAULT_GROW_MEMORY_COST,
            control_flow: ControlFlowCosts::default(),
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
            integer_comparison: rng.gen(),
            conversion: rng.gen(),
            unreachable: rng.gen(),
            nop: rng.gen(),
            current_memory: rng.gen(),
            grow_memory: rng.gen(),
            control_flow: rng.gen(),
        }
    }
}

impl ToBytes for OpcodeCosts {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);

        let Self {
            bit,
            add,
            mul,
            div,
            load,
            store,
            op_const,
            local,
            global,
            integer_comparison,
            conversion,
            unreachable,
            nop,
            current_memory,
            grow_memory,
            control_flow,
        } = self;

        ret.append(&mut bit.to_bytes()?);
        ret.append(&mut add.to_bytes()?);
        ret.append(&mut mul.to_bytes()?);
        ret.append(&mut div.to_bytes()?);
        ret.append(&mut load.to_bytes()?);
        ret.append(&mut store.to_bytes()?);
        ret.append(&mut op_const.to_bytes()?);
        ret.append(&mut local.to_bytes()?);
        ret.append(&mut global.to_bytes()?);
        ret.append(&mut integer_comparison.to_bytes()?);
        ret.append(&mut conversion.to_bytes()?);
        ret.append(&mut unreachable.to_bytes()?);
        ret.append(&mut nop.to_bytes()?);
        ret.append(&mut current_memory.to_bytes()?);
        ret.append(&mut grow_memory.to_bytes()?);
        ret.append(&mut control_flow.to_bytes()?);

        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        let Self {
            bit,
            add,
            mul,
            div,
            load,
            store,
            op_const,
            local,
            global,
            integer_comparison,
            conversion,
            unreachable,
            nop,
            current_memory,
            grow_memory,
            control_flow,
        } = self;
        bit.serialized_length()
            + add.serialized_length()
            + mul.serialized_length()
            + div.serialized_length()
            + load.serialized_length()
            + store.serialized_length()
            + op_const.serialized_length()
            + local.serialized_length()
            + global.serialized_length()
            + integer_comparison.serialized_length()
            + conversion.serialized_length()
            + unreachable.serialized_length()
            + nop.serialized_length()
            + current_memory.serialized_length()
            + grow_memory.serialized_length()
            + control_flow.serialized_length()
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
        let (integer_comparison, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (conversion, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (unreachable, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (nop, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (current_memory, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (grow_memory, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;
        let (control_flow, bytes): (_, &[u8]) = FromBytes::from_bytes(bytes)?;

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
            integer_comparison,
            conversion,
            unreachable,
            nop,
            current_memory,
            grow_memory,
            control_flow,
        };
        Ok((opcode_costs, bytes))
    }
}

#[doc(hidden)]
#[cfg(any(feature = "gens", test))]
pub mod gens {
    use proptest::{num, prop_compose};

    use crate::shared::opcode_costs::OpcodeCosts;

    use super::{BrTableCost, ControlFlowCosts};

    prop_compose! {
        pub fn br_table_cost_arb()(
            cost in num::u32::ANY,
            size_multiplier in num::u32::ANY,
        ) -> BrTableCost {
            BrTableCost { cost, size_multiplier }
        }
    }

    prop_compose! {
        pub fn control_flow_cost_arb()(
            block in num::u32::ANY,
            op_loop in num::u32::ANY,
            op_if in num::u32::ANY,
            op_else in num::u32::ANY,
            end in num::u32::ANY,
            br in num::u32::ANY,
            br_if in num::u32::ANY,
            br_table in br_table_cost_arb(),
            op_return in num::u32::ANY,
            call in num::u32::ANY,
            call_indirect in num::u32::ANY,
            drop in num::u32::ANY,
            select in num::u32::ANY,
        ) -> ControlFlowCosts {
            ControlFlowCosts {
                block,
                op_loop,
                op_if,
                op_else,
                end,
                br,
                br_if,
                br_table,
                op_return,
                call,
                call_indirect,
                drop,
                select
            }
        }

    }

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
            integer_comparison in num::u32::ANY,
            conversion in num::u32::ANY,
            unreachable in num::u32::ANY,
            nop in num::u32::ANY,
            current_memory in num::u32::ANY,
            grow_memory in num::u32::ANY,
            control_flow in control_flow_cost_arb(),
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
                integer_comparison,
                conversion,
                unreachable,
                nop,
                current_memory,
                grow_memory,
                control_flow,
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
