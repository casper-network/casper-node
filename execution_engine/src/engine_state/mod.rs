//!  This module contains all the execution related code.
pub mod deploy_item;
pub mod engine_config;
mod error;
pub(crate) mod execution_kind;
pub mod execution_result;
mod wasm_v1;

use std::{cell::RefCell, rc::Rc};

use once_cell::sync::Lazy;

use casper_storage::{
    global_state::state::StateProvider,
    tracking_copy::{TrackingCopyEntityExt, TrackingCopyError, TrackingCopyExt},
};
use casper_types::{ProtocolVersion, U512};

use crate::{execution::Executor, runtime::RuntimeStack};
pub use deploy_item::DeployItem;
pub use engine_config::{
    EngineConfig, EngineConfigBuilder, DEFAULT_MAX_QUERY_DEPTH,
    DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT,
};
pub use error::Error;
use execution_kind::ExecutionKind;
pub use execution_result::{ExecutionResult, ForcedTransferResult};
pub use wasm_v1::{ExecutableItem, InvalidRequest, WasmV1Request, WasmV1Result};

/// The maximum amount of motes that payment code execution can cost.
pub const MAX_PAYMENT_AMOUNT: u64 = 2_500_000_000;
/// The maximum amount of gas a payment code can use.
///
/// This value also indicates the minimum balance of the main purse of an account when
/// executing payment code, as such amount is held as collateral to compensate for
/// code execution.
pub static MAX_PAYMENT: Lazy<U512> = Lazy::new(|| U512::from(MAX_PAYMENT_AMOUNT));

/// Gas/motes conversion rate of wasmless transfer cost is always 1 regardless of what user wants to
/// pay.
pub const WASMLESS_TRANSFER_FIXED_GAS_PRICE: u64 = 1;

/// Main implementation of an execution engine state.
///
/// Takes an engine's configuration and a provider of a state (aka the global state) to operate on.
/// Methods implemented on this structure are the external API intended to be used by the users such
/// as the node, test framework, and others.
#[derive(Debug, Clone, Default)]
pub struct ExecutionEngineV1 {
    config: EngineConfig,
}

impl ExecutionEngineV1 {
    /// Creates new execution engine.
    pub fn new(config: EngineConfig) -> ExecutionEngineV1 {
        ExecutionEngineV1 { config }
    }

    /// Returns engine config.
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Sets the protocol version of the config.
    ///
    /// NOTE: This is only useful to the WasmTestBuilder for emulating a network upgrade, and hence
    /// is subject to change or deletion without notice.
    #[doc(hidden)]
    pub fn set_protocol_version(&mut self, protocol_version: ProtocolVersion) {
        self.config.set_protocol_version(protocol_version)
    }

    /// Executes wasm, and that's all. Does not commit or handle payment or anything else.
    pub fn execute(
        &self,
        state_provider: &impl StateProvider,
        WasmV1Request {
            state_hash,
            block_time,
            transaction_hash,
            gas_limit,
            initiator_addr,
            executable_item,
            entry_point,
            args,
            authorization_keys,
        }: WasmV1Request,
    ) -> WasmV1Result {
        let account_hash = initiator_addr.account_hash();
        let protocol_version = self.config.protocol_version();
        let tc = match state_provider.tracking_copy(state_hash) {
            Ok(Some(tracking_copy)) => Rc::new(RefCell::new(tracking_copy)),
            Ok(None) => return WasmV1Result::root_not_found(gas_limit, state_hash),
            Err(gse) => {
                return WasmV1Result::precondition_failure(
                    gas_limit,
                    Error::TrackingCopy(TrackingCopyError::Storage(gse)),
                )
            }
        };
        let (entity, entity_hash) = {
            match tc.borrow_mut().get_authorized_addressable_entity(
                protocol_version,
                account_hash,
                &authorization_keys,
                &self.config().administrative_accounts,
            ) {
                Ok((addressable_entity, entity_hash)) => (addressable_entity, entity_hash),
                Err(tce) => {
                    return WasmV1Result::precondition_failure(gas_limit, Error::TrackingCopy(tce))
                }
            }
        };
        let mut named_keys = match tc
            .borrow_mut()
            .get_named_keys(entity.entity_addr(entity_hash))
            .map_err(Into::into)
        {
            Ok(named_keys) => named_keys,
            Err(tce) => {
                return WasmV1Result::precondition_failure(gas_limit, Error::TrackingCopy(tce))
            }
        };
        let phase = executable_item.phase();
        let execution_kind = match ExecutionKind::new(
            &mut *tc.borrow_mut(),
            &named_keys,
            &executable_item,
            entry_point,
            protocol_version,
        ) {
            Ok(execution_kind) => execution_kind,
            Err(ese) => return WasmV1Result::precondition_failure(gas_limit, ese),
        };

        let access_rights = entity.extract_access_rights(entity_hash, &named_keys);
        let execution_result = Executor::new(self.config().clone()).exec(
            execution_kind,
            args,
            entity_hash,
            &entity,
            &mut named_keys,
            access_rights,
            authorization_keys,
            account_hash,
            block_time,
            transaction_hash,
            gas_limit,
            protocol_version,
            Rc::clone(&tc),
            phase,
            RuntimeStack::from_account_hash(
                account_hash,
                self.config.max_runtime_call_stack_height() as usize,
            ),
        );

        WasmV1Result::from_execution_result(gas_limit, execution_result)
    }
}
