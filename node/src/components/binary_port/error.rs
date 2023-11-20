use casper_execution_engine::engine_state;
use casper_types::bytesrepr;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("Serialization error: {}", _0)]
    BytesRepr(bytesrepr::Error),
    #[error("Execution engine error: {}", _0)]
    EngineState(engine_state::Error),
}
