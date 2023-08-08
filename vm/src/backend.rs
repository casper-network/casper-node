pub(crate) mod wasmer;
use thiserror::Error;
use std::{marker::PhantomData, ops::Deref};

use crate::{storage::Storage, Error as VMError};

#[derive(Debug)]
pub struct GasSummary {}

/// Container that holds all relevant modules necessary to process an execution request.
pub struct Context<S: Storage> {
    pub initial_gas_limit: u64,
    pub storage: S,
}

/// An abstraction over the 'caller' object of a host function that works for any Wasm VM.
///
/// This allows access for important instances such as the context object that was passed to the
/// instance, wasm linear memory access, etc.

pub(crate) trait Caller<S: Storage> {
    fn context(&self) -> &Context<S>;
    fn memory_read(&self, offset: u32, size: usize) -> Result<Vec<u8>, VMError> {
        let mut vec = vec![0; size];
        self.memory_read_into(offset, &mut vec)?;
        Ok(vec)
    }
    fn memory_read_into(&self, offset: u32, output: &mut [u8]) -> Result<(), VMError>;
    fn memory_write(&self, offset: u32, data: &[u8]) -> Result<(), VMError>;
    fn alloc(&mut self, size: usize) -> Result<u32, VMError>;
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Export error: {0}")]
    Export(String),
    // OutOfGas,
    // Trap(String),
    #[error("Compile error: {0}")]
    CompileError(String),
}

// struct Payload

pub trait WasmInstance<S: Storage> {
    fn call_export(&mut self, name: &str, args: &[&[u8]]) -> (Result<(), VMError>, GasSummary);
    fn teardown(self) -> Context<S>;
}
