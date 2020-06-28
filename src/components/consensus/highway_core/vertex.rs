use std::iter;

use serde::{Deserialize, Serialize};

use super::{evidence::Evidence, validators::ValidatorIndex, vote::Panorama};
use crate::components::consensus::traits::{Context, ValidatorSecret};

/// A dependency of a `Vertex` that can be satisfied by one or more other vertices.
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) enum Dependency<C: Context> {
    Vote(C::Hash),
    Evidence(ValidatorIndex),
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
        }
    }

    pub(crate) fn id(&self) -> Dependency<C> {
        match self {
            Vertex::Vote(swvote) => Dependency::Vote(swvote.hash()),
            Vertex::Evidence(ev) => Dependency::Evidence(ev.perpetrator()),
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
    pub(crate) signature: <C::ValidatorSecret as ValidatorSecret>::Signature,
}

impl<C: Context> SignedWireVote<C> {
    pub(crate) fn new(wire_vote: WireVote<C>, secret_key: &C::ValidatorSecret) -> Self {
        let signature = secret_key.sign(&wire_vote.hash());
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
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) struct WireVote<C: Context> {
    pub(crate) panorama: Panorama<C>,
    pub(crate) sender: ValidatorIndex,
    pub(crate) value: Option<C::ConsensusValue>,
    pub(crate) seq_number: u64,
    pub(crate) instant: u64,
}

impl<C: Context> WireVote<C> {
    /// Returns the vote's hash, which is used as a vote identifier.
    // TODO: This involves serializing and hashing. Memoize?
    pub(crate) fn hash(&self) -> C::Hash {
        // TODO: Use serialize_into to avoid allocation?
        <C as Context>::hash(&bincode::serialize(self).expect("serialize WireVote"))
    }
}
