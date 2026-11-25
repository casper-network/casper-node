//! Public executor entry point.

use std::collections::BTreeMap;

use casper_storage::{
    data_access_layer::DataAccessLayer,
    global_state::{error::Error as GlobalStateError, state::StateReader},
    TrackingCopy,
};
use casper_types::{EvmConfig, EvmSpec, EvmTransactionError, Key, StoredValue};
use revm::{
    context::{CfgEnv, Evm},
    context_interface::result::{EVMError, InvalidTransaction as RevmInvalidTransaction},
    handler::{instructions::EthInstructions, EthFrame, Handler, MainnetContext, MainnetHandler},
    interpreter::interpreter::EthInterpreter,
    primitives::hardfork::SpecId,
    Context, ExecuteEvm, MainBuilder, MainContext,
};

use crate::{
    db::CasperDb, precompiles::CasperEvmPrecompiles, state, tx, CallValidation, DbError, Error,
    ExecuteKind, ExecuteRequest, ExecutionOutcome, Result,
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

type CasperEvm<CTX> = Evm<
    CTX,
    (),
    EthInstructions<EthInterpreter, CTX>,
    CasperEvmPrecompiles,
    EthFrame<EthInterpreter>,
>;

impl EvmExecutor {
    /// Creates a new executor from chainspec EVM configuration.
    pub fn new(config: EvmConfig) -> Self {
        Self { config }
    }

    /// Returns the immutable EVM configuration used by this executor.
    pub fn config(&self) -> &EvmConfig {
        &self.config
    }

    /// Validates an EVM transaction or call without executing it or applying state changes.
    pub fn validate_transaction<R, S>(
        &self,
        data_access_layer: &DataAccessLayer<S>,
        tracking_copy: &mut TrackingCopy<R>,
        request: ExecuteRequest,
    ) -> Result<()>
    where
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
    {
        let mut evm = self.prepare_evm(data_access_layer, tracking_copy, &request)?;

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
        let result_and_state = {
            let mut evm = self.prepare_evm(data_access_layer, tracking_copy, &request)?;
            evm.replay().map_err(map_revm_error)?
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

    fn prepare_evm<'a, R, S>(
        &self,
        data_access_layer: &'a DataAccessLayer<S>,
        tracking_copy: &'a mut TrackingCopy<R>,
        request: &ExecuteRequest,
    ) -> Result<CasperEvm<MainnetContext<CasperDb<'a, R, S>>>>
    where
        R: StateReader<Key, StoredValue, Error = GlobalStateError>,
    {
        if !self.config.enabled {
            return Err(Error::Disabled);
        }
        if self.config.wei_per_mote == 0 {
            return Err(Error::InvalidWeiPerMote);
        }

        let tx_env = tx::build_tx_env(&self.config, &request.kind)?;
        let block = request.block.to_revm_block(&self.config)?;
        let validation = match &request.kind {
            ExecuteKind::Transaction(_) => CallValidation::Checked,
            ExecuteKind::Call(call) => call.validation,
        };
        let db = CasperDb::new(data_access_layer, tracking_copy, self.config.wei_per_mote);
        Ok(Context::mainnet()
            .with_db(db)
            .with_block(block)
            .with_tx(tx_env)
            .modify_cfg_chained(|cfg| {
                configure_evm_cfg(cfg, &self.config, validation);
            })
            .build_mainnet()
            .with_precompiles(CasperEvmPrecompiles::new(spec_id(self.config.spec))))
    }
}

fn configure_evm_cfg(cfg: &mut CfgEnv, config: &EvmConfig, validation: CallValidation) {
    let skip_validation = validation.is_unchecked_simulation();

    cfg.spec = spec_id(config.spec);
    cfg.chain_id = config.chain_id;
    cfg.tx_chain_id_check = !skip_validation;
    // Read-only simulations may consume the block budget, following eth_call.
    // All checked execution retains revm's Osaka transaction cap.
    cfg.tx_gas_limit_cap = skip_validation.then_some(u64::MAX);
    cfg.disable_block_gas_limit = false;
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
