use std::collections::BTreeSet;
use std::iter::FromIterator;

use types::account::AccountHash;

use crate::{
    components::{
        contract_runtime::core::{
            engine_state::executable_deploy_item::ExecutableDeployItem, DeployHash,
        },
        storage::Value,
    },
    types::Deploy,
};

type GasPrice = u64;

/// Represents a deploy to be executed.  Corresponds to the similarly-named ipc protobuf message.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DeployItem {
    pub address: AccountHash,
    pub session: ExecutableDeployItem,
    pub payment: ExecutableDeployItem,
    pub gas_price: GasPrice,
    pub authorization_keys: BTreeSet<AccountHash>,
    pub deploy_hash: DeployHash,
}

impl DeployItem {
    /// Creates a [`DeployItem`].
    pub fn new(
        address: AccountHash,
        session: ExecutableDeployItem,
        payment: ExecutableDeployItem,
        gas_price: GasPrice,
        authorization_keys: BTreeSet<AccountHash>,
        deploy_hash: DeployHash,
    ) -> Self {
        DeployItem {
            address,
            session,
            payment,
            gas_price,
            authorization_keys,
            deploy_hash,
        }
    }
}

impl From<Deploy> for DeployItem {
    fn from(deploy: Deploy) -> Self {
        let account_hash = deploy.header().account.to_account_hash();
        DeployItem::new(
            account_hash,
            deploy.session().clone(),
            deploy.payment().clone(),
            deploy.header().gas_price,
            BTreeSet::from_iter(vec![account_hash]),
            deploy.id().inner().to_bytes(),
        )
    }
}
