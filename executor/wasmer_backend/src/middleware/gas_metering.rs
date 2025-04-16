use std::sync::Arc;

use wasmer::{wasmparser::Operator, ModuleMiddleware};
use wasmer_middlewares::Metering;

/// Calculated based on the benchmark results and fitted for approx ~1000 CSPR of computation and
/// 16s maximum computation time.
const MULTIPLIER: u64 = 16;

/// The scaling factor for the cost function is used to saturate the computation time for the
/// maximum limits allocated. Multiplier derived from benchmarks itself is not accurate enough due
/// to non-linear overhead of a gas metering on real world code. Fixed scaling factor is used
/// to adjust the multiplier to counter the effects of metering overhead. This is validated with
/// real world compute-intensive Wasm.
const SCALING_FACTOR: u64 = 2;

fn cycles(operator: &Operator) -> u64 {
    match operator {
        Operator::I32Const { .. } => 1,
        Operator::I64Const { .. } => 1,
        Operator::F32Const { .. } => 1,
        Operator::F64Const { .. } => 1,
        Operator::I32Clz => 1,
        Operator::I32Ctz => 1,
        Operator::I32Popcnt => 1,
        Operator::I64Clz => 1,
        Operator::I64Ctz => 1,
        Operator::I64Popcnt => 1,
        Operator::F32Abs => 1,
        Operator::F32Neg => 1,
        Operator::F64Abs => 2,
        Operator::F64Neg => 1,
        Operator::F32Ceil => 4,
        Operator::F32Floor => 4,
        Operator::F32Trunc => 3,
        Operator::F32Nearest => 3,
        Operator::F64Ceil => 4,
        Operator::F64Floor => 4,
        Operator::F64Trunc => 4,
        Operator::F64Nearest => 4,
        Operator::F32Sqrt => 4,
        Operator::F64Sqrt => 8,
        Operator::I32Add => 1,
        Operator::I32Sub => 1,
        Operator::I32Mul => 1,
        Operator::I32And => 1,
        Operator::I32Or => 1,
        Operator::I32Xor => 1,
        Operator::I32Shl => 1,
        Operator::I32ShrS => 1,
        Operator::I32ShrU => 1,
        Operator::I32Rotl => 1,
        Operator::I32Rotr => 1,
        Operator::I64Add => 1,
        Operator::I64Sub => 1,
        Operator::I64Mul => 1,
        Operator::I64And => 1,
        Operator::I64Or => 1,
        Operator::I64Xor => 1,
        Operator::I64Shl => 1,
        Operator::I64ShrS => 1,
        Operator::I64ShrU => 1,
        Operator::I64Rotl => 1,
        Operator::I64Rotr => 1,
        Operator::I32DivS => 18,
        Operator::I32DivU => 18,
        Operator::I32RemS => 19,
        Operator::I32RemU => 19,
        Operator::I64DivS => 19,
        Operator::I64DivU => 18,
        Operator::I64RemS => 18,
        Operator::I64RemU => 18,
        Operator::F32Add => 3,
        Operator::F32Sub => 4,
        Operator::F32Mul => 3,
        Operator::F64Add => 4,
        Operator::F64Sub => 4,
        Operator::F64Mul => 4,
        Operator::F32Div => 5,
        Operator::F64Div => 4,
        Operator::F32Min => 24,
        Operator::F32Max => 21,
        Operator::F64Min => 24,
        Operator::F64Max => 23,
        Operator::F32Copysign => 2,
        Operator::F64Copysign => 4,
        Operator::I32Eqz => 1,
        Operator::I64Eqz => 2,
        Operator::I32Eq => 1,
        Operator::I32Ne => 1,
        Operator::I32LtS => 1,
        Operator::I32LtU => 2,
        Operator::I32GtS => 1,
        Operator::I32GtU => 2,
        Operator::I32LeS => 2,
        Operator::I32LeU => 1,
        Operator::I32GeS => 1,
        Operator::I32GeU => 1,
        Operator::I64Eq => 1,
        Operator::I64Ne => 2,
        Operator::I64LtS => 1,
        Operator::I64LtU => 1,
        Operator::I64GtS => 1,
        Operator::I64GtU => 2,
        Operator::I64LeS => 1,
        Operator::I64LeU => 1,
        Operator::I64GeS => 2,
        Operator::I64GeU => 1,
        Operator::F32Eq => 2,
        Operator::F32Ne => 2,
        Operator::F64Eq => 2,
        Operator::F64Ne => 2,
        Operator::F32Lt => 2,
        Operator::F32Gt => 2,
        Operator::F32Le => 2,
        Operator::F32Ge => 2,
        Operator::F64Lt => 2,
        Operator::F64Gt => 2,
        Operator::F64Le => 2,
        Operator::F64Ge => 2,
        Operator::I32Extend8S => 1,
        Operator::I32Extend16S => 1,
        Operator::I64Extend8S => 1,
        Operator::I64Extend16S => 1,
        Operator::F32ConvertI32S => 2,
        Operator::F32ConvertI64S => 2,
        Operator::F64ConvertI32S => 2,
        Operator::F64ConvertI64S => 2,
        Operator::I64Extend32S => 1,
        Operator::I32WrapI64 => 1,
        Operator::I64ExtendI32S => 1,
        Operator::I64ExtendI32U => 1,
        Operator::F32DemoteF64 => 1,
        Operator::F64PromoteF32 => 2,
        Operator::F32ReinterpretI32 => 1,
        Operator::F64ReinterpretI64 => 1,
        Operator::F32ConvertI32U => 2,
        Operator::F64ConvertI32U => 2,
        Operator::I32ReinterpretF32 => 1,
        Operator::I64ReinterpretF64 => 1,
        Operator::I32TruncF32S => 19,
        Operator::I32TruncF32U => 17,
        Operator::I32TruncF64S => 19,
        Operator::I32TruncF64U => 18,
        Operator::I64TruncF32S => 19,
        Operator::I64TruncF32U => 21,
        Operator::I64TruncF64S => 19,
        Operator::I64TruncF64U => 23,
        Operator::I64TruncSatF32S => 19,
        Operator::I64TruncSatF64S => 19,
        Operator::I32TruncSatF32U => 19,
        Operator::I32TruncSatF64U => 18,
        Operator::I64TruncSatF32U => 20,
        Operator::I64TruncSatF64U => 22,
        Operator::I32TruncSatF32S => 18,
        Operator::I32TruncSatF64S => 19,
        Operator::F32ConvertI64U => 14,
        Operator::F64ConvertI64U => 13,
        Operator::RefFunc { .. } => 29,
        Operator::RefTestNullable { .. } => 34,
        Operator::LocalGet { .. } => 1,
        Operator::GlobalGet { .. } => 5,
        Operator::GlobalSet { .. } => 1,
        Operator::LocalTee { .. } => 1,
        Operator::TableGet { .. } => 29,
        Operator::TableSize { .. } => 25,
        Operator::I32Load { .. } => 2,
        Operator::I64Load { .. } => 2,
        Operator::F32Load { .. } => 2,
        Operator::F64Load { .. } => 2,
        Operator::I32Store { .. } => 1,
        Operator::I64Store { .. } => 1,
        Operator::F32Store { .. } => 1,
        Operator::F64Store { .. } => 1,
        Operator::I32Load8S { .. } => 2,
        Operator::I32Load8U { .. } => 2,
        Operator::I32Load16S { .. } => 2,
        Operator::I32Load16U { .. } => 2,
        Operator::I64Load8S { .. } => 2,
        Operator::I64Load8U { .. } => 2,
        Operator::I64Load16S { .. } => 2,
        Operator::I64Load16U { .. } => 2,
        Operator::I64Load32S { .. } => 2,
        Operator::I64Load32U { .. } => 2,
        Operator::I32Store8 { .. } => 1,
        Operator::I32Store16 { .. } => 1,
        Operator::I64Store8 { .. } => 1,
        Operator::I64Store16 { .. } => 1,
        Operator::I64Store32 { .. } => 1,
        Operator::MemorySize { .. } => 31,
        Operator::MemoryGrow { .. } => 67,
        Operator::MemoryCopy { .. } => 31,
        Operator::Select => 14,
        Operator::If { .. } => 1,
        Operator::Call { .. } => 17,
        Operator::Br { .. } => 12,
        Operator::BrIf { .. } => 14,
        Operator::BrTable { .. } => 34,
        Operator::CallIndirect { .. } => 23,
        Operator::Unreachable => 1,
        Operator::Nop => 1,
        Operator::Block { .. } | Operator::Loop { .. } | Operator::Else => 1,
        Operator::TryTable { .. }
        | Operator::Throw { .. }
        | Operator::ThrowRef
        | Operator::Try { .. }
        | Operator::Catch { .. }
        | Operator::Rethrow { .. }
        | Operator::Delegate { .. }
        | Operator::CatchAll => todo!("{operator:?}"),
        Operator::End
        | Operator::Return
        | Operator::ReturnCall { .. }
        | Operator::ReturnCallIndirect { .. } => 1,
        Operator::Drop => 1,
        Operator::TypedSelect { .. } => unreachable!(),
        Operator::LocalSet { .. } => 1,
        Operator::RefNull { .. }
        | Operator::RefIsNull
        | Operator::RefEq
        | Operator::StructNew { .. }
        | Operator::StructNewDefault { .. }
        | Operator::StructGet { .. }
        | Operator::StructGetS { .. }
        | Operator::StructGetU { .. }
        | Operator::StructSet { .. }
        | Operator::ArrayNew { .. }
        | Operator::ArrayNewDefault { .. }
        | Operator::ArrayNewFixed { .. }
        | Operator::ArrayNewData { .. }
        | Operator::ArrayNewElem { .. }
        | Operator::ArrayGet { .. }
        | Operator::ArrayGetS { .. }
        | Operator::ArrayGetU { .. }
        | Operator::ArraySet { .. }
        | Operator::ArrayLen
        | Operator::ArrayFill { .. }
        | Operator::ArrayCopy { .. }
        | Operator::ArrayInitData { .. }
        | Operator::ArrayInitElem { .. }
        | Operator::RefTestNonNull { .. }
        | Operator::RefCastNonNull { .. }
        | Operator::RefCastNullable { .. }
        | Operator::BrOnCast { .. }
        | Operator::BrOnCastFail { .. }
        | Operator::AnyConvertExtern
        | Operator::ExternConvertAny
        | Operator::RefI31
        | Operator::I31GetS
        | Operator::I31GetU
        | Operator::MemoryInit { .. }
        | Operator::DataDrop { .. }
        | Operator::MemoryFill { .. }
        | Operator::TableInit { .. }
        | Operator::ElemDrop { .. }
        | Operator::TableCopy { .. }
        | Operator::TableFill { .. }
        | Operator::TableSet { .. }
        | Operator::TableGrow { .. }
        | Operator::MemoryDiscard { .. }
        | Operator::MemoryAtomicNotify { .. }
        | Operator::MemoryAtomicWait32 { .. }
        | Operator::MemoryAtomicWait64 { .. }
        | Operator::AtomicFence
        | Operator::I32AtomicLoad { .. }
        | Operator::I64AtomicLoad { .. }
        | Operator::I32AtomicLoad8U { .. }
        | Operator::I32AtomicLoad16U { .. }
        | Operator::I64AtomicLoad8U { .. }
        | Operator::I64AtomicLoad16U { .. }
        | Operator::I64AtomicLoad32U { .. }
        | Operator::I32AtomicStore { .. }
        | Operator::I64AtomicStore { .. }
        | Operator::I32AtomicStore8 { .. }
        | Operator::I32AtomicStore16 { .. }
        | Operator::I64AtomicStore8 { .. }
        | Operator::I64AtomicStore16 { .. }
        | Operator::I64AtomicStore32 { .. }
        | Operator::I32AtomicRmwAdd { .. }
        | Operator::I64AtomicRmwAdd { .. }
        | Operator::I32AtomicRmw8AddU { .. }
        | Operator::I32AtomicRmw16AddU { .. }
        | Operator::I64AtomicRmw8AddU { .. }
        | Operator::I64AtomicRmw16AddU { .. }
        | Operator::I64AtomicRmw32AddU { .. }
        | Operator::I32AtomicRmwSub { .. }
        | Operator::I64AtomicRmwSub { .. }
        | Operator::I32AtomicRmw8SubU { .. }
        | Operator::I32AtomicRmw16SubU { .. }
        | Operator::I64AtomicRmw8SubU { .. }
        | Operator::I64AtomicRmw16SubU { .. }
        | Operator::I64AtomicRmw32SubU { .. }
        | Operator::I32AtomicRmwAnd { .. }
        | Operator::I64AtomicRmwAnd { .. }
        | Operator::I32AtomicRmw8AndU { .. }
        | Operator::I32AtomicRmw16AndU { .. }
        | Operator::I64AtomicRmw8AndU { .. }
        | Operator::I64AtomicRmw16AndU { .. }
        | Operator::I64AtomicRmw32AndU { .. }
        | Operator::I32AtomicRmwOr { .. }
        | Operator::I64AtomicRmwOr { .. }
        | Operator::I32AtomicRmw8OrU { .. }
        | Operator::I32AtomicRmw16OrU { .. }
        | Operator::I64AtomicRmw8OrU { .. }
        | Operator::I64AtomicRmw16OrU { .. }
        | Operator::I64AtomicRmw32OrU { .. }
        | Operator::I32AtomicRmwXor { .. }
        | Operator::I64AtomicRmwXor { .. }
        | Operator::I32AtomicRmw8XorU { .. }
        | Operator::I32AtomicRmw16XorU { .. }
        | Operator::I64AtomicRmw8XorU { .. }
        | Operator::I64AtomicRmw16XorU { .. }
        | Operator::I64AtomicRmw32XorU { .. }
        | Operator::I32AtomicRmwXchg { .. }
        | Operator::I64AtomicRmwXchg { .. }
        | Operator::I32AtomicRmw8XchgU { .. }
        | Operator::I32AtomicRmw16XchgU { .. }
        | Operator::I64AtomicRmw8XchgU { .. }
        | Operator::I64AtomicRmw16XchgU { .. }
        | Operator::I64AtomicRmw32XchgU { .. }
        | Operator::I32AtomicRmwCmpxchg { .. }
        | Operator::I64AtomicRmwCmpxchg { .. }
        | Operator::I32AtomicRmw8CmpxchgU { .. }
        | Operator::I32AtomicRmw16CmpxchgU { .. }
        | Operator::I64AtomicRmw8CmpxchgU { .. }
        | Operator::I64AtomicRmw16CmpxchgU { .. }
        | Operator::I64AtomicRmw32CmpxchgU { .. }
        | Operator::V128Load { .. }
        | Operator::V128Load8x8S { .. }
        | Operator::V128Load8x8U { .. }
        | Operator::V128Load16x4S { .. }
        | Operator::V128Load16x4U { .. }
        | Operator::V128Load32x2S { .. }
        | Operator::V128Load32x2U { .. }
        | Operator::V128Load8Splat { .. }
        | Operator::V128Load16Splat { .. }
        | Operator::V128Load32Splat { .. }
        | Operator::V128Load64Splat { .. }
        | Operator::V128Load32Zero { .. }
        | Operator::V128Load64Zero { .. }
        | Operator::V128Store { .. }
        | Operator::V128Load8Lane { .. }
        | Operator::V128Load16Lane { .. }
        | Operator::V128Load32Lane { .. }
        | Operator::V128Load64Lane { .. }
        | Operator::V128Store8Lane { .. }
        | Operator::V128Store16Lane { .. }
        | Operator::V128Store32Lane { .. }
        | Operator::V128Store64Lane { .. }
        | Operator::V128Const { .. }
        | Operator::I8x16Shuffle { .. }
        | Operator::I8x16ExtractLaneS { .. }
        | Operator::I8x16ExtractLaneU { .. }
        | Operator::I8x16ReplaceLane { .. }
        | Operator::I16x8ExtractLaneS { .. }
        | Operator::I16x8ExtractLaneU { .. }
        | Operator::I16x8ReplaceLane { .. }
        | Operator::I32x4ExtractLane { .. }
        | Operator::I32x4ReplaceLane { .. }
        | Operator::I64x2ExtractLane { .. }
        | Operator::I64x2ReplaceLane { .. }
        | Operator::F32x4ExtractLane { .. }
        | Operator::F32x4ReplaceLane { .. }
        | Operator::F64x2ExtractLane { .. }
        | Operator::F64x2ReplaceLane { .. }
        | Operator::I8x16Swizzle
        | Operator::I8x16Splat
        | Operator::I16x8Splat
        | Operator::I32x4Splat
        | Operator::I64x2Splat
        | Operator::F32x4Splat
        | Operator::F64x2Splat
        | Operator::I8x16Eq
        | Operator::I8x16Ne
        | Operator::I8x16LtS
        | Operator::I8x16LtU
        | Operator::I8x16GtS
        | Operator::I8x16GtU
        | Operator::I8x16LeS
        | Operator::I8x16LeU
        | Operator::I8x16GeS
        | Operator::I8x16GeU
        | Operator::I16x8Eq
        | Operator::I16x8Ne
        | Operator::I16x8LtS
        | Operator::I16x8LtU
        | Operator::I16x8GtS
        | Operator::I16x8GtU
        | Operator::I16x8LeS
        | Operator::I16x8LeU
        | Operator::I16x8GeS
        | Operator::I16x8GeU
        | Operator::I32x4Eq
        | Operator::I32x4Ne
        | Operator::I32x4LtS
        | Operator::I32x4LtU
        | Operator::I32x4GtS
        | Operator::I32x4GtU
        | Operator::I32x4LeS
        | Operator::I32x4LeU
        | Operator::I32x4GeS
        | Operator::I32x4GeU
        | Operator::I64x2Eq
        | Operator::I64x2Ne
        | Operator::I64x2LtS
        | Operator::I64x2GtS
        | Operator::I64x2LeS
        | Operator::I64x2GeS
        | Operator::F32x4Eq
        | Operator::F32x4Ne
        | Operator::F32x4Lt
        | Operator::F32x4Gt
        | Operator::F32x4Le
        | Operator::F32x4Ge
        | Operator::F64x2Eq
        | Operator::F64x2Ne
        | Operator::F64x2Lt
        | Operator::F64x2Gt
        | Operator::F64x2Le
        | Operator::F64x2Ge
        | Operator::V128Not
        | Operator::V128And
        | Operator::V128AndNot
        | Operator::V128Or
        | Operator::V128Xor
        | Operator::V128Bitselect
        | Operator::V128AnyTrue
        | Operator::I8x16Abs
        | Operator::I8x16Neg
        | Operator::I8x16Popcnt
        | Operator::I8x16AllTrue
        | Operator::I8x16Bitmask
        | Operator::I8x16NarrowI16x8S
        | Operator::I8x16NarrowI16x8U
        | Operator::I8x16Shl
        | Operator::I8x16ShrS
        | Operator::I8x16ShrU
        | Operator::I8x16Add
        | Operator::I8x16AddSatS
        | Operator::I8x16AddSatU
        | Operator::I8x16Sub
        | Operator::I8x16SubSatS
        | Operator::I8x16SubSatU
        | Operator::I8x16MinS
        | Operator::I8x16MinU
        | Operator::I8x16MaxS
        | Operator::I8x16MaxU
        | Operator::I8x16AvgrU
        | Operator::I16x8ExtAddPairwiseI8x16S
        | Operator::I16x8ExtAddPairwiseI8x16U
        | Operator::I16x8Abs
        | Operator::I16x8Neg
        | Operator::I16x8Q15MulrSatS
        | Operator::I16x8AllTrue
        | Operator::I16x8Bitmask
        | Operator::I16x8NarrowI32x4S
        | Operator::I16x8NarrowI32x4U
        | Operator::I16x8ExtendLowI8x16S
        | Operator::I16x8ExtendHighI8x16S
        | Operator::I16x8ExtendLowI8x16U
        | Operator::I16x8ExtendHighI8x16U
        | Operator::I16x8Shl
        | Operator::I16x8ShrS
        | Operator::I16x8ShrU
        | Operator::I16x8Add
        | Operator::I16x8AddSatS
        | Operator::I16x8AddSatU
        | Operator::I16x8Sub
        | Operator::I16x8SubSatS
        | Operator::I16x8SubSatU
        | Operator::I16x8Mul
        | Operator::I16x8MinS
        | Operator::I16x8MinU
        | Operator::I16x8MaxS
        | Operator::I16x8MaxU
        | Operator::I16x8AvgrU
        | Operator::I16x8ExtMulLowI8x16S
        | Operator::I16x8ExtMulHighI8x16S
        | Operator::I16x8ExtMulLowI8x16U
        | Operator::I16x8ExtMulHighI8x16U
        | Operator::I32x4ExtAddPairwiseI16x8S
        | Operator::I32x4ExtAddPairwiseI16x8U
        | Operator::I32x4Abs
        | Operator::I32x4Neg
        | Operator::I32x4AllTrue
        | Operator::I32x4Bitmask
        | Operator::I32x4ExtendLowI16x8S
        | Operator::I32x4ExtendHighI16x8S
        | Operator::I32x4ExtendLowI16x8U
        | Operator::I32x4ExtendHighI16x8U
        | Operator::I32x4Shl
        | Operator::I32x4ShrS
        | Operator::I32x4ShrU
        | Operator::I32x4Add
        | Operator::I32x4Sub
        | Operator::I32x4Mul
        | Operator::I32x4MinS
        | Operator::I32x4MinU
        | Operator::I32x4MaxS
        | Operator::I32x4MaxU
        | Operator::I32x4DotI16x8S
        | Operator::I32x4ExtMulLowI16x8S
        | Operator::I32x4ExtMulHighI16x8S
        | Operator::I32x4ExtMulLowI16x8U
        | Operator::I32x4ExtMulHighI16x8U
        | Operator::I64x2Abs
        | Operator::I64x2Neg
        | Operator::I64x2AllTrue
        | Operator::I64x2Bitmask
        | Operator::I64x2ExtendLowI32x4S
        | Operator::I64x2ExtendHighI32x4S
        | Operator::I64x2ExtendLowI32x4U
        | Operator::I64x2ExtendHighI32x4U
        | Operator::I64x2Shl
        | Operator::I64x2ShrS
        | Operator::I64x2ShrU
        | Operator::I64x2Add
        | Operator::I64x2Sub
        | Operator::I64x2Mul
        | Operator::I64x2ExtMulLowI32x4S
        | Operator::I64x2ExtMulHighI32x4S
        | Operator::I64x2ExtMulLowI32x4U
        | Operator::I64x2ExtMulHighI32x4U
        | Operator::F32x4Ceil
        | Operator::F32x4Floor
        | Operator::F32x4Trunc
        | Operator::F32x4Nearest
        | Operator::F32x4Abs
        | Operator::F32x4Neg
        | Operator::F32x4Sqrt
        | Operator::F32x4Add
        | Operator::F32x4Sub
        | Operator::F32x4Mul
        | Operator::F32x4Div
        | Operator::F32x4Min
        | Operator::F32x4Max
        | Operator::F32x4PMin
        | Operator::F32x4PMax
        | Operator::F64x2Ceil
        | Operator::F64x2Floor
        | Operator::F64x2Trunc
        | Operator::F64x2Nearest
        | Operator::F64x2Abs
        | Operator::F64x2Neg
        | Operator::F64x2Sqrt
        | Operator::F64x2Add
        | Operator::F64x2Sub
        | Operator::F64x2Mul
        | Operator::F64x2Div
        | Operator::F64x2Min
        | Operator::F64x2Max
        | Operator::F64x2PMin
        | Operator::F64x2PMax
        | Operator::I32x4TruncSatF32x4S
        | Operator::I32x4TruncSatF32x4U
        | Operator::F32x4ConvertI32x4S
        | Operator::F32x4ConvertI32x4U
        | Operator::I32x4TruncSatF64x2SZero
        | Operator::I32x4TruncSatF64x2UZero
        | Operator::F64x2ConvertLowI32x4S
        | Operator::F64x2ConvertLowI32x4U
        | Operator::F32x4DemoteF64x2Zero
        | Operator::F64x2PromoteLowF32x4
        | Operator::I8x16RelaxedSwizzle
        | Operator::I32x4RelaxedTruncF32x4S
        | Operator::I32x4RelaxedTruncF32x4U
        | Operator::I32x4RelaxedTruncF64x2SZero
        | Operator::I32x4RelaxedTruncF64x2UZero
        | Operator::F32x4RelaxedMadd
        | Operator::F32x4RelaxedNmadd
        | Operator::F64x2RelaxedMadd
        | Operator::F64x2RelaxedNmadd
        | Operator::I8x16RelaxedLaneselect
        | Operator::I16x8RelaxedLaneselect
        | Operator::I32x4RelaxedLaneselect
        | Operator::I64x2RelaxedLaneselect
        | Operator::F32x4RelaxedMin
        | Operator::F32x4RelaxedMax
        | Operator::F64x2RelaxedMin
        | Operator::F64x2RelaxedMax
        | Operator::I16x8RelaxedQ15mulrS
        | Operator::I16x8RelaxedDotI8x16I7x16S
        | Operator::I32x4RelaxedDotI8x16I7x16AddS
        | Operator::CallRef { .. }
        | Operator::ReturnCallRef { .. }
        | Operator::RefAsNonNull
        | Operator::BrOnNull { .. }
        | Operator::BrOnNonNull { .. }
        | Operator::GlobalAtomicGet { .. }
        | Operator::GlobalAtomicSet { .. }
        | Operator::GlobalAtomicRmwAdd { .. }
        | Operator::GlobalAtomicRmwSub { .. }
        | Operator::GlobalAtomicRmwAnd { .. }
        | Operator::GlobalAtomicRmwOr { .. }
        | Operator::GlobalAtomicRmwXor { .. }
        | Operator::GlobalAtomicRmwXchg { .. }
        | Operator::GlobalAtomicRmwCmpxchg { .. }
        | Operator::TableAtomicGet { .. }
        | Operator::TableAtomicSet { .. }
        | Operator::TableAtomicRmwXchg { .. }
        | Operator::TableAtomicRmwCmpxchg { .. }
        | Operator::StructAtomicGet { .. }
        | Operator::StructAtomicGetS { .. }
        | Operator::StructAtomicGetU { .. }
        | Operator::StructAtomicSet { .. }
        | Operator::StructAtomicRmwAdd { .. }
        | Operator::StructAtomicRmwSub { .. }
        | Operator::StructAtomicRmwAnd { .. }
        | Operator::StructAtomicRmwOr { .. }
        | Operator::StructAtomicRmwXor { .. }
        | Operator::StructAtomicRmwXchg { .. }
        | Operator::StructAtomicRmwCmpxchg { .. }
        | Operator::ArrayAtomicGet { .. }
        | Operator::ArrayAtomicGetS { .. }
        | Operator::ArrayAtomicGetU { .. }
        | Operator::ArrayAtomicSet { .. }
        | Operator::ArrayAtomicRmwAdd { .. }
        | Operator::ArrayAtomicRmwSub { .. }
        | Operator::ArrayAtomicRmwAnd { .. }
        | Operator::ArrayAtomicRmwOr { .. }
        | Operator::ArrayAtomicRmwXor { .. }
        | Operator::ArrayAtomicRmwXchg { .. }
        | Operator::ArrayAtomicRmwCmpxchg { .. }
        | Operator::RefI31Shared => todo!(),
    }
}

pub(crate) fn gas_metering_middleware(initial_limit: u64) -> Arc<dyn ModuleMiddleware> {
    Arc::new(Metering::new(initial_limit, |operator| {
        cycles(operator) * MULTIPLIER / SCALING_FACTOR
    }))
}
