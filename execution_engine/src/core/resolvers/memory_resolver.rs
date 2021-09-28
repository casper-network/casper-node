//! This module contains resolver of a memory section of the WASM code.
use wasmi::MemoryRef;

use super::error::ResolverError;

/// This trait takes care of returning an instance of allocated memory.
///
/// This happens once the WASM program tries to resolve "memory". Whenever
/// contract didn't request a memory this method should return an Error.
pub trait MemoryResolver {
    /// Returns a memory instance.
    fn memory_ref(&self) -> Result<MemoryRef, ResolverError>;
}
