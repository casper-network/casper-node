use std::convert::{TryFrom, TryInto};

use casper_types::{account::AccountHash, DeployHash, Key, TransferAddr};

use crate::engine_server::{
    mappings::{self, ParsingError},
    state::{self, Key_Address, Key_Hash, Key_oneof_value},
};

impl From<Key> for state::Key {
    fn from(key: Key) -> Self {
        let mut pb_key = state::Key::new();
        match key {
            Key::Account(account) => {
                let mut pb_account = Key_Address::new();
                pb_account.set_account(account.as_bytes().to_vec());
                pb_key.set_address(pb_account);
            }
            Key::Hash(hash) => {
                let mut pb_hash = Key_Hash::new();
                pb_hash.set_hash(hash.to_vec());
                pb_key.set_hash(pb_hash);
            }
            Key::URef(uref) => {
                pb_key.set_uref(uref.into());
            }
            Key::DeployInfo(deploy_hash) => {
                let mut pb_deploy_hash = state::DeployHash::new();
                pb_deploy_hash.set_deploy_hash(deploy_hash.value().to_vec());
                pb_key.set_deploy_info(pb_deploy_hash)
            }
            Key::Transfer(transfer_addr) => {
                let mut pb_transfer_addr = state::TransferAddr::new();
                pb_transfer_addr.set_transfer_addr(transfer_addr.value().to_vec());
                pb_key.set_transfer(pb_transfer_addr)
            }
            Key::EraInfo(era_id) => pb_key.set_era_info(era_id),
        }
        pb_key
    }
}

impl TryFrom<state::Key> for Key {
    type Error = ParsingError;

    fn try_from(pb_key: state::Key) -> Result<Self, Self::Error> {
        let pb_key = pb_key
            .value
            .ok_or_else(|| ParsingError::from("Unable to parse Protobuf Key"))?;

        let key = match pb_key {
            Key_oneof_value::address(pb_account) => {
                let account = mappings::vec_to_array(pb_account.account, "Protobuf Key::Account")?;
                Key::Account(AccountHash::new(account))
            }
            Key_oneof_value::hash(pb_hash) => {
                let hash = mappings::vec_to_array(pb_hash.hash, "Protobuf Key::Hash")?;
                Key::Hash(hash)
            }
            Key_oneof_value::uref(pb_uref) => {
                let uref = pb_uref.try_into()?;
                Key::URef(uref)
            }
            Key_oneof_value::transfer(pb_transfer_addr) => {
                let transfer_addr = TransferAddr::new(mappings::vec_to_array(
                    pb_transfer_addr.transfer_addr,
                    "Protobuf Key::Transfer",
                )?);
                Key::Transfer(transfer_addr)
            }
            Key_oneof_value::deploy_info(pb_deploy_hash) => {
                let deploy_hash = DeployHash::new(mappings::vec_to_array(
                    pb_deploy_hash.deploy_hash,
                    "Protobuf Key::DeployInfo",
                )?);
                Key::DeployInfo(deploy_hash)
            }
            Key_oneof_value::era_info(era_id) => Key::EraInfo(era_id),
        };
        Ok(key)
    }
}

#[cfg(test)]
mod tests {
    use proptest::proptest;

    use casper_types::gens;

    use super::*;
    use crate::engine_server::mappings::test_utils;

    proptest! {
        #[test]
        fn round_trip(key in gens::key_arb()) {
            test_utils::protobuf_round_trip::<Key, state::Key>(key);
        }
    }
}
