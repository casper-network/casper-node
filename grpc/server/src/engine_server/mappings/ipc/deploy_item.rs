use std::{
    collections::BTreeSet,
    convert::{TryFrom, TryInto},
};

use casper_execution_engine::core::engine_state::deploy_item::DeployItem;
use casper_types::account::AccountHash;

use crate::engine_server::{ipc, mappings::MappingError};

impl TryFrom<ipc::DeployItem> for DeployItem {
    type Error = MappingError;

    fn try_from(mut pb_deploy_item: ipc::DeployItem) -> Result<Self, Self::Error> {
        let address = AccountHash::try_from(pb_deploy_item.get_address())
            .map_err(|_| MappingError::invalid_account_hash_length(pb_deploy_item.address.len()))?;

        let session = pb_deploy_item
            .take_session()
            .payload
            .ok_or(MappingError::MissingPayload)?
            .try_into()?;

        let payment = pb_deploy_item
            .take_payment()
            .payload
            .ok_or(MappingError::MissingPayload)?
            .try_into()?;

        let gas_price = pb_deploy_item.get_gas_price();

        let authorization_keys = pb_deploy_item
            .get_authorization_keys()
            .iter()
            .map(|raw: &Vec<u8>| {
                AccountHash::try_from(raw.as_slice())
                    .map_err(|_| MappingError::invalid_account_hash_length(raw.len()))
            })
            .collect::<Result<BTreeSet<AccountHash>, Self::Error>>()?;

        let deploy_hash = pb_deploy_item.get_deploy_hash().try_into().map_err(|_| {
            MappingError::invalid_deploy_hash_length(pb_deploy_item.deploy_hash.len())
        })?;

        Ok(DeployItem::new(
            address,
            session,
            payment,
            gas_price,
            authorization_keys,
            deploy_hash,
        ))
    }
}

impl From<DeployItem> for ipc::DeployItem {
    fn from(deploy_item: DeployItem) -> Self {
        let mut result = ipc::DeployItem::new();
        result.set_address(deploy_item.address.as_bytes().to_vec());
        result.set_session(deploy_item.session.into());
        result.set_payment(deploy_item.payment.into());
        result.set_gas_price(deploy_item.gas_price);
        result.set_authorization_keys(
            deploy_item
                .authorization_keys
                .into_iter()
                .map(|key| key.as_bytes().to_vec())
                .collect(),
        );
        result.set_deploy_hash(deploy_item.deploy_hash.to_vec());
        result
    }
}
