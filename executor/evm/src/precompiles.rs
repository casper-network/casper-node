//! Casper EVM precompile provider.

use casper_storage::{
    eip2935::BLOCK_HASH_HISTORY_ADDRESS,
    eip4788::BEACON_ROOTS_ADDRESS,
    global_state::{error::Error as GlobalStateError, state::StateReader},
};
use casper_types::{Key, StoredValue};
use revm::{
    context_interface::{Block as _, Cfg, ContextTr},
    handler::{EthPrecompiles, PrecompileProvider},
    interpreter::{CallInputs, CallScheme, InterpreterResult},
    primitives::{hardfork::SpecId, Address},
};

use crate::{db::CasperDb, tx};

/// Ethereum precompiles executing with access to Casper-backed state.
#[derive(Clone, Debug)]
pub(crate) struct CasperEvmPrecompiles(EthPrecompiles);

impl CasperEvmPrecompiles {
    pub(crate) fn new(spec: SpecId) -> Self {
        Self(EthPrecompiles::new(spec))
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
            // Copy the input before borrowing the database mutably.  The input may be backed by
            // revm's shared memory buffer.
            let input = inputs.input.bytes(context);
            let result = context
                .db_mut()
                .eip4788_get(&input, inputs.gas_limit, inputs.reservoir)
                .map_err(|error| error.to_string())?;
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
            let result = context
                .db_mut()
                .eip2935_get(&input, block_number, inputs.gas_limit, inputs.reservoir)
                .map_err(|error| error.to_string())?;
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
