use casperlabs_types::ContractWasm;

use crate::engine_server::state;

impl From<ContractWasm> for state::ContractWasm {
    fn from(contract: ContractWasm) -> Self {
        let mut pb_contract_wasm = state::ContractWasm::new();
        pb_contract_wasm.set_wasm(contract.take_bytes());
        pb_contract_wasm
    }
}

impl From<state::ContractWasm> for ContractWasm {
    fn from(mut contract: state::ContractWasm) -> Self {
        ContractWasm::new(contract.take_wasm())
    }
}

#[cfg(test)]
mod tests {
    use proptest::proptest;

    use casperlabs_types::gens;

    use super::*;
    use crate::engine_server::mappings::test_utils;

    proptest! {
        #[test]
        fn round_trip(contract_wasm in gens::contract_wasm_arb()) {
            test_utils::protobuf_round_trip::<ContractWasm, state::ContractWasm>(contract_wasm);
        }
    }
}
