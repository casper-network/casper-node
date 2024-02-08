use core::fmt::{self, Debug, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::bytesrepr::Bytes;

/// The raw bytes of the chainspec.toml, genesis accounts.toml, and global_state.toml files.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(feature = "json-schema", derive(JsonSchema))]
pub struct ChainspecRawBytes {
    /// Raw bytes of the current chainspec.toml file.
    chainspec_bytes: Bytes,
    /// Raw bytes of the current genesis accounts.toml file.
    maybe_genesis_accounts_bytes: Option<Bytes>,
    /// Raw bytes of the current global_state.toml file.
    maybe_global_state_bytes: Option<Bytes>,
}

impl ChainspecRawBytes {
    /// Create an instance from parts.
    pub fn new(
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

    /// The bytes of the chainspec file.
    pub fn chainspec_bytes(&self) -> &[u8] {
        self.chainspec_bytes.as_slice()
    }

    /// The bytes of global state account entries, when present for a protocol version.
    pub fn maybe_genesis_accounts_bytes(&self) -> Option<&[u8]> {
        match self.maybe_genesis_accounts_bytes.as_ref() {
            Some(bytes) => Some(bytes.as_slice()),
            None => None,
        }
    }

    /// The bytes of global state update entries, when present for a protocol version.
    pub fn maybe_global_state_bytes(&self) -> Option<&[u8]> {
        match self.maybe_global_state_bytes.as_ref() {
            Some(bytes) => Some(bytes.as_slice()),
            None => None,
        }
    }
}

impl Debug for ChainspecRawBytes {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
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

impl Display for ChainspecRawBytes {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "{}",
            String::from_utf8_lossy(&self.chainspec_bytes)
        )?;
        if let Some(genesis_accounts_bytes) = &self.maybe_genesis_accounts_bytes {
            write!(
                formatter,
                "{}",
                String::from_utf8_lossy(genesis_accounts_bytes)
            )?;
        }
        if let Some(global_state_bytes) = &self.maybe_global_state_bytes {
            write!(formatter, "{}", String::from_utf8_lossy(global_state_bytes))?;
        }
        Ok(())
    }
}
