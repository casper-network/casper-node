mod vertex;

pub(crate) use crate::components::consensus::highway_core::state::Params;
pub(crate) use vertex::{Dependency, SignedWireVote, Vertex, WireVote};

use rand::{CryptoRng, Rng};
use thiserror::Error;
use tracing::{debug, error, info};

use crate::{
    components::consensus::{
        consensus_protocol::BlockContext,
        highway_core::{
            active_validator::{ActiveValidator, Effect},
            evidence::EvidenceError,
            state::{State, VoteError},
            validators::{Validator, Validators},
        },
        traits::Context,
    },
    types::Timestamp,
};

/// An error due to an invalid vertex.
#[derive(Debug, Error, PartialEq)]
pub(crate) enum VertexError {
    #[error("The vertex contains an invalid vote: `{0}`")]
    Vote(#[from] VoteError),
    #[error("The vertex contains invalid evidence.")]
    Evidence(#[from] EvidenceError),
}

/// A vertex that has passed initial validation.
///
/// The vertex could not be determined to be invalid based on its contents alone. The remaining
/// checks will be applied once all of its dependencies have been added to `Highway`. (See
/// `ValidVertex`.)
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreValidatedVertex<C: Context>(Vertex<C>);

impl<C: Context> PreValidatedVertex<C> {
    pub(crate) fn inner(&self) -> &Vertex<C> {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn into_vertex(self) -> Vertex<C> {
        self.0
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
pub(crate) struct ValidVertex<C: Context>(pub(super) Vertex<C>);

impl<C: Context> ValidVertex<C> {
    pub(crate) fn inner(&self) -> &Vertex<C> {
        &self.0
    }
}

/// A passive instance of the Highway protocol, containing its local state.
///
/// Both observers and active validators must instantiate this, pass in all incoming vertices from
/// peers, and use a [FinalityDetector](../finality_detector/struct.FinalityDetector.html) to
/// determine the outcome of the consensus process.
#[derive(Debug)]
pub(crate) struct Highway<C: Context> {
    /// The protocol instance ID. This needs to be unique, to prevent replay attacks.
    instance_id: C::InstanceId,
    /// The validator IDs and weight map.
    validators: Validators<C::ValidatorId>,
    /// The abstract protocol state.
    state: State<C>,
    /// The state of an active validator, who is participating and creating new vertices.
    active_validator: Option<ActiveValidator<C>>,
}

impl<C: Context> Highway<C> {
    /// Creates a new `Highway` instance. All participants must agree on the protocol parameters.
    ///
    /// Arguments:
    ///
    /// * `instance_id`: A unique identifier for every execution of the protocol (e.g. for every
    ///   era) to prevent replay attacks.
    /// * `validators`: The set of validators and their weights.
    /// * `params`: The Highway protocol parameters.
    pub(crate) fn new(
        instance_id: C::InstanceId,
        validators: Validators<C::ValidatorId>,
        params: Params,
    ) -> Highway<C> {
        info!(?validators, "creating Highway instance {:?}", instance_id);
        let state = State::new(validators.iter().map(Validator::weight), params);
        Highway {
            instance_id,
            validators,
            state,
            active_validator: None,
        }
    }

    /// Turns this instance from a passive observer into an active validator that proposes new
    /// blocks and creates and signs new vertices.
    ///
    /// Panics if `id` is not the ID of a validator with a weight in this Highway instance.
    pub(crate) fn activate_validator(
        &mut self,
        id: C::ValidatorId,
        secret: C::ValidatorSecret,
        start_time: Timestamp,
    ) -> Vec<Effect<C>> {
        assert!(
            self.active_validator.is_none(),
            "activate_validator called twice"
        );
        let idx = self
            .validators
            .get_index(&id)
            .expect("missing own validator ID");
        let (av, effects) = ActiveValidator::new(idx, secret, start_time, &self.state);
        self.active_validator = Some(av);
        effects
    }

    /// Turns this instance into a passive observer, that does not create any new vertices.
    pub(crate) fn deactivate_validator(&mut self) {
        self.active_validator = None;
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
        match pvv.inner() {
            Vertex::Evidence(_) => None,
            Vertex::Vote(vote) => vote.wire_vote.panorama.missing_dependency(&self.state),
        }
    }

    /// Does full validation. Returns an error if the vertex is invalid.
    ///
    /// All dependencies must be added to the state before this validation step.
    pub(crate) fn validate_vertex(
        &self,
        pvv: PreValidatedVertex<C>,
    ) -> Result<ValidVertex<C>, (PreValidatedVertex<C>, VertexError)> {
        match self.do_validate_vertex(pvv.inner()) {
            Err(err) => Err((pvv, err)),
            Ok(()) => Ok(ValidVertex(pvv.0)),
        }
    }

    /// Add a validated vertex to the protocol state.
    ///
    /// The validation must have been performed by _this_ `Highway` instance.
    /// More precisely: The instance on which `add_valid_vertex` is called must contain everything
    /// (and possibly more) that the instance on which `validate_vertex` was called contained.
    pub(crate) fn add_valid_vertex<R: Rng + CryptoRng + ?Sized>(
        &mut self,
        ValidVertex(vertex): ValidVertex<C>,
        rng: &mut R,
    ) -> Vec<Effect<C>> {
        if !self.has_vertex(&vertex) {
            match vertex {
                Vertex::Vote(vote) => self.add_valid_vote(vote, rng),
                Vertex::Evidence(evidence) => {
                    self.state.add_evidence(evidence);
                    vec![]
                }
            }
        } else {
            vec![]
        }
    }

    /// Returns whether the vertex is already part of this protocol state.
    pub(crate) fn has_vertex(&self, vertex: &Vertex<C>) -> bool {
        match vertex {
            Vertex::Vote(vote) => self.state.has_vote(&vote.hash()),
            Vertex::Evidence(evidence) => self.state.has_evidence(evidence.perpetrator()),
        }
    }

    /// Returns whether we have a vertex that satisfies the dependency.
    pub(crate) fn has_dependency(&self, dependency: &Dependency<C>) -> bool {
        match dependency {
            Dependency::Vote(hash) => self.state.has_vote(hash),
            Dependency::Evidence(idx) => self.state.has_evidence(*idx),
        }
    }

    /// Returns a vertex that satisfies the dependency, if available.
    ///
    /// If we send a vertex to a peer who is missing a dependency, they will ask us for it. In that
    /// case, `get_dependency` will always return `Some`, unless the peer is faulty.
    pub(crate) fn get_dependency(&self, dependency: &Dependency<C>) -> Option<ValidVertex<C>> {
        let state = &self.state;
        match dependency {
            Dependency::Vote(hash) => state
                .wire_vote(hash, self.instance_id.clone())
                .map(Vertex::Vote),
            Dependency::Evidence(idx) => state.opt_evidence(*idx).cloned().map(Vertex::Evidence),
        }
        .map(ValidVertex)
    }

    pub(crate) fn handle_timer<R: Rng + CryptoRng + ?Sized>(
        &mut self,
        timestamp: Timestamp,
        rng: &mut R,
    ) -> Vec<Effect<C>> {
        let instance_id = self.instance_id.clone();
        self.map_active_validator(
            |av, state, rng| av.handle_timer(timestamp, state, instance_id, rng),
            rng,
        )
        .unwrap_or_else(|| {
            debug!(%timestamp, "Ignoring `handle_timer` event: only an observer node.");
            vec![]
        })
    }

    pub(crate) fn propose<R: Rng + CryptoRng + ?Sized>(
        &mut self,
        value: C::ConsensusValue,
        block_context: BlockContext,
        rng: &mut R,
    ) -> Vec<Effect<C>> {
        let instance_id = self.instance_id.clone();
        self.map_active_validator(
            |av, state, rng| av.propose(value, block_context, state, instance_id, rng),
            rng,
        )
        .unwrap_or_else(|| {
            debug!("ignoring `propose` event: validator has been deactivated");
            vec![]
        })
    }

    pub(crate) fn validators(&self) -> &Validators<C::ValidatorId> {
        &self.validators
    }

    pub(super) fn state(&self) -> &State<C> {
        &self.state
    }

    fn on_new_vote<R: Rng + CryptoRng + ?Sized>(
        &mut self,
        vhash: &C::Hash,
        timestamp: Timestamp,
        rng: &mut R,
    ) -> Vec<Effect<C>> {
        let instance_id = self.instance_id.clone();
        self.map_active_validator(
            |av, state, rng| av.on_new_vote(vhash, timestamp, state, instance_id, rng),
            rng,
        )
        .unwrap_or_default()
    }

    /// Applies `f` if this is an active validator, otherwise returns `None`.
    ///
    /// Newly created vertices are added to the state. If an equivocation of this validator is
    /// detected, it gets deactivated.
    fn map_active_validator<F, R>(&mut self, f: F, rng: &mut R) -> Option<Vec<Effect<C>>>
    where
        F: FnOnce(&mut ActiveValidator<C>, &State<C>, &mut R) -> Vec<Effect<C>>,
        R: Rng + CryptoRng + ?Sized,
    {
        let effects = f(self.active_validator.as_mut()?, &self.state, rng);
        let mut result = vec![];
        for effect in &effects {
            match effect {
                Effect::NewVertex(vv) => result.extend(self.add_valid_vertex(vv.clone(), rng)),
                Effect::WeEquivocated(_) => self.deactivate_validator(),
                Effect::ScheduleTimer(_) | Effect::RequestNewBlock(_) => (),
            }
        }
        result.extend(effects);
        Some(result)
    }

    /// Performs initial validation and returns an error if `vertex` is invalid. (See
    /// `PreValidatedVertex` and `validate_vertex`.)
    fn do_pre_validate_vertex(&self, vertex: &Vertex<C>) -> Result<(), VertexError> {
        match vertex {
            Vertex::Vote(vote) => {
                let v_id = self.validator_id(&vote).ok_or(VoteError::Creator)?;
                if vote.wire_vote.instance_id != self.instance_id {
                    return Err(VoteError::InstanceId.into());
                }
                if !C::verify_signature(&vote.hash(), v_id, &vote.signature) {
                    return Err(VoteError::Signature.into());
                }
                Ok(self.state.pre_validate_vote(vote)?)
            }
            Vertex::Evidence(evidence) => {
                let v_id = self
                    .validators
                    .get_by_index(evidence.perpetrator())
                    .map(Validator::id)
                    .ok_or(EvidenceError::UnknownPerpetrator)?;
                Ok(evidence.validate(v_id)?)
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
    fn add_valid_vote<R: Rng + CryptoRng + ?Sized>(
        &mut self,
        swvote: SignedWireVote<C>,
        rng: &mut R,
    ) -> Vec<Effect<C>> {
        let vote_timestamp = swvote.wire_vote.timestamp;
        let vote_hash = swvote.hash();
        self.state.add_valid_vote(swvote);
        self.on_new_vote(&vote_hash, vote_timestamp, rng)
    }

    /// Returns validator ID of the `swvote` creator, if it exists.
    fn validator_id(&self, swvote: &SignedWireVote<C>) -> Option<&C::ValidatorId> {
        self.validators
            .get_by_index(swvote.wire_vote.creator)
            .map(Validator::id)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::iter::FromIterator;

    use crate::{
        components::consensus::{
            highway_core::{
                highway::{Highway, SignedWireVote, Vertex, VertexError, VoteError, WireVote},
                state::{
                    tests::{
                        TestContext, ALICE, ALICE_SEC, BOB, BOB_SEC, CAROL, CAROL_SEC, WEIGHTS,
                    },
                    Panorama, State,
                },
                validators::Validators,
            },
            traits::ValidatorSecret,
        },
        testing::TestRng,
        types::Timestamp,
    };

    #[test]
    fn invalid_signature_error() {
        let mut rng = TestRng::new();

        let state: State<TestContext> = State::new_test(WEIGHTS, 0);
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
        let mut highway = Highway {
            instance_id: 1u64,
            validators,
            state,
            active_validator: None,
        };
        let wvote = WireVote {
            panorama: Panorama::new(WEIGHTS.len()),
            creator: CAROL,
            instance_id: highway.instance_id,
            value: Some(0),
            seq_number: 0,
            timestamp: Timestamp::zero(),
            round_exp: 4,
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

        let valid_signature = CAROL_SEC.sign(&wvote.hash(), &mut rng);
        let correct_signature_vote = SignedWireVote {
            wire_vote: wvote,
            signature: valid_signature,
        };
        let valid_vertex = Vertex::Vote(correct_signature_vote);
        let pvv = highway.pre_validate_vertex(valid_vertex).unwrap();
        assert_eq!(None, highway.missing_dependency(&pvv));
        let vv = highway.validate_vertex(pvv).unwrap();
        assert!(highway.add_valid_vertex(vv, &mut rng).is_empty());
    }
}
