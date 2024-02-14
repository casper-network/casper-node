use std::{collections::BTreeSet, convert::TryFrom, iter};

use casper_execution_engine::engine_state::{
    deploy_item::DeployItem, execute_request::ExecuteRequest, Payment, PaymentInfo, Session,
    SessionInfo,
};
use casper_types::{
    account::AccountHash, runtime_args, AddressableEntityHash, BlockTime, DeployHash, Digest,
    EntityVersion, InitiatorAddr, PackageHash, PublicKey, RuntimeArgs, TransactionHash,
    TransactionV1Hash,
};

use crate::{
    DeployItemBuilder, ARG_AMOUNT, DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_PUBLIC_KEY,
    DEFAULT_BLOCK_TIME, DEFAULT_GAS_PRICE, DEFAULT_PAYMENT, DEFAULT_PROPOSER_PUBLIC_KEY,
};

/// Builds an [`ExecuteRequest`].
#[derive(Debug)]
pub struct ExecuteRequestBuilder {
    pre_state_hash: Digest,
    block_time: BlockTime,
    transaction_hash: TransactionHash,
    gas_price: u64,
    initiator_addr: InitiatorAddr,
    payment: Payment,
    payment_entry_point: String,
    payment_args: RuntimeArgs,
    session: Option<Session>,
    session_entry_point: String,
    session_args: RuntimeArgs,
    authorization_keys: BTreeSet<AccountHash>,
    proposer: PublicKey,
}

impl Default for ExecuteRequestBuilder {
    fn default() -> Self {
        ExecuteRequestBuilder {
            pre_state_hash: Self::DEFAULT_PRE_STATE_HASH,
            block_time: BlockTime::new(DEFAULT_BLOCK_TIME),
            transaction_hash: Self::DEFAULT_TRANSACTION_HASH,
            gas_price: DEFAULT_GAS_PRICE,
            initiator_addr: InitiatorAddr::PublicKey(DEFAULT_ACCOUNT_PUBLIC_KEY.clone()),
            payment: Self::DEFAULT_PAYMENT,
            payment_entry_point: Self::DEFAULT_PAYMENT_ENTRY_POINT.to_string(),
            payment_args: runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT },
            session: None,
            session_entry_point: Self::DEFAULT_SESSION_ENTRY_POINT.to_string(),
            session_args: RuntimeArgs::new(),
            authorization_keys: iter::once(*DEFAULT_ACCOUNT_ADDR).collect(),
            proposer: DEFAULT_PROPOSER_PUBLIC_KEY.clone(),
        }
    }
}

impl ExecuteRequestBuilder {
    /// The default value used for `ExecuteRequest::pre_state_hash`.
    pub const DEFAULT_PRE_STATE_HASH: Digest = Digest::from_raw([1; 32]);
    /// The default value used for `ExecuteRequest::transaction_hash`.
    pub const DEFAULT_TRANSACTION_HASH: TransactionHash =
        TransactionHash::V1(TransactionV1Hash::from_raw([2; 32]));
    /// The default value used for `ExecuteRequest::payment`.
    pub const DEFAULT_PAYMENT: Payment = Payment::Standard;
    /// The default value used for `ExecuteRequest::payment_entry_point`.
    pub const DEFAULT_PAYMENT_ENTRY_POINT: &'static str = "call";
    /// The default value used for `ExecuteRequest::session_entry_point`.
    pub const DEFAULT_SESSION_ENTRY_POINT: &'static str = "call";
    /// The default value used in `ExecuteRequest::authorization_keys`.
    pub const DEFAULT_AUTHORIZATION_KEY: AccountHash = AccountHash::new([3; 32]);

    /// Returns a new `ExecuteRequestBuilder` with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Converts a `DeployItem` into an `ExecuteRequestBuilder`.
    pub fn from_deploy_item(deploy_item: DeployItem) -> Self {
        let session_info =
            SessionInfo::try_from((deploy_item.session, DeployHash::default())).unwrap();
        let payment_info =
            PaymentInfo::try_from((deploy_item.payment, DeployHash::default())).unwrap();

        ExecuteRequestBuilder {
            transaction_hash: TransactionHash::Deploy(deploy_item.deploy_hash),
            gas_price: deploy_item.gas_price,
            initiator_addr: InitiatorAddr::AccountHash(deploy_item.address),
            payment: payment_info.payment,
            payment_entry_point: payment_info.entry_point,
            payment_args: payment_info.args,
            session: Some(session_info.session),
            session_entry_point: session_info.entry_point,
            session_args: session_info.args,
            authorization_keys: deploy_item.authorization_keys,
            ..Self::default()
        }
    }

