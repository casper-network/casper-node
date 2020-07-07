pub mod error;
pub mod memory_resolver;
pub mod v1_function_index;
mod v1_resolver;

use wasmi::ModuleImportResolver;

use types::ProtocolVersion;

use self::error::ResolverError;
use crate::contract_core::resolvers::memory_resolver::MemoryResolver;

/// Creates a module resolver for given protocol version.
///
/// * `protocol_version` Version of the protocol. Can't be lower than 1.
pub fn create_module_resolver(
    protocol_version: ProtocolVersion,
) -> Result<impl ModuleImportResolver + MemoryResolver, ResolverError> {
    // TODO: revisit how protocol_version check here is meant to combine with upgrade
    if protocol_version >= ProtocolVersion::V1_0_0 {
        return Ok(v1_resolver::RuntimeModuleImportResolver::default());
    }
    Err(ResolverError::UnknownProtocolVersion(protocol_version))
}

#[test]
fn resolve_invalid_module() {
    assert!(create_module_resolver(ProtocolVersion::default()).is_err());
}

#[test]
fn protocol_version_1_always_resolves() {
    assert!(create_module_resolver(ProtocolVersion::V1_0_0).is_ok());
}
