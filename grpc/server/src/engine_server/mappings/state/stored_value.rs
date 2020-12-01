use std::convert::{TryFrom, TryInto};

use casper_execution_engine::shared::stored_value::StoredValue;

use crate::engine_server::{
    mappings::ParsingError,
    state::{self, StoredValue_oneof_variants},
};

impl From<StoredValue> for state::StoredValue {
    fn from(value: StoredValue) -> Self {
        let mut pb_value = state::StoredValue::new();

        match value {
            StoredValue::CLValue(cl_value) => pb_value.set_cl_value(cl_value.into()),
            StoredValue::Account(account) => pb_value.set_account(account.into()),
            StoredValue::Contract(contract) => {
                pb_value.set_contract(contract.into());
            }
            StoredValue::ContractWasm(contract_wasm) => {
                pb_value.set_contract_wasm(contract_wasm.into())
            }
            StoredValue::ContractPackage(contract_package) => {
                pb_value.set_contract_package(contract_package.into())
            }
            StoredValue::Transfer(transfer) => pb_value.set_transfer(transfer.into()),
            StoredValue::DeployInfo(deploy_info) => pb_value.set_deploy_info(deploy_info.into()),
            StoredValue::AuctionInfo(auction_info) => {
                pb_value.set_auction_info(auction_info.into())
            }
        }

        pb_value
    }
}

impl TryFrom<state::StoredValue> for StoredValue {
    type Error = ParsingError;

    fn try_from(pb_value: state::StoredValue) -> Result<Self, Self::Error> {
        let pb_value = pb_value
            .variants
            .ok_or_else(|| ParsingError("Unable to parse Protobuf StoredValue".to_string()))?;

        let value = match pb_value {
            StoredValue_oneof_variants::cl_value(pb_value) => {
                StoredValue::CLValue(pb_value.try_into()?)
            }
            StoredValue_oneof_variants::account(pb_account) => {
                StoredValue::Account(pb_account.try_into()?)
            }
            StoredValue_oneof_variants::contract(pb_contract) => {
                StoredValue::Contract(pb_contract.try_into()?)
            }
            StoredValue_oneof_variants::contract_package(pb_contract_package) => {
                StoredValue::ContractPackage(pb_contract_package.try_into()?)
            }
            StoredValue_oneof_variants::contract_wasm(pb_contract_wasm) => {
                StoredValue::ContractWasm(pb_contract_wasm.into())
            }
            StoredValue_oneof_variants::transfer(pb_transfer) => {
                StoredValue::Transfer(pb_transfer.try_into()?)
            }
            StoredValue_oneof_variants::deploy_info(pb_deploy_info) => {
                StoredValue::DeployInfo(pb_deploy_info.try_into()?)
            }
            StoredValue_oneof_variants::auction_info(pb_auction_info) => {
                StoredValue::AuctionInfo(pb_auction_info.try_into()?)
            }
        };

        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use proptest::proptest;

    use casper_execution_engine::shared::stored_value::gens;

    use super::*;
    use crate::engine_server::mappings::test_utils;

    proptest! {
        #[test]
        fn round_trip(value in gens::stored_value_arb()) {
            test_utils::protobuf_round_trip::<StoredValue, state::StoredValue>(value);
        }
    }
}
