//! Definition of a chain kind flag that is used to determine a operating mode of a chain.
use datasize::DataSize;
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use rand::{distributions::Standard, prelude::*};
use serde::{Deserialize, Serialize};

use casper_types::bytesrepr::{Error, FromBytes, ToBytes, U8_SERIALIZED_LENGTH};

/// Flag representing a mode of operation of a chain.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, DataSize, FromPrimitive)]
#[serde(rename_all = "lowercase")]
pub enum ChainKind {
    /// Public (default) mode
    Public,
    /// Private chain mode with extra capabilities only available if this mode is set.
    Private,
}

impl ChainKind {
    /// Returns `true` if the chain kind is [`Public`].
    ///
    /// [`Public`]: ChainKind::Public
    #[must_use]
    pub fn is_public(&self) -> bool {
        matches!(self, Self::Public)
    }

    /// Returns `true` if the chain kind is [`Private`].
    ///
    /// [`Private`]: ChainKind::Private
    #[must_use]
    pub fn is_private(&self) -> bool {
        matches!(self, Self::Private)
    }
}

impl ToBytes for ChainKind {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let chain_kind_value = *self as u8;
        chain_kind_value.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
    }
}

impl FromBytes for ChainKind {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (chain_kind_value, remainder) = u8::from_bytes(bytes)?;
        let chain_kind = ChainKind::from_u8(chain_kind_value).ok_or(Error::Formatting)?;
        Ok((chain_kind, remainder))
    }
}

impl Default for ChainKind {
    fn default() -> Self {
        Self::Public
    }
}

impl Distribution<ChainKind> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> ChainKind {
        let flag: bool = rng.gen();
        if flag {
            ChainKind::Public
        } else {
            ChainKind::Private
        }
    }
}
