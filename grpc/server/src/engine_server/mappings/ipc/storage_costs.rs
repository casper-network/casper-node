use casper_execution_engine::shared::storage_costs::StorageCosts;

use crate::engine_server::ipc;

impl From<StorageCosts> for ipc::ChainSpec_WasmConfig_StorageCosts {
    fn from(storage_costs: StorageCosts) -> Self {
        ipc::ChainSpec_WasmConfig_StorageCosts {
            gas_per_byte: storage_costs.gas_per_byte(),
            ..Default::default()
        }
    }
}

impl From<ipc::ChainSpec_WasmConfig_StorageCosts> for StorageCosts {
    fn from(pb_storage_costs: ipc::ChainSpec_WasmConfig_StorageCosts) -> Self {
        StorageCosts::new(pb_storage_costs.gas_per_byte)
    }
}

#[cfg(test)]
mod tests {
    use proptest::proptest;

    use casper_execution_engine::shared::storage_costs::gens;

    use super::*;
    use crate::engine_server::mappings::test_utils;

    proptest! {
        #[test]
        fn round_trip(storage_costs in gens::storage_costs_arb()) {
            test_utils::protobuf_round_trip::<StorageCosts, ipc::ChainSpec_WasmConfig_StorageCosts>(storage_costs);
        }
    }
}
