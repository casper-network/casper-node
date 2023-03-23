use std::{
    fmt::{self, Display, Formatter},
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
};

use casper_types::bytesrepr::{self, FromBytes, ToBytes};
use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::{
    components::gossiper::{GossipItem, SmallGossipItem},
    effect::GossipTarget,
};

/// Used to gossip our public listening address to peers.
#[derive(
    Copy, Clone, DataSize, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Debug,
)]
pub struct GossipedAddress(SocketAddr);

const BYTESREPR_IPV4_OCTET_LENGTH: usize = 4;
const BYTESREPR_IPV6_OCTET_LENGTH: usize = 16;
const BYTESREPR_U16_LENGTH: usize = 2;

impl ToBytes for GossipedAddress {
    fn to_bytes(&self) -> Result<Vec<u8>, casper_types::bytesrepr::Error> {
        let buffer = bytesrepr::allocate_buffer(self)?;
        match self.0 {
            SocketAddr::V4(v4) => {
                buffer.push(0u8);
                v4.port().write_bytes(&mut buffer)?;
                v4.ip().octets().write_bytes(&mut buffer)?;
            }
            SocketAddr::V6(v6) => {
                buffer.push(1u8);
                v6.port().write_bytes(&mut buffer)?;
                v6.ip().octets().write_bytes(&mut buffer)?;
            }
        }
        Ok(buffer)
    }

    #[inline]
    fn serialized_length(&self) -> usize {
        match self.0 {
            SocketAddr::V4(_) => 1 + BYTESREPR_U16_LENGTH + BYTESREPR_IPV4_OCTET_LENGTH,
            SocketAddr::V6(_) => 1 + BYTESREPR_U16_LENGTH + BYTESREPR_IPV6_OCTET_LENGTH,
        }
    }
}

impl FromBytes for GossipedAddress {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), casper_types::bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        let (port, remainder) = u16::from_bytes(remainder)?;
        match tag {
            0 => {
                let (octets, remainder) = <[u8; 4]>::from_bytes(remainder)?;
                Ok((
                    GossipedAddress(SocketAddr::from((Ipv4Addr::from(octets), port))),
                    remainder,
                ))
            }
            1 => {
                let (octets, remainder) = <[u8; 16]>::from_bytes(remainder)?;
                Ok((
                    GossipedAddress(SocketAddr::from((Ipv6Addr::from(octets), port))),
                    remainder,
                ))
            }
            // Note: `Formatting` has historically been used for invalid variant errors.
            _ => Err(::casper_types::bytesrepr::Error::Formatting),
        }
    }
}

impl GossipedAddress {
    pub(super) fn new(address: SocketAddr) -> Self {
        GossipedAddress(address)
    }
}

impl Display for GossipedAddress {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(formatter, "gossiped-address {}", self.0)
    }
}

impl GossipItem for GossipedAddress {
    const ID_IS_COMPLETE_ITEM: bool = true;
    const REQUIRES_GOSSIP_RECEIVED_ANNOUNCEMENT: bool = false;

    type Id = GossipedAddress;

    fn gossip_id(&self) -> Self::Id {
        *self
    }

    fn gossip_target(&self) -> GossipTarget {
        GossipTarget::All
    }
}

impl SmallGossipItem for GossipedAddress {
    fn id_as_item(id: &Self::Id) -> &Self {
        id
    }
}

impl From<GossipedAddress> for SocketAddr {
    fn from(gossiped_address: GossipedAddress) -> Self {
        gossiped_address.0
    }
}
