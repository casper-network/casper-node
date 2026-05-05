//! Public executor entry point.

use casper_storage::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    TrackingCopy,
};
use casper_types::{evm, Key, StoredValue};
use revm::{
    context_interface::result::EVMError, primitives::hardfork::SpecId, Context, ExecuteEvm,
    MainBuilder, MainContext,
};

use crate::{
    db::CasperDb, state, tx, DbError, Error, ExecuteKind, ExecuteRequest, ExecutionOutcome, Result,
};

/// Executes EVM transactions and calls against a Casper tracking copy.
///
/// The executor writes only to the supplied [`TrackingCopy`]. It never commits
/// global state; callers can pass a forked tracking copy for view execution or
/// commit the resulting effects through the normal Casper storage flow.
#[derive(Clone, Debug)]
pub struct EvmExecutor {
    config: evm::EvmConfig,
}

impl EvmExecutor {
    /// Creates a new executor from chainspec EVM configuration.
    pub fn new(config: evm::EvmConfig) -> Self {
        Self { config }
    }

    /// Returns the immutable EVM configuration used by this executor.
    pub fn config(&self) -> &evm::EvmConfig {
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
        if !self.config.enabled {
            return Err(Error::Disabled);
        }

        if let ExecuteKind::Transaction(transaction) = &request.kind {
            if let Some(actual) = transaction.chain_id() {
                if actual != self.config.chain_id {
                    return Err(Error::ChainIdMismatch {
                        expected: self.config.chain_id,
                        actual,
                    });
                }
            }
        }

        let spec = spec_id(self.config.spec);
        let tx_env = tx::build_tx_env(&self.config, &request.kind)?;
        let block = request.block.to_revm_block(&self.config);
        let is_call = matches!(request.kind, ExecuteKind::Call(_));

        let result_and_state = {
            let db = CasperDb::new(tracking_copy);
            let mut evm = Context::mainnet()
                .with_db(db)
                .with_block(block)
                .modify_cfg_chained(|cfg| {
                    cfg.spec = spec;
                    cfg.chain_id = self.config.chain_id;
                    cfg.tx_chain_id_check = !is_call;
                    cfg.disable_block_gas_limit = false;
                    cfg.disable_base_fee = is_call;
                    cfg.disable_balance_check = is_call;
                    cfg.disable_nonce_check = is_call;
                })
                .build_mainnet();

            evm.transact(tx_env).map_err(map_revm_error)?
        };

        let outcome = ExecutionOutcome::from_revm_result(&result_and_state.result);
        state::apply(tracking_copy, result_and_state.state)?;
        Ok(outcome)
    }
}

fn spec_id(spec: evm::EvmSpec) -> SpecId {
    match spec {
        evm::EvmSpec::Frontier => SpecId::FRONTIER,
        evm::EvmSpec::FrontierThawing => SpecId::FRONTIER_THAWING,
        evm::EvmSpec::Homestead => SpecId::HOMESTEAD,
        evm::EvmSpec::DaoFork => SpecId::DAO_FORK,
        evm::EvmSpec::Tangerine => SpecId::TANGERINE,
        evm::EvmSpec::SpuriousDragon => SpecId::SPURIOUS_DRAGON,
        evm::EvmSpec::Byzantium => SpecId::BYZANTIUM,
        evm::EvmSpec::Constantinople => SpecId::CONSTANTINOPLE,
        evm::EvmSpec::Petersburg => SpecId::PETERSBURG,
        evm::EvmSpec::Istanbul => SpecId::ISTANBUL,
        evm::EvmSpec::MuirGlacier => SpecId::MUIR_GLACIER,
        evm::EvmSpec::Berlin => SpecId::BERLIN,
        evm::EvmSpec::London => SpecId::LONDON,
        evm::EvmSpec::ArrowGlacier => SpecId::ARROW_GLACIER,
        evm::EvmSpec::GrayGlacier => SpecId::GRAY_GLACIER,
        evm::EvmSpec::Merge => SpecId::MERGE,
        evm::EvmSpec::Shanghai => SpecId::SHANGHAI,
        evm::EvmSpec::Cancun => SpecId::CANCUN,
        evm::EvmSpec::Prague => SpecId::PRAGUE,
        evm::EvmSpec::Osaka => SpecId::OSAKA,
    }
}

fn map_revm_error(error: EVMError<DbError>) -> Error {
    match error {
        EVMError::Database(error) => Error::Database(error),
        other => Error::Revm(other.to_string()),
    }
}
