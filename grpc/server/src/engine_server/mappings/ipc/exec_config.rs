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
        let mint_initializer_bytes = pb_exec_config.take_mint_installer();
        let proof_of_stake_initializer_bytes = pb_exec_config.take_pos_installer();
        let standard_payment_installer_bytes = pb_exec_config.take_standard_payment_installer();
        let auction_installer_bytes = pb_exec_config.take_auction_installer();
        Ok(ExecConfig::new(
            mint_initializer_bytes,
            proof_of_stake_initializer_bytes,
            standard_payment_installer_bytes,
            auction_installer_bytes,
            accounts,
            wasm_config,
        ))
    }
}

impl From<ExecConfig> for ipc::ChainSpec_GenesisConfig_ExecConfig {
    fn from(exec_config: ExecConfig) -> ipc::ChainSpec_GenesisConfig_ExecConfig {
        let mut pb_exec_config = ipc::ChainSpec_GenesisConfig_ExecConfig::new();
        pb_exec_config.set_mint_installer(exec_config.mint_installer_bytes().to_vec());
        pb_exec_config.set_pos_installer(exec_config.proof_of_stake_installer_bytes().to_vec());
        pb_exec_config.set_standard_payment_installer(
            exec_config.standard_payment_installer_bytes().to_vec(),
        );
        pb_exec_config.set_auction_installer(exec_config.auction_installer_bytes().to_vec());
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
