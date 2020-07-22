use casperlabs_node::components::contract_runtime::shared::wasm_costs::WasmCosts;

use crate::engine_server::ipc::ChainSpec_CostTable_WasmCosts;

impl From<WasmCosts> for ChainSpec_CostTable_WasmCosts {
    fn from(wasm_costs: WasmCosts) -> Self {
        ChainSpec_CostTable_WasmCosts {
            regular: wasm_costs.regular,
            div: wasm_costs.div,
            mul: wasm_costs.mul,
            mem: wasm_costs.mem,
            initial_mem: wasm_costs.initial_mem,
            grow_mem: wasm_costs.grow_mem,
            memcpy: wasm_costs.memcpy,
            max_stack_height: wasm_costs.max_stack_height,
            opcodes_mul: wasm_costs.opcodes_mul,
            opcodes_div: wasm_costs.opcodes_div,
            ..Default::default()
        }
    }
}

impl From<ChainSpec_CostTable_WasmCosts> for WasmCosts {
    fn from(pb_wasm_costs: ChainSpec_CostTable_WasmCosts) -> Self {
        WasmCosts {
            regular: pb_wasm_costs.regular,
            div: pb_wasm_costs.div,
            mul: pb_wasm_costs.mul,
            mem: pb_wasm_costs.mem,
            initial_mem: pb_wasm_costs.initial_mem,
            grow_mem: pb_wasm_costs.grow_mem,
            memcpy: pb_wasm_costs.memcpy,
            max_stack_height: pb_wasm_costs.max_stack_height,
            opcodes_mul: pb_wasm_costs.opcodes_mul,
            opcodes_div: pb_wasm_costs.opcodes_div,
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::proptest;

    use casperlabs_node::components::contract_runtime::shared::wasm_costs::gens;

    use super::*;
    use crate::engine_server::mappings::test_utils;

    proptest! {
        #[test]
        fn round_trip(wasm_costs in gens::wasm_costs_arb()) {
            test_utils::protobuf_round_trip::<WasmCosts, ChainSpec_CostTable_WasmCosts>(wasm_costs);
        }
    }
}
