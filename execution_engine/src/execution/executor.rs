use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

use casper_storage::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    tracking_copy::TrackingCopy,
    AddressGenerator,
};
use casper_types::{
    account::AccountHash, contract_messages::Messages, contracts::NamedKeys, execution::Effects,
    ContextAccessRights, EntityAddr, EntryPointType, Gas, Key, Phase, RuntimeArgs,
    RuntimeFootprint, StoredValue, TransactionHash, U512,
};

use crate::{
    engine_state::{execution_kind::ExecutionKind, BlockInfo, EngineConfig, WasmV1Result},
    execution::ExecError,
    runtime::{Runtime, RuntimeStack},
    runtime_context::{AllowInstallUpgrade, RuntimeContext},
};

const ARG_AMOUNT: &str = "amount";

fn try_get_amount(runtime_args: &RuntimeArgs) -> Result<U512, ExecError> {
    runtime_args
        .try_get_number(ARG_AMOUNT)
        .map_err(ExecError::from)
}

/// Executor object deals with execution of WASM modules.
pub struct Executor {
    config: EngineConfig,
}

impl Executor {
    /// Creates new executor object.
    pub fn new(config: EngineConfig) -> Self {
        Executor { config }
    }

    /// Executes a WASM module.
    ///
    /// This method checks if a given contract hash is a system contract, and then short circuits to
    /// a specific native implementation of it. Otherwise, a supplied WASM module is executed.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn exec<R>(
        &self,
        execution_kind: ExecutionKind,
        args: RuntimeArgs,
        entity_addr: EntityAddr,
        runtime_footprint: Rc<RefCell<RuntimeFootprint>>,
        named_keys: &mut NamedKeys,
        access_rights: ContextAccessRights,
        authorization_keys: BTreeSet<AccountHash>,
        account_hash: AccountHash,
        block_info: BlockInfo,
        txn_hash: TransactionHash,
        gas_limit: Gas,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
        phase: Phase,
        stack: RuntimeStack,
    ) -> WasmV1Result
    where
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
    {
        let spending_limit: U512 = match try_get_amount(&args) {
            Ok(spending_limit) => spending_limit,
            Err(error) => {
                return WasmV1Result::new(
                    gas_limit,
                    Gas::zero(),
                    Effects::default(),
                    Vec::default(),
                    Vec::default(),
                    Some(error.into()),
                    None,
                    None,
                );
            }
        };

        let address_generator = {
            let generator = AddressGenerator::new(txn_hash.as_ref(), phase);
            Rc::new(RefCell::new(generator))
        };

        let context_key = if self.config.enable_entity {
            Key::AddressableEntity(entity_addr)
        } else {
            match entity_addr {
                EntityAddr::System(hash)
                | EntityAddr::SmartContract(hash)
                | EntityAddr::Package(hash) => Key::Hash(hash),
                EntityAddr::Account(hash) => Key::Account(AccountHash::new(hash)),
            }
        };

        let allow_install_upgrade = match execution_kind {
            ExecutionKind::InstallerUpgrader(_)
            | ExecutionKind::Stored { .. }
            | ExecutionKind::VersionedCall { .. }
            | ExecutionKind::Deploy(_) => AllowInstallUpgrade::Allowed,
            ExecutionKind::Standard(_) => AllowInstallUpgrade::Forbidden,
        };

        let context = self.create_runtime_context(
            named_keys,
            runtime_footprint,
            context_key,
            authorization_keys,
            access_rights,
            account_hash,
            address_generator,
            tracking_copy,
            block_info,
            txn_hash,
            phase,
            args.clone(),
            gas_limit,
            spending_limit,
            EntryPointType::Caller,
            allow_install_upgrade,
        );

        let mut runtime = Runtime::new(context);

        let result = match execution_kind {
            ExecutionKind::Standard(module_bytes)
            | ExecutionKind::InstallerUpgrader(module_bytes)
            | ExecutionKind::Deploy(module_bytes) => {
                runtime.execute_module_bytes(module_bytes, stack)
            }
            ExecutionKind::Stored {
                entity_hash,
                entry_point,
            } => {
                // These args are passed through here as they are required to construct the new
                // `Runtime` during the contract's execution (i.e. inside
                // `Runtime::execute_contract`).
                runtime.call_contract_with_stack(entity_hash, &entry_point, args, stack)
            }
            ExecutionKind::VersionedCall {
                package_hash,
                entity_version,
                protocol_version_major,
                entry_point,
            } => runtime.call_package_version_with_stack(
                package_hash,
                protocol_version_major,
                entity_version,
                entry_point,
                args,
                stack,
            ),
        };
        match result {
            Ok(ret) => WasmV1Result::new(
                gas_limit,
                runtime.context().gas_counter(),
                runtime.context().effects(),
                runtime.context().transfers().to_owned(),
                runtime.context().messages(),
                None,
                Some(ret),
                Some(runtime.context().cache()),
            ),
            Err(error) => WasmV1Result::new(
                gas_limit,
                runtime.context().gas_counter(),
                Effects::new(),
                vec![],
                Messages::new(),
                Some(error.into()),
                None,
                None,
            ),
        }
    }

    /// Creates new runtime context.
    #[allow(clippy::too_many_arguments)]
    fn create_runtime_context<'a, R>(
        &self,
        named_keys: &'a mut NamedKeys,
        runtime_footprint: Rc<RefCell<RuntimeFootprint>>,
        context_key: Key,
        authorization_keys: BTreeSet<AccountHash>,
        access_rights: ContextAccessRights,
        account_hash: AccountHash,
        address_generator: Rc<RefCell<AddressGenerator>>,
        tracking_copy: Rc<RefCell<TrackingCopy<R>>>,
        block_info: BlockInfo,
        txn_hash: TransactionHash,
        phase: Phase,
        runtime_args: RuntimeArgs,
        gas_limit: Gas,
        remaining_spending_limit: U512,
        entry_point_type: EntryPointType,
        allow_install_upgrade: AllowInstallUpgrade,
    ) -> RuntimeContext<'a, R>
    where
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
    {
        let gas_counter = Gas::default();
        let transfers = Vec::default();

        RuntimeContext::new(
            named_keys,
            runtime_footprint,
            context_key,
            authorization_keys,
            access_rights,
            account_hash,
            address_generator,
            tracking_copy,
            self.config.clone(),
            block_info,
            txn_hash,
            phase,
            runtime_args,
            gas_limit,
            gas_counter,
            transfers,
            remaining_spending_limit,
            entry_point_type,
            allow_install_upgrade,
        )
    }
}
