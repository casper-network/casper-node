use super::{
    active_validator::{ActiveValidator, Effect},
    evidence::Evidence,
    state::{AddVoteError, State, VoteError},
    validators::Validators,
    vertex::{Dependency, Vertex, WireVote},
};
use thiserror::Error;
use tracing::warn;

use crate::components::consensus::highway_core::vertex::SignedWireVote;
use crate::components::consensus::{consensus_protocol::BlockContext, traits::Context};

/// An error due to an invalid vertex.
#[derive(Debug, Error)]
pub(crate) enum VertexError {
    #[error("The vertex contains an invalid vote: `{0}`")]
    Vote(#[from] VoteError),
    #[error("The vertex contains invalid evidence.")]
    Evidence(#[from] EvidenceError),
}

/// An error due to invalid evidence.
#[derive(Debug, Error)]
pub(crate) enum EvidenceError {
    #[error("The perpetrator is not a validator.")]
    UnknownPerpetrator,
}

/// The result of trying to add a vertex to the protocol highway.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AddVertexOutcome<C: Context> {
    /// The vertex was successfully added.
    Success(Vec<Effect<C>>),
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
pub(crate) struct Highway<C: Context> {
    /// The parameters that remain constant for the duration of this consensus instance.
    params: HighwayParams<C>,
    /// The abstract protocol state.
    state: State<C>,
    active_validator: Option<ActiveValidator<C>>,
}

impl<C: Context> Highway<C> {
    /// Returns a missing dependency that needs to be added to the protocol state before this one
    /// can be added. Returns an error if the vertex is invalid.
    pub(crate) fn missing_dependency(
        &self,
        vertex: &Vertex<C>,
    ) -> Result<Option<Dependency<C>>, VertexError> {
        match vertex {
            Vertex::Vote(vote) => {
                self.validate_vote(vote)?;
                Ok(self.state.missing_dependency(&vote.wire_vote.panorama))
            }
            Vertex::Evidence(evidence) => {
                if self.params.validators.contains(evidence.perpetrator()) {
                    Ok(None)
                } else {
                    Err(EvidenceError::UnknownPerpetrator.into())
                }
            }
        }
    }

    /// Try to add an incoming vertex to the protocol state.
    ///
    /// If the vertex is invalid, or if there are dependencies that need to be added first, returns
    /// `Invalid` resp. `MissingDependency`.
    pub(crate) fn add_vertex(&mut self, vertex: Vertex<C>) -> AddVertexOutcome<C> {
        match self.missing_dependency(&vertex) {
            Err(err) => AddVertexOutcome::Invalid(vertex),
            Ok(Some(dep)) => AddVertexOutcome::MissingDependency(vertex, dep),
            Ok(None) => match vertex {
                Vertex::Vote(vote) => self.add_vote(vote),
                Vertex::Evidence(evidence) => {
                    self.state.add_evidence(evidence);
                    AddVertexOutcome::Success(vec![])
                }
            },
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

    pub(crate) fn on_new_vote(&self, vhash: &C::Hash, instant: u64) -> Vec<Effect<C>> {
        match self.active_validator.as_ref() {
            None => {
                // TODO: Error?
                warn!(?vhash, %instant, "Observer node was called with `on_new_vote` event.");
                vec![]
            }
            Some(av) => av.on_new_vote(vhash, instant, &self.state),
        }
    }

    pub(crate) fn handle_timer(&mut self, instant: u64) -> Vec<Effect<C>> {
        match self.active_validator.as_mut() {
            None => {
                // TODO: Error?
                // At least add logging about the event.
                warn!(%instant, "Observer node was called with `handle_timer` event.");
                vec![]
            }
            Some(av) => av.handle_timer(instant, &self.state),
        }
    }

    pub(crate) fn propose(
        &self,
        value: C::ConsensusValue,
        block_context: BlockContext,
    ) -> Vec<Effect<C>> {
        match self.active_validator.as_ref() {
            None => {
                // TODO: Error?
                warn!(?value, %block_context.instant, "Observer node was called with `propose` event.");
                vec![]
            }
            Some(av) => av.propose(value, block_context, &self.state),
        }
    }

    pub(crate) fn state(&self) -> &State<C> {
        &self.state
    }

    fn validate_vote(&self, swvote: &SignedWireVote<C>) -> Result<(), VoteError> {
        if !self.params.validators.contains(swvote.wire_vote.sender) {
            Err(VoteError::Creator)
        } else if swvote.wire_vote.panorama.len() != self.params.validators.len() {
            Err(VoteError::Panorama)
        } else if !C::validate_signature(
            &swvote.hash(),
            self.validator_pk(&swvote),
            &swvote.signature,
        ) {
            Err(VoteError::Signature)
        } else {
            Ok(())
        }
    }

    fn add_vote(&mut self, swvote: SignedWireVote<C>) -> AddVertexOutcome<C> {
        let vote_instant = swvote.wire_vote.instant;
        let vote_hash = swvote.hash();
        // If the vote is invalid, `add_vote` returns it as an error.
        match self.state.add_vote(swvote) {
            Ok(()) => {
                let effects = self.on_new_vote(&vote_hash, vote_instant);
                AddVertexOutcome::Success(effects)
            }
            Err(error) => AddVertexOutcome::from(error),
        }
    }

    /// Returns validator ID of the `swvote` sender.
    fn validator_pk(&self, swvote: &SignedWireVote<C>) -> &C::ValidatorId {
        self.params.validators.get_by_id(swvote.wire_vote.sender)
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
        let mut highway = Highway {
            params,
            state,
            active_validator: None,
        };
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
        assert_eq!(
            AddVertexOutcome::Success(vec![]),
            highway.add_vertex(valid_vertex)
        );
        Ok(())
    }
}
