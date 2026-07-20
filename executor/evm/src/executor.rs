//! Public executor entry point.

use casper_storage::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    TrackingCopy,
};
use casper_types::{EvmConfig, EvmSpec, Key, StoredValue};
use revm::{
    context::CfgEnv,
    context_interface::result::{EVMError, ExecutionResult as RevmExecutionResult, ResultGas},
    primitives::{hardfork::SpecId, Bytes, U256},
    Context, ExecuteEvm, MainBuilder, MainContext, SystemCallEvm,
};

use crate::{
    db::CasperDb, precompiles::CasperEvmPrecompiles, state, tx, BlockHashProvider, DbError, Error,
    ExecuteKind, ExecuteRequest, ExecutionOutcome, NoBlockHashProvider, Result, SystemCallRequest,
};

/// Executes EVM transactions and calls against a Casper tracking copy.
///
/// The executor writes only to the supplied [`TrackingCopy`]. It never commits
/// global state; callers can pass a forked tracking copy for view execution or
/// commit the resulting effects through the normal Casper storage flow.
#[derive(Clone, Debug)]
pub struct EvmExecutor {
    config: EvmConfig,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EvmExecutionMode {
    Checked,
    UncheckedSimulation,
    SystemCall,
}

impl EvmExecutor {
    /// Creates a new executor from chainspec EVM configuration.
    pub fn new(config: EvmConfig) -> Self {
        Self { config }
    }

    /// Returns the immutable EVM configuration used by this executor.
    pub fn config(&self) -> &EvmConfig {
        &self.config
    }

    /// Executes an EVM transaction or call against the supplied tracking copy.
    pub fn execute<R>(
        &self,
        tracking_copy: &mut TrackingCopy<R>,
        request: ExecuteRequest,
    ) -> Result<ExecutionOutcome>
    where
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
    {
        let block_hash_provider = NoBlockHashProvider;
        self.execute_with_block_hash_provider(tracking_copy, request, &block_hash_provider)
    }

    /// Executes a system call against the supplied tracking copy.
    pub fn execute_system_call<R>(
        &self,
        tracking_copy: &mut TrackingCopy<R>,
        request: SystemCallRequest,
    ) -> Result<ExecutionOutcome>
    where
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
    {
        let block_hash_provider = NoBlockHashProvider;
        self.execute_system_call_with_block_hash_provider(
            tracking_copy,
            request,
            &block_hash_provider,
        )
    }

    /// Executes with a provider for historical block hashes.
    ///
    /// The provider is used by the EVM `BLOCKHASH` opcode. Current/future
    /// blocks and block numbers older than the EVM 256-block lookup window
    /// return the zero hash before the provider is consulted.
    pub fn execute_with_block_hash_provider<R, B>(
        &self,
        tracking_copy: &mut TrackingCopy<R>,
        request: ExecuteRequest,
        block_hash_provider: &B,
    ) -> Result<ExecutionOutcome>
    where
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
        B: BlockHashProvider + ?Sized,
    {
        if !self.config.enabled {
            return Err(Error::Disabled);
        }

        if let ExecuteKind::Transaction(transaction) = &request.kind {
            let Some(actual) = transaction.chain_id() else {
                return Err(Error::MissingChainId);
            };
            if actual != self.config.chain_id {
                return Err(Error::ChainIdMismatch {
                    expected: self.config.chain_id,
                    actual,
                });
            }
        }

        let tx_env = tx::build_tx_env(&self.config, &request.kind)?;
        let block = request.block.to_revm_block(&self.config)?;
        let execution_mode = match &request.kind {
            ExecuteKind::Transaction(_) => EvmExecutionMode::Checked,
            ExecuteKind::Call(call) if call.validation.is_unchecked_simulation() => {
                EvmExecutionMode::UncheckedSimulation
            }
            ExecuteKind::Call(_) => EvmExecutionMode::Checked,
        };

        let result_and_state = {
            let db = CasperDb::new(tracking_copy, block_hash_provider);
            let mut evm = Context::mainnet()
                .with_db(db)
                .with_block(block)
                .modify_cfg_chained(|cfg| {
                    configure_evm_cfg(cfg, &self.config, execution_mode);
                })
                .build_mainnet()
                .with_precompiles(CasperEvmPrecompiles::new(spec_id(self.config.spec)));

            evm.transact(tx_env).map_err(map_revm_error)?
        };

        let outcome = ExecutionOutcome::from_revm_result(&result_and_state.result);
        let mut state = result_and_state.state;
        // revm skips the upfront fee debit but still applies the
        // post-execution gas reimbursement and beneficiary reward.
        let disabled_fee_transfers =
            disabled_fee_transfers(&self.config, &request, &result_and_state.result);
        state::remove_disabled_fee_transfers(&mut state, disabled_fee_transfers)?;
        state::apply(tracking_copy, state)?;
        Ok(outcome)
    }

