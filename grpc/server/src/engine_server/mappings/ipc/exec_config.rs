use std::convert::{TryFrom, TryInto};

use casper_execution_engine::core::engine_state::genesis::{ExecConfig, GenesisAccount};

use crate::engine_server::{ipc, mappings::MappingError};

impl TryFrom<ipc::ChainSpec_GenesisConfig_ExecConfig> for ExecConfig {
    type Error = MappingError;

    fn try_from(
        mut pb_exec_config: ipc::ChainSpec_GenesisConfig_ExecConfig,
    ) -> Result<Self, Self::Error> {
        let accounts = pb_exec_config
            .take_accounts()
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<GenesisAccount>, Self::Error>>()?;
        let wasm_config = pb_exec_config.take_wasm_config().try_into()?;
        let validator_slots = pb_exec_config.get_validator_slots();
        let auction_delay = pb_exec_config.get_auction_delay();
        let locked_funds_period = pb_exec_config.get_locked_funds_period();
        let round_seigniorage_rate = pb_exec_config.take_round_seigniorage_rate().into();
        let unbonding_delay = pb_exec_config.get_unbonding_delay();
        let wasmless_transfer_cost = pb_exec_config.get_wasmless_transfer_cost();
        Ok(ExecConfig::new(
            accounts,
            wasm_config,
            validator_slots,
            auction_delay,
            locked_funds_period,
            round_seigniorage_rate,
            unbonding_delay,
            wasmless_transfer_cost,
        ))
    }
}

impl From<ExecConfig> for ipc::ChainSpec_GenesisConfig_ExecConfig {
    fn from(exec_config: ExecConfig) -> ipc::ChainSpec_GenesisConfig_ExecConfig {
        let mut pb_exec_config = ipc::ChainSpec_GenesisConfig_ExecConfig::new();
        {
            let accounts = exec_config
                .accounts()
                .iter()
                .cloned()
                .map(Into::into)
                .collect::<Vec<ipc::ChainSpec_GenesisConfig_ExecConfig_GenesisAccount>>();
            pb_exec_config.set_accounts(accounts.into());
        }
        pb_exec_config.set_wasm_config(exec_config.wasm_config().clone().into());
        pb_exec_config.set_validator_slots(exec_config.validator_slots());
        pb_exec_config.set_auction_delay(exec_config.auction_delay());
        pb_exec_config.set_locked_funds_period(exec_config.locked_funds_period());
        pb_exec_config.set_round_seigniorage_rate(exec_config.round_seigniorage_rate().into());
        pb_exec_config.set_unbonding_delay(exec_config.unbonding_delay());
        pb_exec_config.set_wasmless_transfer_cost(exec_config.wasmless_transfer_cost());
        pb_exec_config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine_server::mappings::test_utils;

    #[test]
    fn round_trip() {
        let exec_config = rand::random();
        test_utils::protobuf_round_trip::<ExecConfig, ipc::ChainSpec_GenesisConfig_ExecConfig>(
            exec_config,
        );
    }
}
