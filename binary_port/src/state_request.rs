use std::fmt::{Display, Formatter, Result as DisplayResult};

use crate::entity_qualifier::GlobalStateEntityQualifier;
#[cfg(test)]
use casper_types::testing::TestRng;
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    GlobalStateIdentifier,
};
#[cfg(test)]
use rand::Rng;

/// A request to get data from the global state.
#[derive(Clone, Debug, PartialEq)]
pub struct GlobalStateRequest {
    /// Global state identifier, `None` means "latest block state".
    state_identifier: Option<GlobalStateIdentifier>,
    /// qualifier that points to a specific item (or items) in the global state.
    qualifier: GlobalStateEntityQualifier,
}

impl GlobalStateRequest {
    pub fn new(
        state_identifier: Option<GlobalStateIdentifier>,
        qualifier: GlobalStateEntityQualifier,
    ) -> Self {
        GlobalStateRequest {
            state_identifier,
            qualifier,
        }
    }
    pub fn destructure(self) -> (Option<GlobalStateIdentifier>, GlobalStateEntityQualifier) {
        (self.state_identifier, self.qualifier)
    }

    pub fn state_identifier(self) -> Option<GlobalStateIdentifier> {
        self.state_identifier
    }

    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        let state_identifier = rng
            .gen::<bool>()
            .then(|| GlobalStateIdentifier::random(rng));
        let qualifier = GlobalStateEntityQualifier::random(rng);
        Self {
            state_identifier,
            qualifier,
        }
    }
}

impl ToBytes for GlobalStateRequest {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.state_identifier.write_bytes(writer)?;
        self.qualifier.write_bytes(writer)?;
        Ok(())
    }

    fn serialized_length(&self) -> usize {
        self.state_identifier.serialized_length() + self.qualifier.serialized_length()
    }
}

impl FromBytes for GlobalStateRequest {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (state_identifier, remainder) = FromBytes::from_bytes(bytes)?;
        let (qualifier, remainder) = FromBytes::from_bytes(remainder)?;
        Ok((
            GlobalStateRequest {
                state_identifier,
                qualifier,
            },
            remainder,
        ))
    }
}

impl Display for GlobalStateRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> DisplayResult {
        match self.qualifier {
            GlobalStateEntityQualifier::Item { base_key, .. } => {
                write!(f, "get item from global state ({})", base_key)
            }
            GlobalStateEntityQualifier::AllItems { key_tag, .. } => {
                write!(f, "get all items ({})", key_tag)
            }
            GlobalStateEntityQualifier::DictionaryItem { .. } => {
                write!(f, "get dictionary item")
            }
            GlobalStateEntityQualifier::Balance { .. } => {
                write!(f, "get balance by state root",)
            }
            GlobalStateEntityQualifier::ItemsByPrefix { .. } => {
                write!(f, "get items by prefix")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use casper_types::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        let val = GlobalStateRequest::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
