use node::contract_core::engine_state::{execution_effect::ExecutionEffect, op::Op};
use types::Key;

use crate::engine_server::{
    ipc::{self, AddOp, NoOp, OpEntry, ReadOp, WriteOp},
    transforms::TransformEntry as ProbufTransformEntry,
};

impl From<(Key, Op)> for OpEntry {
    fn from((key, op): (Key, Op)) -> OpEntry {
        let mut pb_op_entry = OpEntry::new();

        pb_op_entry.set_key(key.into());

        match op {
            Op::Read => pb_op_entry.mut_operation().set_read(ReadOp::new()),
            Op::Write => pb_op_entry.mut_operation().set_write(WriteOp::new()),
            Op::Add => pb_op_entry.mut_operation().set_add(AddOp::new()),
            Op::NoOp => pb_op_entry.mut_operation().set_noop(NoOp::new()),
        };

        pb_op_entry
    }
}

impl From<ExecutionEffect> for ipc::ExecutionEffect {
    fn from(execution_effect: ExecutionEffect) -> ipc::ExecutionEffect {
        let mut pb_execution_effect = ipc::ExecutionEffect::new();

        let pb_op_map: Vec<OpEntry> = execution_effect.ops.into_iter().map(Into::into).collect();
        pb_execution_effect.set_op_map(pb_op_map.into());

        let pb_transform_map: Vec<ProbufTransformEntry> = execution_effect
            .transforms
            .into_iter()
            .map(Into::into)
            .collect();
        pb_execution_effect.set_transform_map(pb_transform_map.into());

        pb_execution_effect
    }
}
