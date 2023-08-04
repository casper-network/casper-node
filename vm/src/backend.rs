pub(crate) mod wasmer;

use std::{marker::PhantomData, ops::Deref};

use crate::{storage::Storage, Error as VMError};

#[derive(Debug)]
pub struct GasSummary {}

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
    fn memory_read(&self, offset: u32, size: usize) -> Result<Vec<u8>, Error> {
        let mut vec = vec![0; size];
        self.memory_read_into(offset, &mut vec)?;
        Ok(vec)
    }
    fn memory_read_into(&self, offset: u32, output: &mut [u8]) -> Result<(), Error>;
    fn memory_write(&self, offset: u32, data: &[u8]) -> Result<(), Error>;
    fn alloc(&mut self, size: usize) -> u32;
}

#[derive(Debug)]
pub enum Error {
    // OutOfGas,
    // Trap(String),
    CompileError(String),
}

pub trait WasmInstance<S: Storage> {
    fn call_export0(&mut self, name: &str) -> (Result<(), VMError>, GasSummary);
    fn teardown(self) -> Context<S>;
}
