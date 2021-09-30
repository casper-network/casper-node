//! Units of account-triggered execution.

use std::collections::BTreeSet;

use casper_types::{account::AccountHash, DeployHash};

use crate::core::engine_state::executable_deploy_item::ExecutableDeployItem;

type GasPrice = u64;

/// Definition of a deploy with all the details that make it possible to execute it.
/// Corresponds to the similarly-named IPC protobuf message.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DeployItem {
    /// Address that created and signed this deploy. This address will be used as a context for
    /// executing session code.
    pub address: AccountHash,
    /// Session code.
    pub session: ExecutableDeployItem,
    /// Payment code.
    pub payment: ExecutableDeployItem,
    /// Gas price specified for this deploy by the user.
    pub gas_price: GasPrice,
    /// List of accounts that signed this deploy.
    pub authorization_keys: BTreeSet<AccountHash>,
    /// A unique identifier of the deploy.
    /// Currently it is the hash of the deploy header (see `DeployHeader` in the `types` crate).
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
