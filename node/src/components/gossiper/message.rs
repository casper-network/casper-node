use std::{
    boxed::Box,
    fmt::{self, Display, Formatter},
};

use serde::{Deserialize, Serialize};
use strum::EnumDiscriminants;

use super::GossipItem;

#[derive(Clone, Debug, Deserialize, Serialize, EnumDiscriminants)]
#[strum_discriminants(derive(strum::EnumIter))]
#[serde(bound = "for<'a> T: Deserialize<'a>")]
pub(crate) enum Message<T: GossipItem> {
    /// Gossiped out to random peers to notify them of an item we hold.
    Gossip(T::Id),
    /// Response to a `Gossip` message.  If `is_already_held` is false, the recipient should treat
    /// this as a `GetItem` message and send an `Item` message containing the item.
    GossipResponse {
        item_id: T::Id,
        is_already_held: bool,
    },
    /// Request to get an item we were previously told about, but the peer timed out and we never
    /// received it.
    GetItem(T::Id),
    /// Response to either a `GossipResponse` with `is_already_held` set to `false` or to a
    /// `GetItem` message. Contains the actual item requested.
    Item(Box<T>),
}

impl<T: GossipItem> Display for Message<T> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Message::Gossip(item_id) => write!(formatter, "gossip({})", item_id),
            Message::GossipResponse {
                item_id,
                is_already_held,
            } => write!(
                formatter,
                "gossip-response({}, {})",
                item_id, is_already_held
            ),
            Message::GetItem(item_id) => write!(formatter, "gossip-get-item({})", item_id),
            Message::Item(item) => write!(formatter, "gossip-item({})", item.gossip_id()),
        }
    }
}

mod specimen_support {
    use crate::{
        components::gossiper::GossipItem,
        utils::specimen::{largest_variant, Cache, LargestSpecimen, SizeEstimator},
    };

    use super::{Message, MessageDiscriminants};

    impl<T> LargestSpecimen for Message<T>
    where
        T: GossipItem + LargestSpecimen,
        <T as GossipItem>::Id: LargestSpecimen,
    {
        fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
            largest_variant::<Self, MessageDiscriminants, _, _>(
                estimator,
                |variant| match variant {
                    MessageDiscriminants::Gossip => {
                        Message::Gossip(LargestSpecimen::largest_specimen(estimator, cache))
                    }
                    MessageDiscriminants::GossipResponse => Message::GossipResponse {
                        item_id: LargestSpecimen::largest_specimen(estimator, cache),
                        is_already_held: LargestSpecimen::largest_specimen(estimator, cache),
                    },
                    MessageDiscriminants::GetItem => {
                        Message::GetItem(LargestSpecimen::largest_specimen(estimator, cache))
                    }
                    MessageDiscriminants::Item => {
                        Message::Item(LargestSpecimen::largest_specimen(estimator, cache))
                    }
                },
            )
        }
    }
}
