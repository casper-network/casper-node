use std::convert::{TryFrom, TryInto};

use node::components::contract_runtime::core::engine_state::genesis::GenesisAccount;
use node::components::contract_runtime::shared::motes::Motes;
use types::account::AccountHash;

use crate::engine_server::{
    ipc::ChainSpec_GenesisConfig_ExecConfig_GenesisAccount, mappings::MappingError,
};

impl From<GenesisAccount> for ChainSpec_GenesisConfig_ExecConfig_GenesisAccount {
    fn from(genesis_account: GenesisAccount) -> Self {
        let mut pb_genesis_account = ChainSpec_GenesisConfig_ExecConfig_GenesisAccount::new();

        pb_genesis_account.public_key_hash = genesis_account.account_hash().as_bytes().to_vec();
        pb_genesis_account.set_balance(genesis_account.balance().value().into());
        pb_genesis_account.set_bonded_amount(genesis_account.bonded_amount().value().into());

        pb_genesis_account
    }
}

impl TryFrom<ChainSpec_GenesisConfig_ExecConfig_GenesisAccount> for GenesisAccount {
    type Error = MappingError;

    fn try_from(
        mut pb_genesis_account: ChainSpec_GenesisConfig_ExecConfig_GenesisAccount,
    ) -> Result<Self, Self::Error> {
        // TODO: our TryFromSliceForAccountHashError should convey length info
        let account_hash = AccountHash::try_from(&pb_genesis_account.public_key_hash as &[u8])
            .map_err(|_| {
                MappingError::invalid_account_hash_length(pb_genesis_account.public_key_hash.len())
            })?;
        let balance = pb_genesis_account
            .take_balance()
            .try_into()
            .map(Motes::new)?;
        let bonded_amount = pb_genesis_account
            .take_bonded_amount()
            .try_into()
            .map(Motes::new)?;
        Ok(GenesisAccount::new(account_hash, balance, bonded_amount))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine_server::mappings::test_utils;

    #[test]
    fn round_trip() {
        let genesis_account = rand::random();
        test_utils::protobuf_round_trip::<
            GenesisAccount,
            ChainSpec_GenesisConfig_ExecConfig_GenesisAccount,
        >(genesis_account);
    }
}
