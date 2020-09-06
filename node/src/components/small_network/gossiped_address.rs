use std::{
    fmt::{self, Display, Formatter},
    net::SocketAddr,
};

use serde::{Deserialize, Serialize};

use crate::types::{Item, Tag};

/// Used to gossip our public listening address to peers.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, Debug)]
pub struct GossipedAddress {
    /// Our public listening address.
    address: SocketAddr,
    /// The index of the gossip iteration.  This is used to avoid the gossip table from filtering
    /// out the message - i.e. to make each fresh gossip iteration have a unique identifier for the
    /// gossiped item.
    index: u32,
}

impl GossipedAddress {
    pub(super) fn new(address: SocketAddr, index: u32) -> Self {
        GossipedAddress { address, index }
    }
}

impl Display for GossipedAddress {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "gossiped-address {} iter {}",
            self.address, self.index
        )
    }
}

impl Item for GossipedAddress {
    type Id = GossipedAddress;
    const TAG: Tag = Tag::GossipedAddress;
    const ID_IS_COMPLETE_ITEM: bool = true;

    fn id(&self) -> Self::Id {
        *self
    }
}

impl From<GossipedAddress> for SocketAddr {
    fn from(gossiped_address: GossipedAddress) -> Self {
        gossiped_address.address
    }
}
