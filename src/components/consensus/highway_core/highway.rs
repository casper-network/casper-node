use super::{
    evidence::Evidence,
    state::{AddVoteError, State},
    validators::Validators,
    vertex::{Dependency, Vertex, WireVote},
};
use crate::components::consensus::highway_core::vertex::SignedWireVote;
use crate::components::consensus::traits::Context;

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

    pub(crate) fn state(&self) -> &State<C> {
        &self.state
    }

    fn add_vote(&mut self, swvote: SignedWireVote<C>) -> AddVertexOutcome<C> {
        if !self.params.validators.contains(swvote.wire_vote.sender)
            || swvote.wire_vote.panorama.len() != self.params.validators.len()
        {
            return AddVertexOutcome::Invalid(Vertex::Vote(swvote));
        }
        if !C::validate_signature(
            &swvote.hash(),
            self.validator_pk(&swvote),
            &swvote.signature,
        ) {
            return AddVertexOutcome::Invalid(Vertex::Vote(swvote));
        }
        if let Some(dep) = self.state.missing_dependency(&swvote.wire_vote.panorama) {
            return AddVertexOutcome::MissingDependency(Vertex::Vote(swvote), dep);
        }
        // If the vote is invalid, `add_vote` returns it as an error.
        let opt_wvote = self.state.add_vote(swvote).err();
        opt_wvote.map_or(AddVertexOutcome::Success, AddVertexOutcome::from)
    }

    /// Returns validator ID of the `swvote` sender.
    fn validator_pk(&self, swvote: &SignedWireVote<C>) -> &C::ValidatorId {
        self.params.validators.get_by_id(swvote.wire_vote.sender)
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

#[cfg(test)]
pub(crate) mod tests {
    use crate::components::consensus::highway_core::highway::{
        AddVertexOutcome, Highway, HighwayParams,
    };
    use crate::components::consensus::highway_core::state::tests::{
        TestContext, ALICE, ALICE_SEC, BOB, BOB_SEC, CAROL, CAROL_SEC, WEIGHTS,
    };
    use crate::components::consensus::highway_core::state::{AddVoteError, State, Weight};
    use crate::components::consensus::highway_core::validators::Validators;
    use crate::components::consensus::highway_core::vertex::{SignedWireVote, Vertex, WireVote};
    use crate::components::consensus::highway_core::vote::Panorama;
    use crate::components::consensus::traits::ValidatorSecret;
    use std::iter::FromIterator;

    #[test]
    fn invalid_signature_error() -> Result<(), AddVoteError<TestContext>> {
        let state: State<TestContext> = State::new(WEIGHTS, 0);
        let validators = {
            let vid_weights: Vec<(u32, u64)> =
                vec![(ALICE_SEC, ALICE), (BOB_SEC, BOB), (CAROL_SEC, CAROL)]
                    .into_iter()
                    .map(|(sk, vid)| {
                        assert_eq!(sk.0, vid.0);
                        (sk.0, WEIGHTS[vid.0 as usize].0)
                    })
                    .collect();
            Validators::from_iter(vid_weights)
        };
        let params = HighwayParams {
            instance_id: 1u64,
            validators,
        };
        let mut highway = Highway { params, state };
        let wvote = WireVote {
            panorama: Panorama::new(WEIGHTS.len()),
            sender: ALICE,
            value: Some(0),
            seq_number: 0,
            instant: 1,
        };
        let invalid_signature = 1u64;
        let invalid_signature_vote = SignedWireVote {
            wire_vote: wvote.clone(),
            signature: invalid_signature,
        };
        let invalid_vertex = Vertex::Vote(invalid_signature_vote.clone());
        assert_eq!(
            AddVertexOutcome::Invalid(Vertex::Vote(invalid_signature_vote)),
            highway.add_vertex(invalid_vertex)
        );

        let valid_signature = ALICE_SEC.sign(&wvote.hash());
        let correct_signature_vote = SignedWireVote {
            wire_vote: wvote,
            signature: valid_signature,
        };
        let valid_vertex = Vertex::Vote(correct_signature_vote);
        assert_eq!(AddVertexOutcome::Success, highway.add_vertex(valid_vertex));
        Ok(())
    }
}
