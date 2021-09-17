//! This module is responsible for resolving host functions from within the WASM engine.
pub mod error;
pub mod memory_resolver;
pub(crate) mod v1_function_index;
mod v1_resolver;

use wasmi::ModuleImportResolver;

use casper_types::ProtocolVersion;

use self::error::ResolverError;
use crate::{core::resolvers::memory_resolver::MemoryResolver, shared::wasm_config::WasmConfig};

/// Creates a module resolver for given protocol version.
///
/// * `protocol_version` Version of the protocol. Can't be lower than 1.
pub(crate) fn create_module_resolver(
    protocol_version: ProtocolVersion,
    wasm_config: &WasmConfig,
) -> Result<impl ModuleImportResolver + MemoryResolver, ResolverError> {
    // TODO: revisit how protocol_version check here is meant to combine with upgrade
    if protocol_version >= ProtocolVersion::V1_0_0 {
        return Ok(v1_resolver::RuntimeModuleImportResolver::new(
            wasm_config.max_memory,
        ));
    }
    Err(ResolverError::UnknownProtocolVersion(protocol_version))
}

#[cfg(test)]
mod tests {
    use casper_types::ProtocolVersion;

    use super::*;
    use crate::shared::wasm_config::WasmConfig;

    #[test]
    fn resolve_invalid_module() {
        assert!(
            create_module_resolver(ProtocolVersion::default(), &WasmConfig::default()).is_err()
        );
    }

    #[test]
    fn protocol_version_1_always_resolves() {
        assert!(create_module_resolver(ProtocolVersion::V1_0_0, &WasmConfig::default()).is_ok());
    }
}
