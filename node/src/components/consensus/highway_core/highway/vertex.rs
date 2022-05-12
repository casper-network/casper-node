use std::{collections::BTreeSet, fmt::Debug};

use datasize::DataSize;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use casper_types::Timestamp;

use crate::components::consensus::{
    highway_core::{
        endorsement::SignedEndorsement,
        evidence::Evidence,
        highway::{PingError, VertexError},
        state::{self, Panorama},
        validators::{ValidatorIndex, Validators},
    },
    traits::{Context, ValidatorSecret},
};

/// A dependency of a `Vertex` that can be satisfied by one or more other vertices.
#[derive(Clone, DataSize, Debug, Eq, PartialEq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) enum Dependency<C>
where
    C: Context,
{
    Unit(C::Hash),
    Evidence(ValidatorIndex),
    Endorsement(C::Hash),
    Ping(ValidatorIndex, Timestamp),
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
#[derive(Clone, DataSize, Debug, Eq, PartialEq, Serialize, Deserialize, Hash)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) enum Vertex<C>
where
    C: Context,
{
    Unit(SignedWireUnit<C>),
    Evidence(Evidence<C>),
    Endorsements(Endorsements<C>),
    Ping(Ping<C>),
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
            Vertex::Unit(swunit) => swunit.wire_unit().value.as_ref(),
            Vertex::Evidence(_) | Vertex::Endorsements(_) | Vertex::Ping(_) => None,
        }
    }

    /// Returns the unit hash of this vertex (if it is a unit).
    pub(crate) fn unit_hash(&self) -> Option<C::Hash> {
        match self {
            Vertex::Unit(swunit) => Some(swunit.hash()),
            Vertex::Evidence(_) | Vertex::Endorsements(_) | Vertex::Ping(_) => None,
        }
    }

    /// Returns the seq number of this vertex (if it is a unit).
    pub(crate) fn unit_seq_number(&self) -> Option<u64> {
        match self {
            Vertex::Unit(swunit) => Some(swunit.wire_unit().seq_number),
            _ => None,
        }
    }

    /// Returns whether this is evidence, as opposed to other types of vertices.
    pub(crate) fn is_evidence(&self) -> bool {
        matches!(self, Vertex::Evidence(_))
    }

    /// Returns a `Timestamp` provided the vertex is a `Vertex::Unit` or `Vertex::Ping`.
    pub(crate) fn timestamp(&self) -> Option<Timestamp> {
        match self {
            Vertex::Unit(signed_wire_unit) => Some(signed_wire_unit.wire_unit().timestamp),
            Vertex::Ping(ping) => Some(ping.timestamp()),
            Vertex::Evidence(_) | Vertex::Endorsements(_) => None,
        }
    }

    pub(crate) fn creator(&self) -> Option<ValidatorIndex> {
        match self {
            Vertex::Unit(signed_wire_unit) => Some(signed_wire_unit.wire_unit().creator),
            Vertex::Ping(ping) => Some(ping.creator),
            Vertex::Evidence(_) | Vertex::Endorsements(_) => None,
        }
    }

    pub(crate) fn id(&self) -> Dependency<C> {
        match self {
            Vertex::Unit(signed_wire_unit) => Dependency::Unit(signed_wire_unit.hash()),
            Vertex::Evidence(evidence) => Dependency::Evidence(evidence.perpetrator()),
            Vertex::Endorsements(endorsement) => Dependency::Endorsement(endorsement.unit),
            Vertex::Ping(ping) => Dependency::Ping(ping.creator(), ping.timestamp()),
        }
    }

    /// Returns a reference to the unit, or `None` if this is not a unit.
    pub(crate) fn unit(&self) -> Option<&SignedWireUnit<C>> {
        match self {
            Vertex::Unit(signed_wire_unit) => Some(signed_wire_unit),
            _ => None,
        }
    }

    /// Returns true whether unit is a proposal.
    pub(crate) fn is_proposal(&self) -> bool {
        self.value().is_some()
    }
}

#[derive(Clone, DataSize, Debug, Eq, PartialEq, Serialize, Deserialize, Hash)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) struct SignedWireUnit<C>
where
    C: Context,
{
    pub(crate) hashed_wire_unit: HashedWireUnit<C>,
    pub(crate) signature: C::Signature,
}

impl<C: Context> SignedWireUnit<C> {
    pub(crate) fn new(
        hashed_wire_unit: HashedWireUnit<C>,
        secret_key: &C::ValidatorSecret,
    ) -> Self {
        let signature = secret_key.sign(&hashed_wire_unit.hash);
        SignedWireUnit {
            hashed_wire_unit,
            signature,
        }
    }

    pub(crate) fn wire_unit(&self) -> &WireUnit<C> {
        self.hashed_wire_unit.wire_unit()
    }

    pub(crate) fn hash(&self) -> C::Hash {
        self.hashed_wire_unit.hash()
    }
}

#[derive(Clone, DataSize, Debug, Eq, PartialEq, Hash)]
pub(crate) struct HashedWireUnit<C>
where
    C: Context,
{
    hash: C::Hash,
    wire_unit: WireUnit<C>,
}

