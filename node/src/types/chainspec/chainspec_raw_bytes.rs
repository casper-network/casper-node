use datasize::DataSize;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use casper_types::bytesrepr::Bytes;

/// The raw bytes of the chainspec.toml, genesis accounts.toml, and global_state.toml files.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize, DataSize, JsonSchema)]
pub struct ChainspecRawBytes {
    #[schemars(
        with = "String",
        description = "Hex-encoded raw bytes of the current chainspec.toml file."
    )]
    chainspec_bytes: Bytes,
    #[schemars(
        with = "String",
        description = "Hex-encoded raw bytes of the current genesis accounts.toml file."
    )]
    maybe_genesis_accounts_bytes: Option<Bytes>,
    #[schemars(
        with = "String",
        description = "Hex-encoded raw bytes of the current global_state.toml file."
    )]
    maybe_global_state_bytes: Option<Bytes>,
}

impl ChainspecRawBytes {
    pub(crate) fn new(
        chainspec_bytes: Bytes,
        maybe_genesis_accounts_bytes: Option<Bytes>,
        maybe_global_state_bytes: Option<Bytes>,
    ) -> Self {
        ChainspecRawBytes {
            chainspec_bytes,
            maybe_genesis_accounts_bytes,
            maybe_global_state_bytes,
        }
    }

    pub(crate) fn chainspec_bytes(&self) -> &[u8] {
        self.chainspec_bytes.as_slice()
    }

    pub(crate) fn maybe_genesis_accounts_bytes(&self) -> Option<&[u8]> {
        match self.maybe_genesis_accounts_bytes.as_ref() {
            Some(bytes) => Some(bytes.as_slice()),
            None => None,
        }
    }

    pub(crate) fn maybe_global_state_bytes(&self) -> Option<&[u8]> {
        match self.maybe_global_state_bytes.as_ref() {
            Some(bytes) => Some(bytes.as_slice()),
            None => None,
        }
    }
}
