use casper_execution_engine::shared::opcode_costs::OpCodeCosts;

use crate::engine_server::ipc;

impl From<OpCodeCosts> for ipc::ChainSpec_WasmConfig_OpCodeCosts {
    fn from(opcode_costs: OpCodeCosts) -> Self {
        ipc::ChainSpec_WasmConfig_OpCodeCosts {
            bit: opcode_costs.bit,
            add: opcode_costs.add,
            mul: opcode_costs.mul,
            div: opcode_costs.div,
            load: opcode_costs.load,
            store: opcode_costs.store,
            field_const: opcode_costs.op_const,
            local: opcode_costs.local,
            global: opcode_costs.global,
            control_flow: opcode_costs.control_flow,
            integer_comparsion: opcode_costs.integer_comparsion,
            conversion: opcode_costs.conversion,
            unreachable: opcode_costs.unreachable,
            nop: opcode_costs.nop,
            current_memory: opcode_costs.current_memory,
            grow_memory: opcode_costs.grow_memory,
            regular: opcode_costs.regular,
            ..Default::default()
        }
    }
}

impl From<ipc::ChainSpec_WasmConfig_OpCodeCosts> for OpCodeCosts {
    fn from(pb_opcode_costs: ipc::ChainSpec_WasmConfig_OpCodeCosts) -> Self {
        OpCodeCosts {
            bit: pb_opcode_costs.bit,
            add: pb_opcode_costs.add,
            mul: pb_opcode_costs.mul,
            div: pb_opcode_costs.div,
            load: pb_opcode_costs.load,
            store: pb_opcode_costs.store,
            op_const: pb_opcode_costs.field_const,
            local: pb_opcode_costs.local,
            global: pb_opcode_costs.global,
            control_flow: pb_opcode_costs.control_flow,
            integer_comparsion: pb_opcode_costs.integer_comparsion,
            conversion: pb_opcode_costs.conversion,
            unreachable: pb_opcode_costs.unreachable,
            nop: pb_opcode_costs.nop,
            current_memory: pb_opcode_costs.current_memory,
            grow_memory: pb_opcode_costs.grow_memory,
            regular: pb_opcode_costs.regular,
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::proptest;

    use casper_execution_engine::shared::opcode_costs::gens;

    use super::*;
    use crate::engine_server::mappings::test_utils;

    proptest! {
        #[test]
        fn round_trip(opcode_costs in gens::opcode_costs_arb()) {
            test_utils::protobuf_round_trip::<OpCodeCosts, ipc::ChainSpec_WasmConfig_OpCodeCosts>(opcode_costs);
        }
    }
}
