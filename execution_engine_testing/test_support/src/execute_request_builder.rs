use std::collections::BTreeSet;

use casper_execution_engine::engine_state::{
    deploy_item::DeployItem, ExecutableItem, WasmV1Request,
};
use casper_types::{
    account::AccountHash, addressable_entity::DEFAULT_ENTRY_POINT_NAME, runtime_args,
    AddressableEntityHash, BlockTime, Digest, EntityVersion, Gas, InitiatorAddr, PackageHash,
    Phase, RuntimeArgs, Transaction, TransactionHash, TransactionV1Hash,
};

use crate::{DeployItemBuilder, ARG_AMOUNT, DEFAULT_BLOCK_TIME, DEFAULT_PAYMENT};

/// A request comprising a [`WasmV1Request`] for use as session code, and an optional custom
/// payment `WasmV1Request`.
#[derive(Debug)]
pub struct ExecuteRequest {
    /// The session request.
    pub session: WasmV1Request,
    /// The optional custom payment request.
    pub custom_payment: Option<WasmV1Request>,
}

/// Builds an [`ExecuteRequest`].
#[derive(Debug)]
pub struct ExecuteRequestBuilder {
    state_hash: Digest,
    block_time: BlockTime,
    transaction_hash: TransactionHash,
    initiator_addr: InitiatorAddr,
    payment: Option<ExecutableItem>,
    payment_gas_limit: Gas,
    payment_entry_point: String,
    payment_args: RuntimeArgs,
    session: ExecutableItem,
    session_gas_limit: Gas,
    session_entry_point: String,
    session_args: RuntimeArgs,
    authorization_keys: BTreeSet<AccountHash>,
}

impl ExecuteRequestBuilder {
    /// The default value used for `WasmV1Request::state_hash`.
    pub const DEFAULT_STATE_HASH: Digest = Digest::from_raw([1; 32]);
    /// The default value used for `WasmV1Request::transaction_hash`.
    pub const DEFAULT_TRANSACTION_HASH: TransactionHash =
        TransactionHash::V1(TransactionV1Hash::from_raw([2; 32]));
    /// The default value used for `WasmV1Request::entry_point`.
    pub const DEFAULT_ENTRY_POINT: &'static str = "call";

    /// Converts a `Transaction` into an `ExecuteRequestBuilder`.
    pub fn from_transaction(txn: &Transaction) -> Self {
        let authorization_keys = txn.authorization_keys();
        let session = WasmV1Request::new_session(
            Self::DEFAULT_STATE_HASH,
            BlockTime::new(DEFAULT_BLOCK_TIME),
            Gas::new(5_000_000_000_000_u64), // TODO - set proper value
            txn,
        )
        .unwrap();

        let payment: Option<ExecutableItem>;
        let payment_gas_limit: Gas;
        let payment_entry_point: String;
        let payment_args: RuntimeArgs;
        if txn.is_standard_payment() {
            payment = None;
            payment_gas_limit = Gas::zero();
            payment_entry_point = DEFAULT_ENTRY_POINT_NAME.to_string();
            payment_args = RuntimeArgs::new();
        } else {
            let request = WasmV1Request::new_custom_payment(
                Self::DEFAULT_STATE_HASH,
                BlockTime::new(DEFAULT_BLOCK_TIME),
                Gas::new(5_000_000_000_000_u64), // TODO - set proper value
                txn,
            )
            .unwrap();
            payment = Some(request.executable_item);
            payment_gas_limit = request.gas_limit;
            payment_entry_point = request.entry_point;
            payment_args = request.args;
        }

        ExecuteRequestBuilder {
            state_hash: session.state_hash,
            block_time: session.block_time,
            transaction_hash: session.transaction_hash,
            initiator_addr: session.initiator_addr,
            payment,
            payment_gas_limit,
            payment_entry_point,
            payment_args,
            session: session.executable_item,
            session_gas_limit: session.gas_limit,
            session_entry_point: session.entry_point,
            session_args: session.args,
            authorization_keys,
        }
    }

