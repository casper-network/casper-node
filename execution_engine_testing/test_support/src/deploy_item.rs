//! Units of account-triggered execution.

use std::collections::BTreeSet;

use casper_execution_engine::engine_state::{BlockInfo, InvalidRequest, WasmV1Request};
use casper_types::{
    account::AccountHash, Deploy, DeployHash, ExecutableDeployItem, Gas, InitiatorAddr,
    TransactionHash,
};

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
    pub gas_price: u8,
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
        gas_price: u8,
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

    /// Is this a native transfer?
    pub fn is_native_transfer(&self) -> bool {
        matches!(self.session, ExecutableDeployItem::Transfer { .. })
    }

    /// Creates a new request from a deploy item for use as the session code.
    pub fn new_session_from_deploy_item(
        &self,
        block_info: BlockInfo,
        gas_limit: Gas,
    ) -> Result<WasmV1Request, InvalidRequest> {
        let address = &self.address;
        let session = &self.session;
        let authorization_keys = &self.authorization_keys;
        let deploy_hash = &self.deploy_hash;

        let transaction_hash = TransactionHash::Deploy(*deploy_hash);
        let initiator_addr = InitiatorAddr::AccountHash(*address);
        let authorization_keys = authorization_keys.clone();
        WasmV1Request::new_from_executable_deploy_item(
            block_info,
            gas_limit,
            transaction_hash,
            initiator_addr,
            authorization_keys,
            session,
        )
    }

    /// Creates a new request from a deploy item for use as custom payment.
    pub fn new_custom_payment_from_deploy_item(
        &self,
        block_info: BlockInfo,
        gas_limit: Gas,
    ) -> Result<WasmV1Request, InvalidRequest> {
        let address = &self.address;
        let payment = &self.payment;
        let authorization_keys = &self.authorization_keys;
        let deploy_hash = &self.deploy_hash;

        let transaction_hash = TransactionHash::Deploy(*deploy_hash);
        let initiator_addr = InitiatorAddr::AccountHash(*address);
        let authorization_keys = authorization_keys.clone();

        WasmV1Request::new_payment_from_executable_deploy_item(
            block_info,
            gas_limit,
            transaction_hash,
            initiator_addr,
            authorization_keys,
            payment,
        )
    }
}

impl From<Deploy> for DeployItem {
    fn from(deploy: Deploy) -> Self {
        let address = deploy.header().account().to_account_hash();
        let authorization_keys = deploy
            .approvals()
            .iter()
            .map(|approval| approval.signer().to_account_hash())
            .collect();

        DeployItem::new(
            address,
            deploy.session().clone(),
            deploy.payment().clone(),
            deploy.header().gas_price() as u8,
            authorization_keys,
            DeployHash::new(*deploy.hash().inner()),
        )
    }
}
