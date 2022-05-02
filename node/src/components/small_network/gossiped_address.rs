use std::{
    convert::Infallible,
    fmt::{self, Display, Formatter},
    net::SocketAddr,
};

use datasize::DataSize;
#[cfg(test)]
use rand::Rng;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use casper_types::testing::TestRng;
use casper_types::{
    bytesrepr::{allocate_buffer, Error as BytesreprError, FromBytes, ToBytes},
    EraId,
};

use crate::types::{Item, Tag};

/// Used to gossip our public listening address to peers.
#[derive(
    Copy, Clone, DataSize, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Debug,
)]
pub struct GossipedAddress(SocketAddr);

impl GossipedAddress {
    pub(super) fn new(address: SocketAddr) -> Self {
        GossipedAddress(address)
    }

    #[cfg(test)]
    pub fn random_ipv4(rng: &mut TestRng) -> Self {
        let bytes = rng.gen::<[u8; 4]>();
        let port = rng.gen::<u16>();
        Self::new(SocketAddr::new(bytes.into(), port))
    }

    #[cfg(test)]
    pub fn random_ipv6(rng: &mut TestRng) -> Self {
        let bytes = rng.gen::<[u8; 16]>();
        let port = rng.gen::<u16>();
        Self::new(SocketAddr::new(bytes.into(), port))
    }
}

impl Display for GossipedAddress {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "gossiped-address {}", self.0)
    }
}

impl ToBytes for GossipedAddress {
    fn to_bytes(&self) -> Result<Vec<u8>, BytesreprError> {
        let mut buf = allocate_buffer(self)?;
        self.write_bytes(&mut buf)?;
        Ok(buf)
    }

    fn serialized_length(&self) -> usize {
        ToBytes::serialized_length(&self.0.to_string())
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), BytesreprError> {
        self.0.to_string().write_bytes(writer)
    }
}

impl FromBytes for GossipedAddress {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), BytesreprError> {
        let (sock_addr_str, bytes): (String, &[u8]) = FromBytes::from_bytes(bytes)?;
        let sock_addr = sock_addr_str
            .parse()
            .map_err(|_| BytesreprError::Formatting)?;
        Ok((GossipedAddress::new(sock_addr), bytes))
    }
}

impl Item for GossipedAddress {
    type Id = GossipedAddress;
    type ValidationError = Infallible;
    const TAG: Tag = Tag::GossipedAddress;
    const ID_IS_COMPLETE_ITEM: bool = true;

    fn validate(
        &self,
        _verifiable_chunked_hash_activation: EraId,
    ) -> Result<(), Self::ValidationError> {
        Ok(())
    }

    fn id(&self, _verifiable_chunked_hash_activation: EraId) -> Self::Id {
        *self
    }
}

impl From<GossipedAddress> for SocketAddr {
    fn from(gossiped_address: GossipedAddress) -> Self {
        gossiped_address.0
    }
}

#[cfg(test)]
mod tests {
    use casper_types::bytesrepr;

    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let mut rng = crate::new_rng();
        let gossiped_address_ipv4 = GossipedAddress::random_ipv4(&mut rng);
        bytesrepr::test_serialization_roundtrip(&gossiped_address_ipv4);

        let gossiped_address_ipv6 = GossipedAddress::random_ipv6(&mut rng);
        bytesrepr::test_serialization_roundtrip(&gossiped_address_ipv6);
    }
}
