use std::{
    fmt::{Debug, Display},
    hash::Hash,
};

use serde::{de::DeserializeOwned, Serialize};

use crate::effect::GossipTarget;

/// A trait which allows an implementing type to be used by a gossiper component.
pub(crate) trait GossipItem:
    Clone + Serialize + DeserializeOwned + Send + Sync + Debug + Display + Eq
{
    /// The type of ID of the item.
    type Id: Clone + Eq + Hash + Serialize + DeserializeOwned + Send + Sync + Debug + Display;

    /// Whether the item's ID _is_ the complete item or not.
    const ID_IS_COMPLETE_ITEM: bool;
    /// Whether the arrival of a new gossip message should be announced or not.
    const REQUIRES_GOSSIP_RECEIVED_ANNOUNCEMENT: bool;

    /// The ID of the specific item.
    fn gossip_id(&self) -> Self::Id;

    /// Identifies the kind of peers which should be targeted for onwards gossiping.
    fn gossip_target(&self) -> GossipTarget;
}
