use super::{
    evidence::Evidence,
    state::{AddVoteError, State},
    traits::Context,
    validators::Validators,
    vertex::{Dependency, Vertex, WireVote},
};
use crate::components::consensus::highway_core::vertex::SignedWireVote;

/// The result of trying to add a vertex to the protocol highway.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AddVertexOutcome<C: Context> {
    /// The vertex was successfully added.
    Success,
    /// The vertex could not be added because it is missing a dependency. The vertex itself is
    /// returned, together with the missing dependency.
    MissingDependency(Vertex<C>, Dependency<C>),
    /// The vertex is invalid and cannot be added to the protocol highway at all.
    // TODO: Distinction — is it the vertex creator's attributable fault?
    Invalid(Vertex<C>),
}

impl<C: Context> From<AddVoteError<C>> for AddVertexOutcome<C> {
    fn from(err: AddVoteError<C>) -> Self {
        // TODO: debug!("Invalid vote: {}", err);
        Self::Invalid(Vertex::Vote(err.swvote))
    }
}

#[derive(Debug)]
pub(crate) struct HighwayParams<C: Context> {
    /// The protocol instance ID. This needs to be unique, to prevent replay attacks.
    // TODO: Add this to every `WireVote`?
    instance_id: C::InstanceId,
    /// The validator IDs and weight map.
    validators: Validators<C::ValidatorId>,
}

/// A passive instance of the Highway protocol, containing its local state.
///
/// Both observers and active validators must instantiate this, pass in all incoming vertices from
/// peers, and use a [FinalityDetector](../finality_detector/struct.FinalityDetector.html) to
/// determine the outcome of the consensus process.
#[derive(Debug)]
pub(crate) struct Highway<C: Context> {
    /// The parameters that remain constant for the duration of this consensus instance.
    params: HighwayParams<C>,
    /// The abstract protocol state.
    state: State<C>,
}

impl<C: Context> Highway<C> {
    /// Try to add an incoming vertex to the protocol state.
    ///
    /// If the vertex is invalid, or if there are dependencies that need to be added first, returns
    /// `Invalid` resp. `MissingDependency`.
    pub(crate) fn add_vertex(&mut self, vertex: Vertex<C>) -> AddVertexOutcome<C> {
        match vertex {
            Vertex::Vote(vote) => self.add_vote(vote),
            Vertex::Evidence(evidence) => self.add_evidence(evidence),
        }
    }

    /// Returns a vertex that satisfies the dependency, if available.
    ///
    /// If we send a vertex to a peer who is missing a dependency, they will ask us for it. In that
    /// case, `get_dependency` will always return `Some`, unless the peer is faulty.
    pub(crate) fn get_dependency(&self, dependency: Dependency<C>) -> Option<Vertex<C>> {
        let state = &self.state;
        match dependency {
            Dependency::Vote(hash) => state.wire_vote(&hash).map(Vertex::Vote),
            Dependency::Evidence(idx) => state.opt_evidence(idx).cloned().map(Vertex::Evidence),
        }
    }

    fn add_vote(&mut self, swvote: SignedWireVote<C>) -> AddVertexOutcome<C> {
        if !self.params.validators.contains(swvote.wire_vote.sender) {
            return AddVertexOutcome::Invalid(Vertex::Vote(swvote));
        }
        if let Some(dep) = self.state.missing_dependency(&swvote.wire_vote.panorama) {
            return AddVertexOutcome::MissingDependency(Vertex::Vote(swvote), dep);
        }
        // If the vote is invalid, `add_vote` returns it as an error.
        let opt_wvote = self.state.add_vote(swvote).err();
        opt_wvote.map_or(AddVertexOutcome::Success, AddVertexOutcome::from)
    }

    fn add_evidence(&mut self, evidence: Evidence<C>) -> AddVertexOutcome<C> {
        // TODO: Validate evidence. Signatures, sequence numbers, etc.
        if self.params.validators.contains(evidence.perpetrator()) {
            self.state.add_evidence(evidence);
            AddVertexOutcome::Success
        } else {
            AddVertexOutcome::Invalid(Vertex::Evidence(evidence))
        }
    }
}
