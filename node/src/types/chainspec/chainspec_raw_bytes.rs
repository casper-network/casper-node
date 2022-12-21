use datasize::DataSize;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use casper_types::bytesrepr::Bytes;

/// The raw bytes of the chainspec.toml, genesis accounts.toml, and global_state.toml files.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, DataSize, JsonSchema)]
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

impl std::fmt::Debug for ChainspecRawBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let genesis_accounts_bytes_owned: Bytes;
        let global_state_bytes_owned: Bytes;
        f.debug_struct("ChainspecRawBytes")
            .field(
                "chainspec_bytes",
                &self.chainspec_bytes[0..16].to_ascii_uppercase(),
            )
            .field(
                "maybe_genesis_accounts_bytes",
                match self.maybe_genesis_accounts_bytes.as_ref() {
                    Some(genesis_accounts_bytes) => {
                        genesis_accounts_bytes_owned =
                            genesis_accounts_bytes[0..16].to_ascii_uppercase().into();
                        &genesis_accounts_bytes_owned
                    }
                    None => &self.maybe_genesis_accounts_bytes,
                },
            )
            .field(
                "maybe_global_state_bytes",
                match self.maybe_global_state_bytes.as_ref() {
                    Some(global_state_bytes) => {
                        global_state_bytes_owned =
                            global_state_bytes[0..16].to_ascii_uppercase().into();
                        &global_state_bytes_owned
                    }
                    None => &self.maybe_global_state_bytes,
                },
            )
            .finish()
    }
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
