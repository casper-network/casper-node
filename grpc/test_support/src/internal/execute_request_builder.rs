use std::convert::TryInto;

use rand::Rng;

use casper_execution_engine::core::engine_state::{
    deploy_item::DeployItem, execute_request::ExecuteRequest,
};
use casper_types::{
    account::AccountHash, contracts::ContractVersion, runtime_args, ContractHash, ProtocolVersion,
    RuntimeArgs,
};

use crate::internal::{DeployItemBuilder, DEFAULT_BLOCK_TIME, DEFAULT_PAYMENT};

const ARG_AMOUNT: &str = "amount";

#[derive(Debug)]
pub struct ExecuteRequestBuilder {
    execute_request: ExecuteRequest,
}

impl ExecuteRequestBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn from_deploy_item(deploy_item: DeployItem) -> Self {
        ExecuteRequestBuilder::new().push_deploy(deploy_item)
    }

    pub fn push_deploy(mut self, deploy: DeployItem) -> Self {
        self.execute_request.deploys.push(Ok(deploy));
        self
    }

    pub fn with_pre_state_hash(mut self, pre_state_hash: &[u8]) -> Self {
        self.execute_request.parent_state_hash = pre_state_hash.try_into().unwrap();
        self
    }

    pub fn with_block_time(mut self, block_time: u64) -> Self {
        self.execute_request.block_time = block_time;
        self
    }

    pub fn with_protocol_version(mut self, protocol_version: ProtocolVersion) -> Self {
        self.execute_request.protocol_version = protocol_version;
        self
    }

    pub fn build(self) -> ExecuteRequest {
        self.execute_request
    }

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

    /// Calls a versioned contract from contract package hash key_name
    pub fn versioned_contract_call_by_hash_key_name(
        sender: AccountHash,
        hash_key_name: &str,
        version: Option<ContractVersion>,
        entry_point_name: &str,
        args: RuntimeArgs,
    ) -> Self {
        let mut rng = rand::thread_rng();
        let deploy_hash = rng.gen();

        let deploy = DeployItemBuilder::new()
            .with_address(sender)
            .with_stored_versioned_contract_by_name(hash_key_name, version, entry_point_name, args)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[sender])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy)
    }
}

impl Default for ExecuteRequestBuilder {
    fn default() -> Self {
        let mut execute_request: ExecuteRequest = Default::default();
        execute_request.block_time = DEFAULT_BLOCK_TIME;
        execute_request.protocol_version = ProtocolVersion::V1_0_0;
        ExecuteRequestBuilder { execute_request }
    }
}
