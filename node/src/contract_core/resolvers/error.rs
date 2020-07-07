use failure::Fail;

use types::ProtocolVersion;

#[derive(Fail, Debug, Copy, Clone)]
pub enum ResolverError {
    #[fail(display = "Unknown protocol version: {}", _0)]
    UnknownProtocolVersion(ProtocolVersion),
    #[fail(display = "No imported memory")]
    NoImportedMemory,
}
