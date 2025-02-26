//!  This module contains all the execution related code.
pub mod engine_config;
mod error;
pub(crate) mod execution_kind;
mod wasm_v1;

use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

use casper_types::{
    account::AccountHash, Gas, InitiatorAddr, Key, Phase, RuntimeArgs, StoredValue, TransactionHash,
};

use casper_storage::{
    global_state::{
        error::Error as GlobalStateError,
        state::{StateProvider, StateReader},
    },
    tracking_copy::{TrackingCopyEntityExt, TrackingCopyError},
    TrackingCopy,
};

use crate::{execution::Executor, runtime::RuntimeStack};
pub use engine_config::{
    EngineConfig, EngineConfigBuilder, DEFAULT_MAX_QUERY_DEPTH,
    DEFAULT_MAX_RUNTIME_CALL_STACK_HEIGHT,
};
pub use error::Error;
use execution_kind::ExecutionKind;
pub use wasm_v1::{
    BlockInfo, ExecutableItem, InvalidRequest, SessionDataDeploy, SessionDataV1, SessionInputData,
    WasmV1Request, WasmV1Result,
};

/// Gas/motes conversion rate of wasmless transfer cost is always 1 regardless of what user wants to
/// pay.
pub const WASMLESS_TRANSFER_FIXED_GAS_PRICE: u8 = 1;

/// The public api of the v1 execution engine, as of protocol version 2.0.0
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

    /// Executes wasm, and that's all. Does not commit or handle payment or anything else.
    pub fn execute(
        &self,
        state_provider: &impl StateProvider,
        wasm_v1_request: WasmV1Request,
    ) -> WasmV1Result {
        let WasmV1Request {
            block_info,
            transaction_hash,
            gas_limit,
            initiator_addr,
            executable_item,
            entry_point,
            args,
            authorization_keys,
            phase,
        } = wasm_v1_request;
        // NOTE to core engineers: it is intended for the EE to ONLY execute wasm targeting the
        // casper v1 virtual machine. it should not handle native behavior, database / global state
        // interaction, payment processing, or anything other than its single function.
        // A good deal of effort has been put into removing all such behaviors; please do not
        // come along and start adding it back.

        let account_hash = initiator_addr.account_hash();
        let protocol_version = self.config.protocol_version();
        let state_hash = block_info.state_hash;
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
        let (runtime_footprint, entity_addr) = {
            match tc.borrow_mut().authorized_runtime_footprint_by_account(
                protocol_version,
                account_hash,
                &authorization_keys,
                &self.config().administrative_accounts,
            ) {
                Ok((runtime_footprint, entity_hash)) => (runtime_footprint, entity_hash),
                Err(tce) => {
                    return WasmV1Result::precondition_failure(gas_limit, Error::TrackingCopy(tce))
                }
            }
        };
        let mut named_keys = runtime_footprint.named_keys().clone();
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
        let access_rights = runtime_footprint.extract_access_rights(entity_addr.value());
        Executor::new(self.config().clone()).exec(
            execution_kind,
            args,
            entity_addr,
            Rc::new(RefCell::new(runtime_footprint)),
            &mut named_keys,
            access_rights,
            authorization_keys,
            account_hash,
            block_info,
            transaction_hash,
            gas_limit,
            Rc::clone(&tc),
            phase,
            RuntimeStack::from_account_hash(
                account_hash,
                self.config.max_runtime_call_stack_height() as usize,
            ),
        )
    }

    /// Executes wasm, and that's all. Does not commit or handle payment or anything else.
    #[allow(clippy::too_many_arguments)]
    pub fn execute_with_tracking_copy<R>(
        &self,
        tracking_copy: TrackingCopy<R>,
        block_info: BlockInfo,
        transaction_hash: TransactionHash,
        gas_limit: Gas,
        initiator_addr: InitiatorAddr,
        executable_item: ExecutableItem,
        entry_point: String,
        args: RuntimeArgs,
        authorization_keys: BTreeSet<AccountHash>,
        phase: Phase,
    ) -> WasmV1Result
    where
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
    {
        // NOTE to core engineers: it is intended for the EE to ONLY execute wasm targeting the
        // casper v1 virtual machine. it should not handle native behavior, database / global state
        // interaction, payment processing, or anything other than its single function.
        // A good deal of effort has been put into removing all such behaviors; please do not
        // come along and start adding it back.

        let account_hash = initiator_addr.account_hash();
        let protocol_version = self.config.protocol_version();
        let tc = Rc::new(RefCell::new(tracking_copy));
        let (runtime_footprint, entity_addr) = {
            match tc.borrow_mut().authorized_runtime_footprint_by_account(
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
        let mut named_keys = runtime_footprint.named_keys().clone();
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
        let access_rights = runtime_footprint.extract_access_rights(entity_addr.value());
        Executor::new(self.config().clone()).exec(
            execution_kind,
            args,
            entity_addr,
            Rc::new(RefCell::new(runtime_footprint)),
            &mut named_keys,
            access_rights,
            authorization_keys,
            account_hash,
            block_info,
            transaction_hash,
            gas_limit,
            Rc::clone(&tc),
            phase,
            RuntimeStack::from_account_hash(
                account_hash,
                self.config.max_runtime_call_stack_height() as usize,
            ),
        )
    }
}
