use std::fmt::Debug;

use serde::{Deserialize, Serialize};

use crate::{
    components::consensus::{
        highway_core::{
            endorsement::Endorsements,
            evidence::Evidence,
            state::{self, Panorama},
            validators::ValidatorIndex,
        },
        traits::{Context, ValidatorSecret},
    },
    types::{CryptoRngCore, Timestamp},
};

/// A dependency of a `Vertex` that can be satisfied by one or more other vertices.
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) enum Dependency<C: Context> {
    Vote(C::Hash),
    Evidence(ValidatorIndex),
    Endorsement(C::Hash),
}

/// An element of the protocol state, that might depend on other elements.
///
/// It is the vertex in a directed acyclic graph, whose edges are dependencies.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) enum Vertex<C: Context> {
    Vote(SignedWireVote<C>),
    Evidence(Evidence<C>),
    Endorsements(Endorsements<C>),
}

impl<C: Context> Vertex<C> {
    /// Returns the consensus value mentioned in this vertex, if any.
    ///
    /// These need to be validated before passing the vertex into the protocol state. E.g. if
    /// `C::ConsensusValue` is a transaction, it should be validated first (correct signature,
    /// structure, gas limit, etc.). If it is a hash of a transaction, the transaction should be
    /// obtained _and_ validated. Only after that, the vertex can be considered valid.
    pub(crate) fn value(&self) -> Option<&C::ConsensusValue> {
        match self {
            Vertex::Vote(swvote) => swvote.wire_vote.value.as_ref(),
            Vertex::Evidence(_) => None,
            Vertex::Endorsements(_) => None,
        }
    }

    /// Returns the vote hash of this vertex (if it is a vote).
    pub(crate) fn vote_hash(&self) -> Option<C::Hash> {
        match self {
            Vertex::Vote(swvote) => Some(swvote.hash()),
            Vertex::Evidence(_) => None,
            Vertex::Endorsements(_) => None,
        }
    }

    /// Returns whether this is evidence, as opposed to other types of vertices.
    pub(crate) fn is_evidence(&self) -> bool {
        match self {
            Vertex::Vote(_) => false,
            Vertex::Evidence(_) => true,
            Vertex::Endorsements(_) => false,
        }
    }

    /// Returns a `Timestamp` provided the vertex is a `Vertex::Vote`
    pub(crate) fn timestamp(&self) -> Option<Timestamp> {
        match self {
            Vertex::Vote(signed_wire_vote) => Some(signed_wire_vote.wire_vote.timestamp),
            Vertex::Evidence(_) => None,
            Vertex::Endorsements(_) => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) struct SignedWireVote<C: Context> {
    pub(crate) wire_vote: WireVote<C>,
    pub(crate) signature: C::Signature,
}

impl<C: Context> SignedWireVote<C> {
    pub(crate) fn new(
        wire_vote: WireVote<C>,
        secret_key: &C::ValidatorSecret,
        rng: &mut dyn CryptoRngCore,
    ) -> Self {
        let signature = secret_key.sign(&wire_vote.hash(), rng);
        SignedWireVote {
            wire_vote,
            signature,
        }
    }

    pub(crate) fn hash(&self) -> C::Hash {
        self.wire_vote.hash()
    }
}

/// A vote as it is sent over the wire, possibly containing a new block.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) struct WireVote<C: Context> {
    pub(crate) panorama: Panorama<C>,
    pub(crate) creator: ValidatorIndex,
    pub(crate) instance_id: C::InstanceId,
    pub(crate) value: Option<C::ConsensusValue>,
    pub(crate) seq_number: u64,
    pub(crate) timestamp: Timestamp,
    pub(crate) round_exp: u8,
}

impl<C: Context> Debug for WireVote<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        /// A type whose debug implementation prints ".." (without the quotes).
        struct Ellipsis;

        impl Debug for Ellipsis {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "..")
            }
        }

        f.debug_struct("WireVote")
            .field("hash()", &self.hash())
            .field("value", &self.value.as_ref().map(|_| Ellipsis))
            .field("creator.0", &self.creator.0)
            .field("instance_id", &self.instance_id)
            .field("seq_number", &self.seq_number)
            .field("timestamp", &self.timestamp.millis())
            .field("panorama", self.panorama.as_ref())
            .field("round_exp", &self.round_exp)
            .field("round_id()", &self.round_id())
            .finish()
    }
}

impl<C: Context> WireVote<C> {
    /// Returns the vote's hash, which is used as a vote identifier.
    // TODO: This involves serializing and hashing. Memoize?
    pub(crate) fn hash(&self) -> C::Hash {
        // TODO: Use serialize_into to avoid allocation?
        <C as Context>::hash(&bincode::serialize(self).expect("serialize WireVote"))
    }

    /// Returns the time at which the round containing this vote began.
    pub(crate) fn round_id(&self) -> Timestamp {
        state::round_id(self.timestamp, self.round_exp)
    }
}
