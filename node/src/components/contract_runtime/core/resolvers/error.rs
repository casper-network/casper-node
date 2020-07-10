use thiserror::Error;

use types::ProtocolVersion;

#[derive(Error, Debug, Copy, Clone)]
pub enum ResolverError {
    #[error("Unknown protocol version: {}", _0)]
    UnknownProtocolVersion(ProtocolVersion),
    #[error("No imported memory")]
    NoImportedMemory,
}