    /// Returns an [`ExecuteRequest`] derived from a deploy with standard dependencies.
    pub fn standard(
        account_hash: AccountHash,
        session_file: &str,
        session_args: RuntimeArgs,
    ) -> Self {
        let deploy_item = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_code(session_file, session_args)
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => *DEFAULT_PAYMENT
            })
            .with_authorization_keys(&[account_hash])
            .build();
        Self::from_deploy_item(deploy_item)
    }

    /// Returns an [`ExecuteRequest`] derived from a deploy with session module bytes.
    pub fn module_bytes(
        account_hash: AccountHash,
        module_bytes: Vec<u8>,
        session_args: RuntimeArgs,
    ) -> Self {
        let deploy_item = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_bytes(module_bytes, session_args)
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => *DEFAULT_PAYMENT
            })
            .with_authorization_keys(&[account_hash])
            .build();
        Self::from_deploy_item(deploy_item)
    }

    /// Returns an [`ExecuteRequest`] derived from a deploy with a session item that will call a
    /// stored contract by hash.
    pub fn contract_call_by_hash(
        sender: AccountHash,
        contract_hash: AddressableEntityHash,
        entry_point: &str,
        args: RuntimeArgs,
    ) -> Self {
        let deploy_item = DeployItemBuilder::new()
            .with_address(sender)
            .with_stored_session_hash(contract_hash, entry_point, args)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[sender])
            .build();
        Self::from_deploy_item(deploy_item)
    }

    /// Returns an [`ExecuteRequest`] derived from a deploy with a session item that will call a
    /// stored contract by name.
    pub fn contract_call_by_name(
        sender: AccountHash,
        contract_name: &str,
        entry_point: &str,
        args: RuntimeArgs,
    ) -> Self {
        let deploy_item = DeployItemBuilder::new()
            .with_address(sender)
            .with_stored_session_named_key(contract_name, entry_point, args)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[sender])
            .build();
        Self::from_deploy_item(deploy_item)
    }

    /// Returns an [`ExecuteRequest`] derived from a deploy with a session item that will call a
    /// versioned stored contract by hash.
    pub fn versioned_contract_call_by_hash(
        sender: AccountHash,
        contract_package_hash: PackageHash,
        version: Option<EntityVersion>,
        entry_point_name: &str,
        args: RuntimeArgs,
    ) -> Self {
        let deploy_item = DeployItemBuilder::new()
            .with_address(sender)
            .with_stored_versioned_contract_by_hash(
                contract_package_hash.value(),
                version,
                entry_point_name,
                args,
            )
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[sender])
            .build();
        Self::from_deploy_item(deploy_item)
    }

    /// Returns an [`ExecuteRequest`] derived from a deploy with a session item that will call a
    /// versioned stored contract by name.
    pub fn versioned_contract_call_by_name(
        sender: AccountHash,
        contract_name: &str,
        version: Option<EntityVersion>,
        entry_point_name: &str,
        args: RuntimeArgs,
    ) -> Self {
        let deploy_item = DeployItemBuilder::new()
            .with_address(sender)
            .with_stored_versioned_contract_by_name(contract_name, version, entry_point_name, args)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[sender])
            .build();
        Self::from_deploy_item(deploy_item)
    }

    /// Sets the block time of the [`ExecuteRequest`].
    pub fn with_block_time(mut self, block_time: u64) -> Self {
        self.block_time = BlockTime::new(block_time);
        self
    }

    /// Sets the proposer used by the [`ExecuteRequest`].
    pub fn with_proposer(mut self, proposer: PublicKey) -> Self {
        self.proposer = proposer;
        self
    }

    /// Sets the authorization keys used by the [`ExecuteRequest`].
    pub fn with_authorization_keys(mut self, authorization_keys: BTreeSet<AccountHash>) -> Self {
        self.authorization_keys = authorization_keys;
        self
    }

    /// Consumes self and returns an [`ExecuteRequest`].
    pub fn build(self) -> ExecuteRequest {
        ExecuteRequest {
            pre_state_hash: self.pre_state_hash,
            block_time: self.block_time,
            transaction_hash: self.transaction_hash,
            gas_price: self.gas_price,
            initiator_addr: self.initiator_addr,
            payment: self.payment,
            payment_entry_point: self.payment_entry_point,
            payment_args: self.payment_args,
            session: self
                .session
                .expect("builder must have been provided with a valid session item"),
            session_entry_point: self.session_entry_point,
            session_args: self.session_args,
            authorization_keys: self.authorization_keys,
            proposer: self.proposer,
        }
    }

    /// Returns an [`ExecuteRequest`] for a native transfer.
    pub fn transfer(_sender: AccountHash, _transfer_args: RuntimeArgs) -> Self {
        todo!(
            "this should not be a part of Self - should maybe have an ExecuteNativeRequestBuilder"
        );
    }
}
