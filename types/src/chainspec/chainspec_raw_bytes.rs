use core::fmt::{self, Debug, Display, Formatter};

use crate::bytesrepr::{self, Bytes, FromBytes, ToBytes};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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

    /// Returns a random `ChainspecRawBytes`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        use rand::Rng;

        let chainspec_bytes = Bytes::from(rng.random_vec(0..1024));
        let maybe_genesis_accounts_bytes = rng
            .gen::<bool>()
            .then(|| Bytes::from(rng.random_vec(0..1024)));
        let maybe_global_state_bytes = rng
            .gen::<bool>()
            .then(|| Bytes::from(rng.random_vec(0..1024)));
        ChainspecRawBytes {
            chainspec_bytes,
            maybe_genesis_accounts_bytes,
            maybe_global_state_bytes,
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

impl ToBytes for ChainspecRawBytes {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        let ChainspecRawBytes {
            chainspec_bytes,
            maybe_genesis_accounts_bytes,
            maybe_global_state_bytes,
        } = self;

        chainspec_bytes.write_bytes(writer)?;
        maybe_genesis_accounts_bytes.write_bytes(writer)?;
        maybe_global_state_bytes.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        let ChainspecRawBytes {
            chainspec_bytes,
            maybe_genesis_accounts_bytes,
            maybe_global_state_bytes,
        } = self;
        chainspec_bytes.serialized_length()
            + maybe_genesis_accounts_bytes.serialized_length()
            + maybe_global_state_bytes.serialized_length()
    }
}

impl FromBytes for ChainspecRawBytes {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (chainspec_bytes, remainder) = FromBytes::from_bytes(bytes)?;
        let (maybe_genesis_accounts_bytes, remainder) = FromBytes::from_bytes(remainder)?;
        let (maybe_global_state_bytes, remainder) = FromBytes::from_bytes(remainder)?;

        Ok((
            ChainspecRawBytes {
                chainspec_bytes,
                maybe_genesis_accounts_bytes,
                maybe_global_state_bytes,
            },
            remainder,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = ChainspecRawBytes::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