    /// Executes a system call with a provider for historical block hashes.
    pub fn execute_system_call_with_block_hash_provider<R, B>(
        &self,
        tracking_copy: &mut TrackingCopy<R>,
        request: SystemCallRequest,
        block_hash_provider: &B,
    ) -> Result<ExecutionOutcome>
    where
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
        B: BlockHashProvider + ?Sized,
    {
        if !self.config.enabled {
            return Err(Error::Disabled);
        }

        let block = request.block.to_revm_block(&self.config)?;
        let result_and_state = {
            let db = CasperDb::new(tracking_copy, block_hash_provider);
            let mut evm = Context::mainnet()
                .with_db(db)
                .with_block(block)
                .modify_cfg_chained(|cfg| {
                    configure_evm_cfg(cfg, &self.config, EvmExecutionMode::SystemCall);
                })
                .build_mainnet()
                .with_precompiles(CasperEvmPrecompiles::new(spec_id(self.config.spec)));

            evm.system_call(
                tx::to_revm_address(request.target),
                Bytes::from(request.input),
            )
            .map_err(map_revm_error)?
        };

        let outcome = ExecutionOutcome::from_revm_result(&result_and_state.result);
        state::apply(tracking_copy, result_and_state.state)?;
        Ok(outcome)
    }
}

fn configure_evm_cfg(cfg: &mut CfgEnv, config: &EvmConfig, execution_mode: EvmExecutionMode) {
    let skip_validation = matches!(
        execution_mode,
        EvmExecutionMode::UncheckedSimulation | EvmExecutionMode::SystemCall
    );

    cfg.spec = spec_id(config.spec);
    cfg.chain_id = config.chain_id;
    cfg.tx_chain_id_check = matches!(execution_mode, EvmExecutionMode::Checked);
    cfg.disable_block_gas_limit = matches!(execution_mode, EvmExecutionMode::SystemCall);
    cfg.disable_base_fee = skip_validation;
    cfg.disable_balance_check = skip_validation;
    cfg.disable_nonce_check = skip_validation;
    cfg.disable_fee_charge = true;
}

fn disabled_fee_transfers(
    config: &EvmConfig,
    request: &ExecuteRequest,
    result: &RevmExecutionResult,
) -> state::DisabledFeeTransfers {
    let gas = result_gas(result);
    let base_fee = request
        .block
        .base_fee
        .unwrap_or_else(|| config.base_fee_wei());
    let (caller, gas_limit, effective_gas_price) = match &request.kind {
        ExecuteKind::Transaction(transaction) => (
            tx::to_revm_address(transaction.from()),
            transaction.gas_limit(),
            transaction.effective_gas_price(base_fee),
        ),
        ExecuteKind::Call(call) => (
            tx::to_revm_address(call.from),
            call.gas_limit,
            call.gas_price,
        ),
    };

    let reimbursed_gas = gas_limit
        .saturating_sub(gas.total_gas_spent())
        .saturating_add(gas.inner_refunded());
    let caller_reimbursement = U256::from(effective_gas_price) * U256::from(reimbursed_gas);
    let coinbase_gas_price = effective_gas_price.saturating_sub(base_fee);
    let beneficiary_reward = U256::from(coinbase_gas_price) * U256::from(gas.tx_gas_used());

    state::DisabledFeeTransfers {
        caller,
        caller_reimbursement,
        beneficiary: tx::to_revm_address(request.block.beneficiary),
        beneficiary_reward,
    }
}

fn result_gas(result: &RevmExecutionResult) -> &ResultGas {
    match result {
        RevmExecutionResult::Success { gas, .. }
        | RevmExecutionResult::Revert { gas, .. }
        | RevmExecutionResult::Halt { gas, .. } => gas,
    }
}

fn spec_id(spec: EvmSpec) -> SpecId {
    match spec {
        EvmSpec::Prague => SpecId::PRAGUE,
    }
}

fn map_revm_error(error: EVMError<DbError>) -> Error {
    match error {
        EVMError::Database(error) => Error::Database(error),
        other => Error::Revm(other.to_string()),
    }
}
