use super::{
    active_validator::{ActiveValidator, Effect},
    state::{State, VoteError},
    validators::Validators,
    vertex::{Dependency, Vertex},
};
use thiserror::Error;
use tracing::warn;

use crate::components::consensus::highway_core::vertex::SignedWireVote;
use crate::components::consensus::{consensus_protocol::BlockContext, traits::Context};

/// An error due to an invalid vertex.
#[derive(Debug, Error, PartialEq)]
pub(crate) enum VertexError {
    #[error("The vertex contains an invalid vote: `{0}`")]
    Vote(#[from] VoteError),
    #[error("The vertex contains invalid evidence.")]
    Evidence(#[from] EvidenceError),
}

/// An error due to invalid evidence.
#[derive(Debug, Error, PartialEq)]
pub(crate) enum EvidenceError {
    #[error("The perpetrator is not a validator.")]
    UnknownPerpetrator,
}

/// A vertex that has passed initial validation.
///
/// The vertex could not be determined to be invalid based on its contents alone. The remaining
/// checks will be applied once all of its dependencies have been added to `Highway`. (See
/// `ValidVertex`.)
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreValidatedVertex<C: Context>(Vertex<C>);

impl<C: Context> PreValidatedVertex<C> {
    pub(crate) fn vertex(&self) -> &Vertex<C> {
        &self.0
    }
}

impl<C: Context> From<ValidVertex<C>> for PreValidatedVertex<C> {
    fn from(vv: ValidVertex<C>) -> PreValidatedVertex<C> {
        PreValidatedVertex(vv.0)
    }
}

impl<C: Context> From<ValidVertex<C>> for Vertex<C> {
    fn from(vv: ValidVertex<C>) -> Vertex<C> {
        vv.0
    }
}

impl<C: Context> From<PreValidatedVertex<C>> for Vertex<C> {
    fn from(pvv: PreValidatedVertex<C>) -> Vertex<C> {
        pvv.0
    }
}

/// A vertex that has been validated: `Highway` has all its dependencies and can add it to its
/// protocol state.
///
/// Note that this must only be added to the `Highway` instance that created it. Can cause a panic
/// or inconsistent state otherwise.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ValidVertex<C: Context>(Vertex<C>);

#[derive(Debug)]
pub(crate) struct HighwayParams<C: Context> {
    /// The protocol instance ID. This needs to be unique, to prevent replay attacks.
    // TODO: Add this to every `WireVote`?
    pub(crate) instance_id: C::InstanceId,
    /// The validator IDs and weight map.
    pub(crate) validators: Validators<C::ValidatorId>,
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
    active_validator: Option<ActiveValidator<C>>,
}

impl<C: Context> Highway<C> {
    /// Creates a new Highway instance
    pub(crate) fn new(
        params: HighwayParams<C>,
        seed: u64,
        our_id: C::ValidatorId,
        secret: C::ValidatorSecret,
        round_exp: u8,
        timestamp: u64,
    ) -> (Self, Vec<Effect<C>>) {
        let our_index = params.validators.get_index(&our_id);
        let weights = params
            .validators
            .enumerate()
            .map(|(_, val)| val.weight())
            .collect::<Vec<_>>();
        let state = State::new(&weights, seed);
        let (av, effects) = ActiveValidator::new(our_index, secret, round_exp, timestamp, &state);
        let instance = Self {
            params,
            state,
            // TODO: Make it possible to create an instance without an active validator
            active_validator: Some(av),
        };
        (instance, effects)
    }

    /// Does initial validation. Returns an error if the vertex is invalid.
    pub(crate) fn pre_validate_vertex(
        &self,
        vertex: Vertex<C>,
    ) -> Result<PreValidatedVertex<C>, (Vertex<C>, VertexError)> {
        match self.do_pre_validate_vertex(&vertex) {
            Err(err) => Err((vertex, err)),
            Ok(()) => Ok(PreValidatedVertex(vertex)),
        }
    }

    /// Returns the next missing dependency, or `None` if all dependencies of `pvv` are satisfied.
    ///
    /// If this returns `None`, `validate_vertex` can be called.
    pub(crate) fn missing_dependency(&self, pvv: &PreValidatedVertex<C>) -> Option<Dependency<C>> {
        match pvv.vertex() {
            Vertex::Evidence(_) => None,
            Vertex::Vote(vote) => self.state.missing_dependency(&vote.wire_vote.panorama),
        }
    }

    /// Does full validation. Returns an error if the vertex is invalid.
    ///
    /// All dependencies must be added to the state before this validation step.
    pub(crate) fn validate_vertex(
        &self,
        pvv: PreValidatedVertex<C>,
    ) -> Result<ValidVertex<C>, (PreValidatedVertex<C>, VertexError)> {
        match self.do_validate_vertex(pvv.vertex()) {
            Err(err) => Err((pvv, err)),
            Ok(()) => Ok(ValidVertex(pvv.0)),
        }
    }

    /// Add a validated vertex to the protocol state.
    ///
    /// The validation must have been performed by _this_ `Highway` instance.
    /// More precisely: The instance on which `add_valid_vertex` is called must contain everything
    /// (and possibly more) that the instance on which `validate_vertex` was called contained.
    pub(crate) fn add_valid_vertex(
        &mut self,
        ValidVertex(vertex): ValidVertex<C>,
    ) -> Vec<Effect<C>> {
        match vertex {
            Vertex::Vote(vote) => self.add_valid_vote(vote),
            Vertex::Evidence(evidence) => {
                self.state.add_evidence(evidence);
                vec![]
            }
        }
    }

    /// Returns whether the vertex is already part of this protocol state.
    pub(crate) fn has_vertex(&self, vertex: &Vertex<C>) -> bool {
        match vertex {
            Vertex::Vote(vote) => self.state.has_vote(&vote.hash()),
            Vertex::Evidence(evidence) => self.state.has_evidence(evidence.perpetrator()),
        }
    }

    /// Returns a vertex that satisfies the dependency, if available.
    ///
    /// If we send a vertex to a peer who is missing a dependency, they will ask us for it. In that
    /// case, `get_dependency` will always return `Some`, unless the peer is faulty.
    pub(crate) fn get_dependency(&self, dependency: &Dependency<C>) -> Option<ValidVertex<C>> {
        let state = &self.state;
        match dependency {
            Dependency::Vote(hash) => state.wire_vote(hash).map(Vertex::Vote),
            Dependency::Evidence(idx) => state.opt_evidence(*idx).cloned().map(Vertex::Evidence),
        }
        .map(ValidVertex)
    }

    pub(crate) fn handle_timer(&mut self, timestamp: u64) -> Vec<Effect<C>> {
        match self.active_validator.as_mut() {
            None => {
                // TODO: Error?
                // At least add logging about the event.
                warn!(%timestamp, "Observer node was called with `handle_timer` event.");
                vec![]
            }
            Some(av) => av.handle_timer(timestamp, &self.state),
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
                warn!(
                    ?value,
                    ?block_context,
                    "Observer node was called with `propose` event."
                );
                vec![]
            }
            Some(av) => av.propose(value, block_context, &self.state),
        }
    }

    fn on_new_vote(&self, vhash: &C::Hash, timestamp: u64) -> Vec<Effect<C>> {
        self.active_validator
            .as_ref()
            .map_or_else(Vec::new, |av| av.on_new_vote(vhash, timestamp, &self.state))
    }

    /// Performs initial validation and returns an error if `vertex` is invalid. (See
    /// `PreValidatedVertex` and `validate_vertex`.)
    fn do_pre_validate_vertex(&self, vertex: &Vertex<C>) -> Result<(), VertexError> {
        match vertex {
            Vertex::Vote(vote) => {
                if !C::validate_signature(&vote.hash(), self.validator_pk(&vote), &vote.signature) {
                    return Err(VoteError::Signature.into());
                }
                Ok(self.state.pre_validate_vote(vote)?)
            }
            Vertex::Evidence(evidence) => {
                if self.params.validators.contains(evidence.perpetrator()) {
                    Ok(())
                } else {
                    Err(EvidenceError::UnknownPerpetrator.into())
                }
            }
        }
    }

    /// Validates `vertex` and returns an error if it is invalid.
    /// This requires all dependencies to be present.
    fn do_validate_vertex(&self, vertex: &Vertex<C>) -> Result<(), VertexError> {
        match vertex {
            Vertex::Vote(vote) => Ok(self.state.validate_vote(vote)?),
            Vertex::Evidence(_evidence) => Ok(()),
        }
    }

    /// Adds a valid vote to the protocol state.
    ///
    /// Validity must be checked before calling this! Adding an invalid vote will result in a panic
    /// or an inconsistent state.
    fn add_valid_vote(&mut self, swvote: SignedWireVote<C>) -> Vec<Effect<C>> {
        let vote_timestamp = swvote.wire_vote.timestamp;
        let vote_hash = swvote.hash();
        self.state.add_valid_vote(swvote);
        self.on_new_vote(&vote_hash, vote_timestamp)
    }

    /// Returns validator ID of the `swvote` creator.
    fn validator_pk(&self, swvote: &SignedWireVote<C>) -> &C::ValidatorId {
        self.params
            .validators
            .get_by_id(swvote.wire_vote.creator)
            .id()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::components::consensus::{
        highway_core::{
            highway::{Highway, HighwayParams, VertexError, VoteError},
            state::tests::{
                TestContext, ALICE, ALICE_SEC, BOB, BOB_SEC, CAROL, CAROL_SEC, WEIGHTS,
            },
            state::State,
            validators::Validators,
            vertex::{SignedWireVote, Vertex, WireVote},
            vote::Panorama,
        },
        traits::ValidatorSecret,
    };
    use std::iter::FromIterator;

    #[test]
    fn invalid_signature_error() {
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
            creator: ALICE,
            value: Some(0),
            seq_number: 0,
            timestamp: 1,
        };
        let invalid_signature = 1u64;
        let invalid_signature_vote = SignedWireVote {
            wire_vote: wvote.clone(),
            signature: invalid_signature,
        };
        let invalid_vertex = Vertex::Vote(invalid_signature_vote);
        let err = VertexError::Vote(VoteError::Signature);
        let expected = (invalid_vertex.clone(), err);
        assert_eq!(Err(expected), highway.pre_validate_vertex(invalid_vertex));

        // TODO: Also test the `missing_dependency` and `validate_vertex` steps.

        let valid_signature = ALICE_SEC.sign(&wvote.hash());
        let correct_signature_vote = SignedWireVote {
            wire_vote: wvote,
            signature: valid_signature,
        };
        let valid_vertex = Vertex::Vote(correct_signature_vote);
        let pvv = highway.pre_validate_vertex(valid_vertex).unwrap();
        assert_eq!(None, highway.missing_dependency(&pvv));
        let vv = highway.validate_vertex(pvv).unwrap();
        assert!(highway.add_valid_vertex(vv).is_empty());
    }
}
