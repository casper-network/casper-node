use std::convert::TryInto;

use rand::Rng;

use casper_execution_engine::core::engine_state::{
    deploy_item::DeployItem, execute_request::ExecuteRequest,
};
use casper_types::{
    account::AccountHash, runtime_args, ContractHash, ContractPackageHash, ContractVersion,
    ProtocolVersion, RuntimeArgs,
};

use crate::{DeployItemBuilder, DEFAULT_BLOCK_TIME, DEFAULT_PAYMENT, DEFAULT_PROPOSER_PUBLIC_KEY};

const ARG_AMOUNT: &str = "amount";

/// Builds an [`ExecuteRequest`].
#[derive(Debug)]
pub struct ExecuteRequestBuilder {
    execute_request: ExecuteRequest,
}

impl ExecuteRequestBuilder {
    /// Returns an [`ExecuteRequestBuilder`].
    pub fn new() -> Self {
        Default::default()
    }

    /// Takes a [`DeployItem`] and returns an [`ExecuteRequestBuilder`].
    pub fn from_deploy_item(deploy_item: DeployItem) -> Self {
        ExecuteRequestBuilder::new().push_deploy(deploy_item)
    }

    /// Adds a [`DeployItem`] to the [`ExecuteRequest`].
    pub fn push_deploy(mut self, deploy: DeployItem) -> Self {
        self.execute_request.deploys.push(deploy);
        self
    }

    /// Sets the parent state hash of the [`ExecuteRequest`].
    pub fn with_pre_state_hash(mut self, pre_state_hash: &[u8]) -> Self {
        self.execute_request.parent_state_hash = pre_state_hash.try_into().unwrap();
        self
    }

    /// Sets the block time of the [`ExecuteRequest`].
    pub fn with_block_time(mut self, block_time: u64) -> Self {
        self.execute_request.block_time = block_time;
        self
    }

    /// Sets the protocol version of the [`ExecuteRequest`].
    pub fn with_protocol_version(mut self, protocol_version: ProtocolVersion) -> Self {
        self.execute_request.protocol_version = protocol_version;
        self
    }

    /// Sets the proposer used by the [`ExecuteRequest`].
    pub fn with_proposer(mut self, proposer: casper_types::PublicKey) -> Self {
        self.execute_request.proposer = proposer;
        self
    }

    /// Consumes self and returns an [`ExecuteRequest`].
    pub fn build(self) -> ExecuteRequest {
        self.execute_request
    }

    /// Returns an [`ExecuteRequest`] with standard dependencies.
    pub fn standard(
        account_hash: AccountHash,
        session_file: &str,
        session_args: RuntimeArgs,
    ) -> Self {
        let mut rng = rand::thread_rng();
        let deploy_hash: [u8; 32] = rng.gen();

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_code(session_file, session_args)
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => *DEFAULT_PAYMENT
            })
            .with_authorization_keys(&[account_hash])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy)
    }

    /// Returns an [`ExecuteRequest`] from a module bytes.
    pub fn module_bytes(
        account_hash: AccountHash,
        module_bytes: Vec<u8>,
        session_args: RuntimeArgs,
    ) -> Self {
        let mut rng = rand::thread_rng();
        let deploy_hash: [u8; 32] = rng.gen();

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_bytes(module_bytes, session_args)
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => *DEFAULT_PAYMENT
            })
            .with_authorization_keys(&[account_hash])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy)
    }

    /// Returns an [`ExecuteRequest`] that will call a stored contract by hash.
    pub fn contract_call_by_hash(
        sender: AccountHash,
        contract_hash: ContractHash,
        entry_point: &str,
        args: RuntimeArgs,
    ) -> Self {
        let mut rng = rand::thread_rng();
        let deploy_hash = rng.gen();

        let deploy = DeployItemBuilder::new()
            .with_address(sender)
            .with_stored_session_hash(contract_hash, entry_point, args)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[sender])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy)
    }

    /// Returns an [`ExecuteRequest`] that will call a stored contract by named key.
    pub fn contract_call_by_name(
        sender: AccountHash,
        contract_name: &str,
        entry_point: &str,
        args: RuntimeArgs,
    ) -> Self {
        let mut rng = rand::thread_rng();
        let deploy_hash = rng.gen();

        let deploy = DeployItemBuilder::new()
            .with_address(sender)
            .with_stored_session_named_key(contract_name, entry_point, args)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[sender])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy)
    }

    /// Returns an [`ExecuteRequest`] that will call a versioned stored contract by hash.
    pub fn versioned_contract_call_by_hash(
        sender: AccountHash,
        contract_package_hash: ContractPackageHash,
        version: Option<ContractVersion>,
        entry_point_name: &str,
        args: RuntimeArgs,
    ) -> Self {
        let mut rng = rand::thread_rng();
        let deploy_hash = rng.gen();

        let deploy = DeployItemBuilder::new()
            .with_address(sender)
            .with_stored_versioned_contract_by_hash(
                contract_package_hash.value(),
                version,
                entry_point_name,
                args,
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[sender])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy)
    }

    /// Calls a versioned contract from contract package hash key_name
    pub fn versioned_contract_call_by_name(
        sender: AccountHash,
        contract_name: &str,
        version: Option<ContractVersion>,
        entry_point_name: &str,
        args: RuntimeArgs,
    ) -> Self {
        let mut rng = rand::thread_rng();
        let deploy_hash = rng.gen();

        let deploy = DeployItemBuilder::new()
            .with_address(sender)
            .with_stored_versioned_contract_by_name(contract_name, version, entry_point_name, args)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[sender])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy)
    }

    /// Returns an [`ExecuteRequest`] for a native transfer.
    pub fn transfer(sender: AccountHash, transfer_args: RuntimeArgs) -> Self {
        let mut rng = rand::thread_rng();
        let deploy_hash = rng.gen();

        let deploy_item = DeployItemBuilder::new()
            .with_address(sender)
            .with_empty_payment_bytes(runtime_args! {})
            .with_transfer_args(transfer_args)
            .with_authorization_keys(&[sender])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::from_deploy_item(deploy_item)
    }
}

impl Default for ExecuteRequestBuilder {
    fn default() -> Self {
        let execute_request = ExecuteRequest {
            block_time: DEFAULT_BLOCK_TIME,
            protocol_version: ProtocolVersion::V1_0_0,
            proposer: DEFAULT_PROPOSER_PUBLIC_KEY.clone(),
            ..Default::default()
        };
        ExecuteRequestBuilder { execute_request }
    }
}
