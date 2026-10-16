//! Public executor entry point.

use std::collections::BTreeMap;

use casper_storage::{
    data_access_layer::DataAccessLayer,
    global_state::{error::Error as GlobalStateError, state::StateReader},
    TrackingCopy,
};
use casper_types::{EvmConfig, EvmSpec, EvmTransaction, EvmTransactionError, Key, StoredValue};
use revm::{
    context::CfgEnv,
    context_interface::result::{EVMError, InvalidTransaction as RevmInvalidTransaction},
    handler::{Handler, MainnetHandler},
    primitives::{hardfork::SpecId, Bytes},
    Context, ExecuteEvm, MainBuilder, MainContext, SystemCallEvm,
};

use crate::{
    db::CasperDb, precompiles::CasperEvmPrecompiles, state, tx, BlockContext, DbError, Error,
    ExecuteKind, ExecuteRequest, ExecutionOutcome, Result, SystemCallRequest,
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

    /// Validates an EVM transaction without executing it or applying state changes.
    pub fn validate_transaction<R, S>(
        &self,
        data_access_layer: &DataAccessLayer<S>,
        tracking_copy: &mut TrackingCopy<R>,
        block_context: BlockContext,
        transaction: &EvmTransaction,
    ) -> Result<()>
    where
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
    {
        if !self.config.enabled {
            return Err(Error::Disabled);
        }
        if self.config.wei_per_mote == 0 {
            return Err(Error::InvalidWeiPerMote);
        }

        let Some(actual) = transaction.chain_id() else {
            return Err(Error::MissingChainId);
        };
        if actual != self.config.chain_id {
            return Err(Error::ChainIdMismatch {
                expected: self.config.chain_id,
                actual,
            });
        }

        let kind = ExecuteKind::Transaction(Box::new(transaction.clone()));
        let tx_env = tx::build_tx_env(&self.config, &kind)?;
        let block = block_context.to_revm_block(&self.config)?;
        let db = CasperDb::new(data_access_layer, tracking_copy, self.config.wei_per_mote);
        let mut evm = Context::mainnet()
            .with_db(db)
            .with_block(block)
            .with_tx(tx_env)
            .modify_cfg_chained(|cfg| {
                configure_evm_cfg(cfg, &self.config, EvmExecutionMode::Checked);
            })
            .build_mainnet()
            .with_precompiles(CasperEvmPrecompiles::new(spec_id(self.config.spec)));

        // revm's validate() includes the caller state phase. Running that phase
        // again would bump a call transaction's nonce twice in this journal.
        // The journal is discarded with this temporary EVM.
        let handler = MainnetHandler::default();
        handler
            .validate(&mut evm)
            .map(|_| ())
            .map_err(map_revm_validation_error)
    }

    /// Names and addresses from this executor's selected precompile provider.
    /// Disabled EVM configurations expose no active precompiles.
    pub fn precompile_addresses(&self) -> BTreeMap<String, casper_types::evm::Address> {
        if !self.config.enabled {
            return BTreeMap::new();
        }
        CasperEvmPrecompiles::new(spec_id(self.config.spec)).addresses()
    }

    /// Executes an EVM transaction or call against the supplied tracking copy.
    pub fn execute<R, S>(
        &self,
        data_access_layer: &DataAccessLayer<S>,
        tracking_copy: &mut TrackingCopy<R>,
        request: ExecuteRequest,
    ) -> Result<ExecutionOutcome>
    where
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
    {
        if !self.config.enabled {
            return Err(Error::Disabled);
        }
        if self.config.wei_per_mote == 0 {
            return Err(Error::InvalidWeiPerMote);
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
            let db = CasperDb::new(data_access_layer, tracking_copy, self.config.wei_per_mote);
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

        let balance_losses = state::apply(
            tracking_copy,
            result_and_state.state,
            self.config.wei_per_mote,
        )?;
        Ok(ExecutionOutcome::from_revm_result(
            &result_and_state.result,
            balance_losses,
            self.config.wei_per_mote,
        ))
    }

    /// Executes a system call against the supplied tracking copy.
    pub fn execute_system_call<R, S>(
        &self,
        data_access_layer: &DataAccessLayer<S>,
        tracking_copy: &mut TrackingCopy<R>,
        request: SystemCallRequest,
    ) -> Result<ExecutionOutcome>
    where
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
    {
        if !self.config.enabled {
            return Err(Error::Disabled);
        }
        if self.config.wei_per_mote == 0 {
            return Err(Error::InvalidWeiPerMote);
        }

        let block = request.block.to_revm_block(&self.config)?;
        let result_and_state = {
            let db = CasperDb::new(data_access_layer, tracking_copy, self.config.wei_per_mote);
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

        let balance_losses = state::apply(
            tracking_copy,
            result_and_state.state,
            self.config.wei_per_mote,
        )?;
        Ok(ExecutionOutcome::from_revm_result(
            &result_and_state.result,
            balance_losses,
            self.config.wei_per_mote,
        ))
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
    // Read-only simulations may consume the block budget, following eth_call.
    // All checked execution retains revm's Osaka transaction cap.
    cfg.tx_gas_limit_cap = skip_validation.then_some(u64::MAX);
    cfg.disable_block_gas_limit = matches!(execution_mode, EvmExecutionMode::SystemCall);
    cfg.disable_base_fee = skip_validation;
    cfg.disable_balance_check = skip_validation;
    cfg.disable_nonce_check = skip_validation;
    cfg.disable_fee_charge = true;
}

fn spec_id(spec: EvmSpec) -> SpecId {
    match spec {
        EvmSpec::Osaka => SpecId::OSAKA,
    }
}

fn map_revm_error(error: EVMError<DbError>) -> Error {
    match error {
        EVMError::Database(error) => Error::Database(error),
        other => Error::Revm(other.to_string()),
    }
}

fn map_revm_validation_error(error: EVMError<DbError>) -> Error {
    match error {
        EVMError::Transaction(
            RevmInvalidTransaction::NonceTooHigh { tx, state }
            | RevmInvalidTransaction::NonceTooLow { tx, state },
        ) => Error::InvalidTransaction(EvmTransactionError::InvalidNonce {
            expected: state,
            actual: tx,
        }),
        EVMError::Transaction(error) => {
            Error::InvalidTransaction(EvmTransactionError::Validation(error.to_string()))
        }
        other => map_revm_error(other),
    }
}
