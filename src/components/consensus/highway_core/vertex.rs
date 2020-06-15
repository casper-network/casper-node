use std::iter;

use serde::{Deserialize, Serialize};

use super::{evidence::Evidence, traits::Context, validators::ValidatorIndex, vote::Panorama};

/// A dependency of a `Vertex` that can be satisfied by one or more other vertices.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Dependency<C: Context> {
    Vote(C::Hash),
    Evidence(ValidatorIndex),
}

/// An element of the protocol state, that might depend on other elements.
///
/// It is the vertex in a directed acyclic graph, whose edges are dependencies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Vertex<C: Context> {
    Vote(WireVote<C>),
    Evidence(Evidence<C>),
}

impl<C: Context> Vertex<C> {
    /// Returns an iterator over all consensus values mentioned in this vertex.
    ///
    /// These need to be validated before passing the vertex into the protocol state. E.g. if
    /// `C::ConsensusValue` is a transaction, it should be validated first (correct signature,
    /// structure, gas limit, etc.). If it is a hash of a transaction, the transaction should be
    /// obtained _and_ validated. Only after that, the vertex can be considered valid.
    pub(crate) fn values<'a>(&'a self) -> Box<dyn Iterator<Item = &'a C::ConsensusValue> + 'a> {
        match self {
            Vertex::Vote(wvote) => Box::new(wvote.values.iter().flat_map(|v| v.iter())),
            Vertex::Evidence(_) => Box::new(iter::empty()),
        }
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
    pub(crate) values: Option<Vec<C::ConsensusValue>>,
    pub(crate) seq_number: u64,
    pub(crate) instant: u64,
}

impl<C: Context> WireVote<C> {
    /// Returns the vote's hash, which is used as a vote identifier.
    // TODO: This involves serializing and hashing. Memoize?
    pub(crate) fn hash(&self) -> C::Hash {
        // TODO: Use serialize_into to avoid allocation?
        C::hash(&bincode::serialize(self).expect("serialize WireVote"))
    }
}
