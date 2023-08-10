pub(crate) mod wasmer;
use std::{marker::PhantomData, ops::Deref};
use thiserror::Error;

use crate::{storage::Storage, Error as VMError};

#[derive(Debug)]
pub struct GasUsage {
    pub(crate) gas_limit: u64,
    pub(crate) remaining_points: u64,
    // pub(crate) external_operations: u64, i.e. alloc/dealloc
}

/// Container that holds all relevant modules necessary to process an execution request.
pub struct Context<S: Storage> {
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
    /// Allocates memory inside the Wasm VM by calling an export.
    ///
    /// Error is a type-erased error coming from the VM itself.
    fn alloc(&mut self, idx: u32, size: usize, ctx: u32)
        -> Result<u32, Box<dyn std::error::Error>>;
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Export error: {0}")]
    Export(String),
    // OutOfGas,
    // Trap(String),
    #[error("Compile error: {0}")]
    Compile(String),
    #[error("Memory instantiation error: {0}")]
    Memory(String),
    #[error("Instantiation error: {0}")]
    Instantiation(String),
}

// struct Payload

pub trait WasmInstance<S: Storage> {
    fn call_export(&mut self, name: &str, args: &[&[u8]]) -> (Result<(), VMError>, GasUsage);
    fn teardown(self) -> Context<S>;
}
