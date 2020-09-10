use std::convert::{TryFrom, TryInto};

use casper_execution_engine::{core::engine_state::genesis::GenesisAccount, shared::motes::Motes};
use casper_types::{
    account::AccountHash,
    bytesrepr::{self, ToBytes},
};

use crate::engine_server::{
    ipc::ChainSpec_GenesisConfig_ExecConfig_GenesisAccount, mappings::MappingError,
};

impl From<GenesisAccount> for ChainSpec_GenesisConfig_ExecConfig_GenesisAccount {
    fn from(genesis_account: GenesisAccount) -> Self {
        let mut pb_genesis_account = ChainSpec_GenesisConfig_ExecConfig_GenesisAccount::new();

        if let Some(public_key) = genesis_account.public_key() {
            pb_genesis_account.set_public_key_bytes(public_key.to_bytes().unwrap());
        }

        pb_genesis_account.set_account_hash_bytes(genesis_account.account_hash().value().to_vec());
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

        let balance = pb_genesis_account
            .take_balance()
            .try_into()
            .map(Motes::new)?;
        let bonded_amount = pb_genesis_account
            .take_bonded_amount()
            .try_into()
            .map(Motes::new)?;

        if pb_genesis_account.public_key_bytes.is_empty() {
            return Ok(GenesisAccount::system(balance, bonded_amount));
        }

        let public_key = bytesrepr::deserialize(pb_genesis_account.take_public_key_bytes())?;

        let account_hash = AccountHash::try_from(
            &pb_genesis_account.get_account_hash_bytes() as &[u8]
        )
        .map_err(|_| {
            MappingError::invalid_account_hash_length(
                pb_genesis_account.get_account_hash_bytes().len(),
            )
        })?;

        Ok(GenesisAccount::new(
            public_key,
            account_hash,
            balance,
            bonded_amount,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine_server::mappings::test_utils;
    use casper_types::U512;

    #[test]
    fn round_trip() {
        let genesis_account = rand::random();
        test_utils::protobuf_round_trip::<
            GenesisAccount,
            ChainSpec_GenesisConfig_ExecConfig_GenesisAccount,
        >(genesis_account);
    }

    #[test]
    fn round_trip_system_account() {
        let genesis_account = GenesisAccount::system(
            Motes::new(U512::max_value()),
            Motes::new(U512::max_value() - 1),
        );
        test_utils::protobuf_round_trip::<
            GenesisAccount,
            ChainSpec_GenesisConfig_ExecConfig_GenesisAccount,
        >(genesis_account);
    }
}
