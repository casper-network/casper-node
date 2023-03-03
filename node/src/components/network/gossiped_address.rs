use std::{
    fmt::{self, Display, Formatter},
    net::SocketAddr,
};

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

mod specimen_support {
    use crate::utils::specimen::{Cache, LargestSpecimen, SizeEstimator};

    use super::GossipedAddress;

    impl LargestSpecimen for GossipedAddress {
        fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
            GossipedAddress::new(LargestSpecimen::largest_specimen(estimator, cache))
        }
    }
}
