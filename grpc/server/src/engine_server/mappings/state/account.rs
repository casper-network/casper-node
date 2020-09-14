use std::{
    collections::BTreeMap,
    convert::{TryFrom, TryInto},
    mem,
};

use casper_execution_engine::shared::account::{Account, ActionThresholds, AssociatedKeys};
use casper_types::account::{AccountHash, Weight};

use super::NamedKeyMap;
use crate::engine_server::{
    mappings::{self, ParsingError},
    state::{self, Account_AssociatedKey, NamedKey},
};

impl From<Account> for state::Account {
    fn from(mut account: Account) -> Self {
        let mut pb_account = state::Account::new();

        pb_account.public_key = account.account_hash().as_bytes().to_vec();

        let named_keys = mem::replace(account.named_keys_mut(), BTreeMap::new());
        let pb_named_keys: Vec<NamedKey> = NamedKeyMap::new(named_keys).into();
        pb_account.set_named_keys(pb_named_keys.into());

        pb_account.set_main_purse(account.main_purse().into());

        let associated_keys: Vec<Account_AssociatedKey> =
            account.get_associated_keys().map(Into::into).collect();
        pb_account.set_associated_keys(associated_keys.into());

        {
            let deployment = u32::from(account.action_thresholds().deployment().value());
            let key_management = u32::from(account.action_thresholds().key_management().value());
            let pb_action_thresholds = pb_account.mut_action_thresholds();
            pb_action_thresholds.set_deployment_threshold(deployment);
            pb_action_thresholds.set_key_management_threshold(key_management)
        }

        pb_account
    }
}

impl TryFrom<state::Account> for Account {
    type Error = ParsingError;

    fn try_from(pb_account: state::Account) -> Result<Self, Self::Error> {
        let account_hash =
            mappings::vec_to_array(pb_account.public_key, "Protobuf Account::AccountHash")?;

        let named_keys: NamedKeyMap = pb_account.named_keys.into_vec().try_into()?;

        let main_purse = {
            let pb_uref = pb_account
                .main_purse
                .into_option()
                .ok_or_else(|| ParsingError::from("Protobuf Account missing MainPurse field"))?;
            pb_uref.try_into()?
        };

        let associated_keys = {
            let mut associated_keys = AssociatedKeys::default();
            for pb_associated_key in pb_account.associated_keys.into_vec() {
                let (key, weight) = pb_associated_key.try_into()?;
                associated_keys.add_key(key, weight).map_err(|error| {
                    ParsingError(format!(
                        "Error parsing Protobuf Account::AssociatedKeys: {:?}",
                        error
                    ))
                })?;
            }
            associated_keys
        };

        let action_thresholds = {
            let pb_action_thresholds =
                pb_account.action_thresholds.into_option().ok_or_else(|| {
                    ParsingError::from("Protobuf Account missing ActionThresholds field")
                })?;

            ActionThresholds::new(
                weight_from(
                    pb_action_thresholds.deployment_threshold,
                    "Protobuf DeploymentThreshold",
                )?,
                weight_from(
                    pb_action_thresholds.key_management_threshold,
                    "Protobuf KeyManagementThreshold",
                )?,
            )
            .map_err(ParsingError::from)?
        };

        let account = Account::new(
            AccountHash::new(account_hash),
            named_keys.into_inner(),
            main_purse,
            associated_keys,
            action_thresholds,
        );
        Ok(account)
    }
}

impl From<(&AccountHash, &Weight)> for Account_AssociatedKey {
    fn from((account_hash, weight): (&AccountHash, &Weight)) -> Self {
        let mut pb_associated_key = Account_AssociatedKey::new();
        pb_associated_key.public_key = account_hash.as_bytes().to_vec();
        pb_associated_key.set_weight(weight.value().into());
        pb_associated_key
    }
}

impl TryFrom<Account_AssociatedKey> for (AccountHash, Weight) {
    type Error = ParsingError;

    fn try_from(pb_associated_key: Account_AssociatedKey) -> Result<Self, Self::Error> {
        let account_hash = AccountHash::new(mappings::vec_to_array(
            pb_associated_key.public_key,
            "Protobuf Account::AssociatedKey",
        )?);

        let weight = weight_from(pb_associated_key.weight, "Protobuf AssociatedKey::Weight")?;

        Ok((account_hash, weight))
    }
}

fn weight_from(value: u32, value_name: &str) -> Result<Weight, ParsingError> {
    let weight = u8::try_from(value).map_err(|_| {
        ParsingError(format!(
            "Unable to convert {} to u8 while parsing {}",
            value, value_name
        ))
    })?;
    Ok(Weight::new(weight))
}

#[cfg(test)]
mod tests {
    use proptest::proptest;

    use casper_execution_engine::shared::account::gens;

    use super::*;
    use crate::engine_server::mappings::test_utils;

    proptest! {
        #[test]
        fn round_trip(account in gens::account_arb()) {
            test_utils::protobuf_round_trip::<Account, state::Account>(account);
        }
    }
}
