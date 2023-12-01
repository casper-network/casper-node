use std::collections::BTreeMap;

use crate::bytesrepr::{self, FromBytes, ToBytes};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Node peer entry.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PeerEntry {
    /// Node id.
    pub node_id: String,
    /// Node address.
    pub address: String,
}

impl ToBytes for PeerEntry {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.node_id.write_bytes(writer)?;
        self.address.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.node_id.serialized_length() + self.address.serialized_length()
    }
}

impl FromBytes for PeerEntry {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (node_id, remainder) = String::from_bytes(bytes)?;
        let (address, remainder) = String::from_bytes(remainder)?;
        Ok((PeerEntry { node_id, address }, remainder))
    }
}

/// Map of peer IDs to network addresses.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Peers(Vec<PeerEntry>);

impl Peers {
    /// Retrieve collection of `PeerEntry` records.
    pub fn into_inner(self) -> Vec<PeerEntry> {
        self.0
    }
}

impl<Id: ToString> From<BTreeMap<Id, String>> for Peers {
    fn from(input: BTreeMap<Id, String>) -> Self {
        let ret = input
            .into_iter()
            .map(|(node_id, address)| PeerEntry {
                node_id: node_id.to_string(),
                address,
            })
            .collect();
        Peers(ret)
    }
}

impl ToBytes for Peers {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.0.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for Peers {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (inner, remainder) = Vec::<PeerEntry>::from_bytes(bytes)?;
        Ok((Peers(inner), remainder))
    }
}