    /// Converts a `DeployItem` into an `ExecuteRequestBuilder`.
    pub fn from_deploy_item(deploy_item: &DeployItem) -> Self {
        let authorization_keys = deploy_item.authorization_keys.clone();
        let session = WasmV1Request::new_session_from_deploy_item(
            Self::DEFAULT_STATE_HASH,
            BlockTime::new(DEFAULT_BLOCK_TIME),
            Gas::new(5_000_000_000_000_u64), // TODO - set proper value
            deploy_item,
        )
        .unwrap();

        let payment: Option<ExecutableItem>;
        let payment_gas_limit: Gas;
        let payment_entry_point: String;
        let payment_args: RuntimeArgs;
        if deploy_item.payment.is_standard_payment(Phase::Payment) {
            payment = None;
            payment_gas_limit = Gas::zero();
            payment_entry_point = DEFAULT_ENTRY_POINT_NAME.to_string();
            payment_args = RuntimeArgs::new();
        } else {
            let request = WasmV1Request::new_custom_payment_from_deploy_item(
                Self::DEFAULT_STATE_HASH,
                BlockTime::new(DEFAULT_BLOCK_TIME),
                Gas::new(5_000_000_000_000_u64), // TODO - set proper value
                deploy_item,
            )
            .unwrap();
            payment = Some(request.executable_item);
            payment_gas_limit = request.gas_limit;
            payment_entry_point = request.entry_point;
            payment_args = request.args;
        }

        ExecuteRequestBuilder {
            state_hash: session.state_hash,
            block_time: session.block_time,
            transaction_hash: session.transaction_hash,
            initiator_addr: session.initiator_addr,
            payment,
            payment_gas_limit,
            payment_entry_point,
            payment_args,
            session: session.executable_item,
            session_gas_limit: session.gas_limit,
            session_entry_point: session.entry_point,
            session_args: session.args,
            authorization_keys,
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
            .with_standard_payment(runtime_args! {
                ARG_AMOUNT => *DEFAULT_PAYMENT
            })
            .with_authorization_keys(&[account_hash])
            .build();
        Self::from_deploy_item(&deploy_item)
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
            .with_standard_payment(runtime_args! {
                ARG_AMOUNT => *DEFAULT_PAYMENT
            })
            .with_authorization_keys(&[account_hash])
            .build();
        Self::from_deploy_item(&deploy_item)
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
            .with_standard_payment(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[sender])
            .build();
        Self::from_deploy_item(&deploy_item)
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
            .with_standard_payment(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[sender])
            .build();
        Self::from_deploy_item(&deploy_item)
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
            .with_standard_payment(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[sender])
            .build();
        Self::from_deploy_item(&deploy_item)
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
            .with_standard_payment(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_authorization_keys(&[sender])
            .build();
        Self::from_deploy_item(&deploy_item)
    }

    /// Sets the block time of the [`WasmV1Request`]s.
    pub fn with_block_time<T: Into<BlockTime>>(mut self, block_time: T) -> Self {
        self.block_time = block_time.into();
        self
    }

    /// Sets the authorization keys used by the [`WasmV1Request`]s.
    pub fn with_authorization_keys(mut self, authorization_keys: BTreeSet<AccountHash>) -> Self {
        self.authorization_keys = authorization_keys;
        self
    }

    /// Consumes self and returns an `ExecuteRequest`.
    pub fn build(self) -> ExecuteRequest {
        let ExecuteRequestBuilder {
            state_hash,
            block_time,
            transaction_hash,
            initiator_addr,
            payment,
            payment_gas_limit,
            payment_entry_point,
            payment_args,
            session,
            session_gas_limit,
            session_entry_point,
            session_args,
            authorization_keys,
        } = self;

        let maybe_custom_payment = payment.map(|executable_item| WasmV1Request {
            state_hash,
            block_time,
            transaction_hash,
            gas_limit: payment_gas_limit,
            initiator_addr: initiator_addr.clone(),
            executable_item,
            entry_point: payment_entry_point,
            args: payment_args,
            authorization_keys: authorization_keys.clone(),
            phase: Phase::Payment,
        });

        let session = WasmV1Request {
            state_hash,
            block_time,
            transaction_hash,
            gas_limit: session_gas_limit,
            initiator_addr,
            executable_item: session,
            entry_point: session_entry_point,
            args: session_args,
            authorization_keys,
            phase: Phase::Session,
        };

        ExecuteRequest {
            session,
            custom_payment: maybe_custom_payment,
        }
    }
}
