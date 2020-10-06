use casper_execution_engine::shared::opcode_costs::OpCodeCosts;

use crate::engine_server::ipc;

impl From<OpCodeCosts> for ipc::ChainSpec_WasmConfig_OpCodeCosts {
    fn from(opcode_costs: OpCodeCosts) -> Self {
        ipc::ChainSpec_WasmConfig_OpCodeCosts {
            regular: opcode_costs.regular,
            div: opcode_costs.div,
            mul: opcode_costs.mul,
            mem: opcode_costs.mem,
            grow_mem: opcode_costs.grow_mem,
            ..Default::default()
        }
    }
}

impl From<ipc::ChainSpec_WasmConfig_OpCodeCosts> for OpCodeCosts {
    fn from(pb_opcode_costs: ipc::ChainSpec_WasmConfig_OpCodeCosts) -> Self {
        OpCodeCosts {
            regular: pb_opcode_costs.regular,
            div: pb_opcode_costs.div,
            mul: pb_opcode_costs.mul,
            mem: pb_opcode_costs.mem,
            grow_mem: pb_opcode_costs.grow_mem,
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
