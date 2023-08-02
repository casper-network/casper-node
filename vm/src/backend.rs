pub(crate) mod wasmer;

use std::sync::Arc;

use bytes::Bytes;

use crate::storage::Storage;

#[derive(Debug)]
pub struct GasSummary;

struct Environment<S:Storage> {
    storage: Arc<S>,
}

#[derive(Debug)]
pub enum Error {
    // OutOfGas,
    // Trap(String),
    CompileError(String),
}
