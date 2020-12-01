use std::{collections::BTreeSet, fmt::Debug};

use serde::{Deserialize, Serialize};

use crate::{
    components::consensus::{
        highway_core::{
            endorsement::SignedEndorsement,
            evidence::Evidence,
            state::{self, Panorama},
            validators::ValidatorIndex,
        },
        traits::{Context, ValidatorSecret},
    },
    types::Timestamp,
    NodeRng,
};

/// A dependency of a `Vertex` that can be satisfied by one or more other vertices.
#[derive(Clone, Debug, Eq, PartialEq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) enum Dependency<C: Context> {
    Unit(C::Hash),
    Evidence(ValidatorIndex),
    Endorsement(C::Hash),
}

impl<C: Context> Dependency<C> {
    /// Returns whether this identifies a unit, as opposed to other types of vertices.
    pub(crate) fn is_unit(&self) -> bool {
        matches!(self, Dependency::Unit(_))
    }
}

/// An element of the protocol state, that might depend on other elements.
///
/// It is the vertex in a directed acyclic graph, whose edges are dependencies.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Hash)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) enum Vertex<C: Context> {
    Unit(SignedWireUnit<C>),
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
            Vertex::Unit(swunit) => swunit.wire_unit.value.as_ref(),
            Vertex::Evidence(_) | Vertex::Endorsements(_) => None,
        }
    }

    /// Returns the unit hash of this vertex (if it is a unit).
    pub(crate) fn unit_hash(&self) -> Option<C::Hash> {
        match self {
            Vertex::Unit(swunit) => Some(swunit.hash()),
            Vertex::Evidence(_) | Vertex::Endorsements(_) => None,
        }
    }

    /// Returns whether this is evidence, as opposed to other types of vertices.
    pub(crate) fn is_evidence(&self) -> bool {
        match self {
            Vertex::Unit(_) | Vertex::Endorsements(_) => false,
            Vertex::Evidence(_) => true,
        }
    }

    /// Returns a `Timestamp` provided the vertex is a `Vertex::Unit`
    pub(crate) fn timestamp(&self) -> Option<Timestamp> {
        match self {
            Vertex::Unit(signed_wire_unit) => Some(signed_wire_unit.wire_unit.timestamp),
            Vertex::Evidence(_) => None,
            Vertex::Endorsements(_) => None,
        }
    }

    pub(crate) fn signed_wire_unit(&self) -> Option<&SignedWireUnit<C>> {
        match self {
            Vertex::Unit(signed_wire_unit) => Some(signed_wire_unit),
            _ => None,
        }
    }

    pub(crate) fn id(&self) -> Dependency<C> {
        match self {
            Vertex::Unit(signed_wire_unit) => Dependency::Unit(signed_wire_unit.wire_unit.hash()),
            Vertex::Evidence(evidence) => Dependency::Evidence(evidence.perpetrator()),
            Vertex::Endorsements(endorsement) => Dependency::Endorsement(endorsement.unit),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Hash)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) struct SignedWireUnit<C: Context> {
    pub(crate) wire_unit: WireUnit<C>,
    pub(crate) signature: C::Signature,
}

impl<C: Context> SignedWireUnit<C> {
    pub(crate) fn new(
        wire_unit: WireUnit<C>,
        secret_key: &C::ValidatorSecret,
        rng: &mut NodeRng,
    ) -> Self {
        let signature = secret_key.sign(&wire_unit.hash(), rng);
        SignedWireUnit {
            wire_unit,
            signature,
        }
    }

    pub(crate) fn hash(&self) -> C::Hash {
        self.wire_unit.hash()
    }
}

/// A unit as it is sent over the wire, possibly containing a new block.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Hash)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) struct WireUnit<C: Context> {
    pub(crate) panorama: Panorama<C>,
    pub(crate) creator: ValidatorIndex,
    pub(crate) instance_id: C::InstanceId,
    pub(crate) value: Option<C::ConsensusValue>,
    pub(crate) seq_number: u64,
    pub(crate) timestamp: Timestamp,
    pub(crate) round_exp: u8,
    pub(crate) endorsed: BTreeSet<C::Hash>,
}

impl<C: Context> Debug for WireUnit<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        /// A type whose debug implementation prints ".." (without the quotes).
        struct Ellipsis;

        impl Debug for Ellipsis {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "..")
            }
        }

        f.debug_struct("WireUnit")
            .field("hash()", &self.hash())
            .field("value", &self.value.as_ref().map(|_| Ellipsis))
            .field("creator.0", &self.creator.0)
            .field("instance_id", &self.instance_id)
            .field("seq_number", &self.seq_number)
            .field("timestamp", &self.timestamp.millis())
            .field("panorama", self.panorama.as_ref())
            .field("round_exp", &self.round_exp)
            .field("endorsed", &self.endorsed)
            .field("round_id()", &self.round_id())
            .finish()
    }
}

impl<C: Context> WireUnit<C> {
    /// Returns the unit's hash, which is used as a unit identifier.
    // TODO: This involves serializing and hashing. Memoize?
    pub(crate) fn hash(&self) -> C::Hash {
        // TODO: Use serialize_into to avoid allocation?
        <C as Context>::hash(&bincode::serialize(self).expect("serialize WireUnit"))
    }

    /// Returns the time at which the round containing this unit began.
    pub(crate) fn round_id(&self) -> Timestamp {
        state::round_id(self.timestamp, self.round_exp)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Hash)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) struct Endorsements<C: Context> {
    pub(crate) unit: C::Hash,
    pub(crate) endorsers: Vec<(ValidatorIndex, C::Signature)>,
}

impl<C: Context> Endorsements<C> {
    pub fn new<I: IntoIterator<Item = SignedEndorsement<C>>>(endorsements: I) -> Self {
        let mut iter = endorsements.into_iter().peekable();
        let unit = *iter.peek().expect("non-empty iter").unit();
        let endorsers = iter
            .map(|e| {
                assert_eq!(e.unit(), &unit, "endorsements for different units.");
                (e.validator_idx(), *e.signature())
            })
            .collect();
        Endorsements { unit, endorsers }
    }

    /// Returns hash of the endorsed vode.
    pub fn unit(&self) -> &C::Hash {
        &self.unit
    }

    /// Returns an iterator over validator indexes that endorsed the `unit`.
    pub fn validator_ids(&self) -> impl Iterator<Item = &ValidatorIndex> {
        self.endorsers.iter().map(|(v, _)| v)
    }
}