impl<C> HashedWireUnit<C>
where
    C: Context,
{
    /// Computes the unit's hash and creates a new `HashedWireUnit`.
    pub(crate) fn new(wire_unit: WireUnit<C>) -> Self {
        let hash = wire_unit.compute_hash();
        Self::new_with_hash(wire_unit, hash)
    }

    pub(crate) fn into_inner(self) -> WireUnit<C> {
        self.wire_unit
    }

    pub(crate) fn wire_unit(&self) -> &WireUnit<C> {
        &self.wire_unit
    }

    pub(crate) fn hash(&self) -> C::Hash {
        self.hash
    }

    /// Creates a new `HashedWireUnit`. Make sure the `hash` is correct, and identical with the
    /// result of `wire_unit.compute_hash`.
    pub(crate) fn new_with_hash(wire_unit: WireUnit<C>, hash: C::Hash) -> Self {
        HashedWireUnit { hash, wire_unit }
    }
}

impl<C: Context> Serialize for HashedWireUnit<C> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.wire_unit.serialize(serializer)
    }
}

impl<'de, C: Context> Deserialize<'de> for HashedWireUnit<C> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(HashedWireUnit::new(<_>::deserialize(deserializer)?))
    }
}

/// A unit as it is sent over the wire, possibly containing a new block.
#[derive(Clone, DataSize, Eq, PartialEq, Serialize, Deserialize, Hash)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) struct WireUnit<C>
where
    C: Context,
{
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
    pub(crate) fn into_hashed(self) -> HashedWireUnit<C> {
        HashedWireUnit::new(self)
    }

    /// Returns the time at which the round containing this unit began.
    pub(crate) fn round_id(&self) -> Timestamp {
        state::round_id(self.timestamp, self.round_exp)
    }

    /// Returns the creator's previous unit.
    pub(crate) fn previous(&self) -> Option<&C::Hash> {
        self.panorama[self.creator].correct()
    }

    /// Returns the unit's hash, which is used as a unit identifier.
    fn compute_hash(&self) -> C::Hash {
        // TODO: Use serialize_into to avoid allocation?
        <C as Context>::hash(&bincode::serialize(self).expect("serialize WireUnit"))
    }
}

#[derive(Clone, DataSize, Debug, Eq, PartialEq, Serialize, Deserialize, Hash)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) struct Endorsements<C>
where
    C: Context,
{
    pub(crate) unit: C::Hash,
    pub(crate) endorsers: Vec<(ValidatorIndex, C::Signature)>,
}

impl<C: Context> Endorsements<C> {
    /// Returns hash of the endorsed vote.
    pub fn unit(&self) -> &C::Hash {
        &self.unit
    }

    /// Returns an iterator over validator indexes that endorsed the `unit`.
    pub fn validator_ids(&self) -> impl Iterator<Item = ValidatorIndex> + '_ {
        self.endorsers.iter().map(|(v, _)| *v)
    }
}

impl<C: Context> From<SignedEndorsement<C>> for Endorsements<C> {
    fn from(signed_e: SignedEndorsement<C>) -> Self {
        Endorsements {
            unit: *signed_e.unit(),
            endorsers: vec![(signed_e.validator_idx(), *signed_e.signature())],
        }
    }
}

/// A ping sent by a validator to signal that it is online but has not created new units in a
/// while.
#[derive(Clone, DataSize, Debug, Eq, PartialEq, Serialize, Deserialize, Hash)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) struct Ping<C>
where
    C: Context,
{
    creator: ValidatorIndex,
    timestamp: Timestamp,
    instance_id: C::InstanceId,
    signature: C::Signature,
}

impl<C: Context> Ping<C> {
    /// Creates a new signed ping.
    pub(crate) fn new(
        creator: ValidatorIndex,
        timestamp: Timestamp,
        instance_id: C::InstanceId,
        sk: &C::ValidatorSecret,
    ) -> Self {
        let signature = sk.sign(&Self::hash(creator, timestamp, instance_id));
        Ping {
            creator,
            timestamp,
            instance_id,
            signature,
        }
    }

    /// The creator who signals that it is online.
    pub(crate) fn creator(&self) -> ValidatorIndex {
        self.creator
    }

    /// The timestamp when the ping was created.
    pub(crate) fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// Validates the ping and returns an error if it is not signed by the creator.
    pub(crate) fn validate(
        &self,
        validators: &Validators<C::ValidatorId>,
        our_instance_id: &C::InstanceId,
    ) -> Result<(), VertexError> {
        let Ping {
            creator,
            timestamp,
            instance_id,
            signature,
        } = self;
        if instance_id != our_instance_id {
            return Err(PingError::InstanceId.into());
        }
        let v_id = validators.id(self.creator).ok_or(PingError::Creator)?;
        let hash = Self::hash(*creator, *timestamp, *instance_id);
        if !C::verify_signature(&hash, v_id, signature) {
            return Err(PingError::Signature.into());
        }
        Ok(())
    }

    /// Computes the hash of a ping, i.e. of the creator and timestamp.
    fn hash(creator: ValidatorIndex, timestamp: Timestamp, instance_id: C::InstanceId) -> C::Hash {
        let bytes = bincode::serialize(&(creator, timestamp, instance_id)).expect("serialize Ping");
        <C as Context>::hash(&bytes)
    }
}
