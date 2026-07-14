//! Casper EVM precompile provider.

use casper_storage::global_state::{error::Error as GlobalStateError, state::StateReader};
use casper_types::{Key, StoredValue};
use revm::{
    context_interface::{Cfg, ContextTr},
    handler::{EthPrecompiles, PrecompileProvider},
    interpreter::{CallInputs, InterpreterResult},
    primitives::{hardfork::SpecId, Address},
};

use crate::{db::CasperDb, BlockHashProvider};

/// Ethereum precompiles executing with access to Casper-backed state.
#[derive(Clone, Debug)]
pub(crate) struct CasperEvmPrecompiles(EthPrecompiles);

impl CasperEvmPrecompiles {
    pub(crate) fn new(spec: SpecId) -> Self {
        Self(EthPrecompiles::new(spec))
    }
}

impl<'a, R, B, CTX> PrecompileProvider<CTX> for CasperEvmPrecompiles
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError> + 'a,
    B: BlockHashProvider + ?Sized + 'a,
    CTX: ContextTr<Db = CasperDb<'a, R, B>>,
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
        // Placeholder
        let _block_time = context
            .db_mut()
            .get_block_time()
            .map_err(|e| e.to_string())?;

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
