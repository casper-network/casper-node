//! Casper EVM precompile provider.

use casper_storage::{
    eip2935::BLOCK_HASH_HISTORY_ADDRESS,
    eip4788::BEACON_ROOTS_ADDRESS,
    global_state::{error::Error as GlobalStateError, state::StateReader},
};
use casper_types::{Key, StoredValue};
use revm::{
    context_interface::{Block as _, Cfg, ContextError, ContextTr},
    database_interface::Database,
    handler::{EthPrecompiles, PrecompileProvider},
    interpreter::{CallInputs, CallScheme, Gas, InstructionResult, InterpreterResult},
    primitives::{hardfork::SpecId, Address, Bytes, B256},
};

use crate::{db::CasperDb, tx, DbError};

/// Ethereum precompiles executing with access to Casper-backed state.
#[derive(Clone, Debug)]
pub(crate) struct CasperEvmPrecompiles(EthPrecompiles);

impl CasperEvmPrecompiles {
    pub(crate) fn new(spec: SpecId) -> Self {
        Self(EthPrecompiles::new(spec))
    }
}

fn native_get_result<CTX>(
    context: &mut CTX,
    lookup_result: Result<Option<B256>, DbError>,
    gas_limit: u64,
    reservoir: u64,
) -> InterpreterResult
where
    CTX: ContextTr,
    <CTX::Db as Database>::Error: From<DbError>,
{
    let (result, output) = match lookup_result {
        Ok(Some(value)) => (
            InstructionResult::Return,
            Bytes::copy_from_slice(value.as_slice()),
        ),
        Ok(None) => (InstructionResult::Revert, Bytes::new()),
        Err(error) => {
            // Preserve database errors in revm's typed error channel.  Returning a string from
            // this provider would turn the error into `EVMError::Custom`.
            *context.error() = Err(ContextError::Db(error.into()));
            (InstructionResult::FatalExternalError, Bytes::new())
        }
    };

    InterpreterResult {
        result,
        output,
        // Native lookups have no interpreted-bytecode or SLOAD cost. Retain the call frame's
        // reservoir so EIP-8037 accounting remains unchanged.
        gas: Gas::new_with_regular_gas_and_reservoir(gas_limit, reservoir),
    }
}

impl<'a, R, S, CTX> PrecompileProvider<CTX> for CasperEvmPrecompiles
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError> + 'a,
    S: 'a,
    CTX: ContextTr<Db = CasperDb<'a, R, S>>,
{
    type Output = InterpreterResult;

    #[inline(always)]
    fn set_spec(&mut self, spec: <CTX::Cfg as Cfg>::Spec) -> bool {
        <EthPrecompiles as PrecompileProvider<CTX>>::set_spec(&mut self.0, spec)
    }

    fn run(
        &mut self,
        context: &mut CTX,
        inputs: &CallInputs,
    ) -> Result<Option<Self::Output>, String> {
        let beacon_roots_address = tx::to_revm_address(BEACON_ROOTS_ADDRESS);
        if inputs.target_address == beacon_roots_address
            && inputs.bytecode_address == beacon_roots_address
            && matches!(inputs.scheme, CallScheme::Call | CallScheme::StaticCall)
        {
            // Casper does not support Ethereum beacon-chain roots. Keep the predeploy callable
            // without maintaining placeholder state.
            let result = native_get_result(
                context,
                Ok(Some(B256::ZERO)),
                inputs.gas_limit,
                inputs.reservoir,
            );
            return Ok(Some(result));
        }

        let block_hash_history_address = tx::to_revm_address(BLOCK_HASH_HISTORY_ADDRESS);
        if inputs.target_address == block_hash_history_address
            && inputs.bytecode_address == block_hash_history_address
            && matches!(inputs.scheme, CallScheme::Call | CallScheme::StaticCall)
        {
            // Copy the input before borrowing the database mutably.  The input may be backed by
            // revm's shared memory buffer.
            let input = inputs.input.bytes(context);
            let block_number = context.block().number();
            let lookup_result = context.db_mut().eip2935_get(&input, block_number);
            let result =
                native_get_result(context, lookup_result, inputs.gas_limit, inputs.reservoir);
            return Ok(Some(result));
        }

        <EthPrecompiles as PrecompileProvider<CTX>>::run(&mut self.0, context, inputs)
    }

    #[inline(always)]
    fn warm_addresses(&self) -> Box<impl Iterator<Item = Address>> {
        self.0.warm_addresses()
    }

    #[inline(always)]
    fn contains(&self, address: &Address) -> bool {
        self.0.contains(address)
    }
}
