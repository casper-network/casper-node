mod vertex;

pub(crate) use crate::components::consensus::highway_core::state::Params;
pub(crate) use vertex::{
    Dependency, Endorsements, HashedWireUnit, Ping, SignedWireUnit, Vertex, WireUnit,
};

use std::path::PathBuf;

use datasize::DataSize;
use thiserror::Error;
use tracing::{debug, error, info, trace};

use casper_types::{TimeDiff, Timestamp};

use crate::components::consensus::{
    consensus_protocol::BlockContext,
    highway_core::{
        active_validator::{ActiveValidator, Effect},
        evidence::EvidenceError,
        state::{Fault, State, UnitError, Weight},
        validators::{Validator, Validators},
    },
    traits::Context,
};

use super::{
    endorsement::{Endorsement, EndorsementError},
    evidence::Evidence,
    state::Observation,
    validators::ValidatorIndex,
};

/// If a lot of rounds were skipped between two blocks, log at most this many.
const MAX_SKIPPED_PROPOSAL_LOGS: u64 = 10;

/// An error due to an invalid vertex.
#[derive(Debug, Error, PartialEq)]
pub(crate) enum VertexError {
    #[error("The vertex contains an invalid unit: `{0}`")]
    Unit(#[from] UnitError),
    #[error("The vertex contains invalid evidence: `{0}`")]
    Evidence(#[from] EvidenceError),
    #[error("The endorsements contains invalid entry: `{0}`")]
    Endorsement(#[from] EndorsementError),
    #[error("Invalid ping: `{0}`")]
    Ping(#[from] PingError),
}

/// An error due to an invalid ping.
#[derive(Debug, Error, PartialEq)]
pub(crate) enum PingError {
    #[error("The creator is not a validator.")]
    Creator,
    #[error("The signature is invalid.")]
    Signature,
    #[error("The ping is for a different consensus protocol instance.")]
    InstanceId,
}

/// A vertex that has passed initial validation.
///
/// The vertex could not be determined to be invalid based on its contents alone. The remaining
/// checks will be applied once all of its dependencies have been added to `Highway`. (See
/// `ValidVertex`.)
#[derive(Clone, DataSize, Debug, Eq, PartialEq, Hash)]
pub(crate) struct PreValidatedVertex<C>(Vertex<C>)
where
    C: Context;

impl<C: Context> PreValidatedVertex<C> {
    pub(crate) fn inner(&self) -> &Vertex<C> {
        &self.0
    }

    pub(crate) fn timestamp(&self) -> Option<Timestamp> {
        self.0.timestamp()
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
#[derive(Clone, DataSize, Debug, Eq, PartialEq, Hash)]
pub(crate) struct ValidVertex<C>(pub(crate) Vertex<C>)
where
    C: Context;

impl<C: Context> ValidVertex<C> {
    pub(crate) fn inner(&self) -> &Vertex<C> {
        &self.0
    }

    pub(crate) fn is_proposal(&self) -> bool {
        self.0.value().is_some()
    }
    pub(crate) fn endorsements(&self) -> Option<&Endorsements<C>> {
        match &self.0 {
            Vertex::Endorsements(endorsements) => Some(endorsements),
            Vertex::Evidence(_) | Vertex::Unit(_) | Vertex::Ping(_) => None,
        }
    }
}

/// A result indicating whether and how a requested dependency is satisfied.
pub(crate) enum GetDepOutcome<C: Context> {
    /// We don't have this dependency.
    None,
    /// This vertex satisfies the dependency.
    Vertex(ValidVertex<C>),
    /// The dependency must be satisfied by providing evidence against this faulty validator, but
    /// this `Highway` instance does not have direct evidence.
    Evidence(C::ValidatorId),
}

/// A passive instance of the Highway protocol, containing its local state.
///
/// Both observers and active validators must instantiate this, pass in all incoming vertices from
/// peers, and use a [FinalityDetector](../finality_detector/struct.FinalityDetector.html) to
/// determine the outcome of the consensus process.
#[derive(Debug, DataSize)]
pub(crate) struct Highway<C>
where
    C: Context,
{
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
        info!(%validators, instance=%instance_id, "creating Highway instance");
        let weights = validators.iter().map(Validator::weight);
        let banned = validators.iter_banned_idx();
        let cannot_propose = validators.iter_cannot_propose_idx();
        let state = State::new(weights, params, banned, cannot_propose);
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
        current_time: Timestamp,
        unit_hash_file: Option<PathBuf>,
        target_ftt: Weight,
    ) -> Vec<Effect<C>> {
        if self.active_validator.is_some() {
            error!(?id, "activate_validator called twice");
            return vec![];
        }
        let idx = match self.validators.get_index(&id) {
            Some(idx) => idx,
            None => {
                error!(?id, "missing own validator ID");
                return vec![];
            }
        };
        let start_time = current_time.max(self.state.params().start_timestamp());
        let (av, effects) = ActiveValidator::new(
            idx,
            secret,
            current_time,
            start_time,
            &self.state,
            unit_hash_file,
            target_ftt,
            self.instance_id,
        );
        self.active_validator = Some(av);
        self.add_new_own_vertices(effects, current_time)
    }

    /// Turns this instance into a passive observer, that does not create any new vertices.
    pub(crate) fn deactivate_validator(&mut self) {
        self.active_validator = None;
    }

    /// Switches the active validator to a new round exponent.
    pub(crate) fn set_round_exp(&mut self, new_round_exp: u8) {
        if let Some(ref mut av) = self.active_validator {
            av.set_round_exp(new_round_exp);
        }
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
    pub(super) fn missing_dependency(&self, pvv: &PreValidatedVertex<C>) -> Option<Dependency<C>> {
        match pvv.inner() {
            Vertex::Evidence(_) | Vertex::Ping(_) => None,
            Vertex::Endorsements(endorsements) => {
                let unit = *endorsements.unit();
                if !self.state.has_unit(&unit) {
                    Some(Dependency::Unit(unit))
                } else {
                    None
                }
            }
            Vertex::Unit(unit) => unit
                .wire_unit()
                .panorama
                .missing_dependency(&self.state)
                .or_else(|| {
                    self.state
                        .needs_endorsements(unit)
                        .map(Dependency::Endorsement)
                }),
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
    pub(crate) fn add_valid_vertex(
        &mut self,
        ValidVertex(vertex): ValidVertex<C>,
        now: Timestamp,
    ) -> Vec<Effect<C>> {
        if !self.has_vertex(&vertex) {
            match vertex {
                Vertex::Unit(unit) => self.add_valid_unit(unit, now),
                Vertex::Evidence(evidence) => self.add_evidence(evidence),
                Vertex::Endorsements(endorsements) => self.add_endorsements(endorsements),
                Vertex::Ping(ping) => self.add_ping(ping),
            }
        } else {
            vec![]
        }
    }

    /// Returns whether the vertex is already part of this protocol state.
    pub(crate) fn has_vertex(&self, vertex: &Vertex<C>) -> bool {
        match vertex {
            Vertex::Unit(unit) => self.state.has_unit(&unit.hash()),
            Vertex::Evidence(evidence) => self.state.has_evidence(evidence.perpetrator()),
            Vertex::Endorsements(endorsements) => {
                let unit = endorsements.unit();
                self.state
                    .has_all_endorsements(unit, endorsements.validator_ids())
            }
            Vertex::Ping(ping) => self.state.has_ping(ping.creator(), ping.timestamp()),
        }
    }

    /// Returns whether the validator is known to be faulty and we have evidence.
    pub(crate) fn has_evidence(&self, vid: &C::ValidatorId) -> bool {
        self.validators
            .get_index(vid)
            .map_or(false, |vidx| self.state.has_evidence(vidx))
    }

    /// Marks the given validator as faulty, if it exists.
    pub(crate) fn mark_faulty(&mut self, vid: &C::ValidatorId) {
        if let Some(vidx) = self.validators.get_index(vid) {
            self.state.mark_faulty(vidx);
        }
    }

    /// Returns whether we have a vertex that satisfies the dependency.
    pub(crate) fn has_dependency(&self, dependency: &Dependency<C>) -> bool {
        match dependency {
            Dependency::Unit(hash) => self.state.has_unit(hash),
            Dependency::Evidence(idx) => self.state.is_faulty(*idx),
            Dependency::Endorsement(hash) => self.state.is_endorsed(hash),
            Dependency::Ping(_, _) => false, // We don't store signatures; nothing depends on pings.
        }
    }

    /// Returns a vertex that satisfies the dependency, if available.
    ///
    /// If we send a vertex to a peer who is missing a dependency, they will ask us for it. In that
    /// case, `get_dependency` will never return `None`, unless the peer is faulty.
    pub(crate) fn get_dependency(&self, dependency: &Dependency<C>) -> GetDepOutcome<C> {
        match dependency {
            Dependency::Unit(hash) => match self.state.wire_unit(hash, self.instance_id) {
                None => GetDepOutcome::None,
                Some(unit) => GetDepOutcome::Vertex(ValidVertex(Vertex::Unit(unit))),
            },
            Dependency::Evidence(idx) => match self.state.maybe_fault(*idx) {
                None | Some(Fault::Banned) => GetDepOutcome::None,
                Some(Fault::Direct(ev)) => {
                    GetDepOutcome::Vertex(ValidVertex(Vertex::Evidence(ev.clone())))
                }
                Some(Fault::Indirect) => {
                    let vid = self.validators.id(*idx).expect("missing validator").clone();
                    GetDepOutcome::Evidence(vid)
                }
            },
            Dependency::Endorsement(hash) => match self.state.maybe_endorsements(hash) {
                None => GetDepOutcome::None,
                Some(e) => GetDepOutcome::Vertex(ValidVertex(Vertex::Endorsements(e))),
            },
            Dependency::Ping(_, _) => GetDepOutcome::None, // We don't store ping signatures.
        }
    }

    /// Returns a vertex by a validator with the requested sequence number.
    pub(crate) fn get_dependency_by_index(
        &self,
        vid: ValidatorIndex,
        unit_seq: u64,
    ) -> GetDepOutcome<C> {
        let obs = match self.state.panorama().get(vid) {
            Some(obs) => obs,
            None => return GetDepOutcome::None,
        };
        match obs {
            Observation::None => GetDepOutcome::None,
            Observation::Faulty => match self.state.maybe_fault(vid) {
                None | Some(Fault::Banned) => GetDepOutcome::None,
                Some(Fault::Direct(ev)) => {
                    GetDepOutcome::Vertex(ValidVertex(Vertex::Evidence(ev.clone())))
                }
                Some(Fault::Indirect) => match self.validators.id(vid) {
                    Some(vid) => GetDepOutcome::Evidence(vid.clone()),
                    None => GetDepOutcome::None,
                },
            },
            Observation::Correct(last_seen) => self
                .state
                .find_in_swimlane(last_seen, unit_seq)
                .and_then(|req_hash| self.state.wire_unit(req_hash, self.instance_id))
                .map(|swunit| GetDepOutcome::Vertex(ValidVertex(Vertex::Unit(swunit))))
                .unwrap_or_else(|| GetDepOutcome::None),
        }
    }

    pub(crate) fn handle_timer(&mut self, timestamp: Timestamp) -> Vec<Effect<C>> {
        let instance_id = self.instance_id;

        // Here we just use the timer's timestamp, and assume it's ~ Timestamp::now()
        //
        // This is because proposal units, i.e. new blocks, are
        // supposed to have the exact timestamp that matches the
        // beginning of the round (which we use as the "round ID").
        //
        // But at least any discrepancy here can only come from event
        // handling delays in our own node, and not from timestamps
        // set by other nodes.

        self.map_active_validator(
            |av, state| av.handle_timer(timestamp, state, instance_id),
            timestamp,
        )
        .unwrap_or_else(|| {
            debug!(%timestamp, "Ignoring `handle_timer` event: only an observer node.");
            vec![]
        })
    }

    pub(crate) fn propose(
        &mut self,
        value: C::ConsensusValue,
        block_context: BlockContext<C>,
    ) -> Vec<Effect<C>> {
        let instance_id = self.instance_id;

        // We just use the block context's timestamp, which is
        // hopefully not much older than `Timestamp::now()`
        //
        // We do this because essentially what happens is this:
        //
        // 1. We realize it's our turn to propose a block in
        // millisecond 64, so we set a timer.
        //
        // 2. The timer for timestamp 64 fires, and we request deploys
        // for the new block from the block proposer (with 64 in the
        // block context).
        //
        // 3. The block proposer responds and we finally end up here,
        // and can propose the new block. But we still have to use
        // timestamp 64.

        let timestamp = block_context.timestamp();
        self.map_active_validator(
            |av, state| av.propose(value, block_context, state, instance_id),
            timestamp,
        )
        .unwrap_or_else(|| {
            debug!("ignoring `propose` event: validator has been deactivated");
            vec![]
        })
    }

    pub(crate) fn validators(&self) -> &Validators<C::ValidatorId> {
        &self.validators
    }

    /// Returns an iterator over all validators against which we have direct evidence.
    pub(crate) fn validators_with_evidence(&self) -> impl Iterator<Item = &C::ValidatorId> {
        self.validators
            .enumerate_ids()
            .filter(move |(idx, _)| self.state.has_evidence(*idx))
            .map(|(_, v_id)| v_id)
    }

    pub(crate) fn state(&self) -> &State<C> {
        &self.state
    }

    /// Sets the pause status: While paused we don't create any new units, just pings.
    pub(crate) fn set_paused(&mut self, paused: bool) {
        if let Some(av) = &mut self.active_validator {
            av.set_paused(paused);
        }
    }

    /// Drops all state other than evidence.
    pub(crate) fn retain_evidence_only(&mut self) {
        self.deactivate_validator();
        self.state.retain_evidence_only();
    }

    fn on_new_unit(&mut self, uhash: &C::Hash, timestamp: Timestamp) -> Vec<Effect<C>> {
        let instance_id = self.instance_id;
        self.map_active_validator(
            |av, state| av.on_new_unit(uhash, timestamp, state, instance_id),
            timestamp,
        )
        .unwrap_or_default()
    }

    /// Takes action on a new evidence.
    fn on_new_evidence(&mut self, evidence: Evidence<C>) -> Vec<Effect<C>> {
        let state = &self.state;
        let mut effects = self
            .active_validator
            .as_mut()
            .map(|av| av.on_new_evidence(&evidence, state))
            .unwrap_or_default();
        // Add newly created endorsements to the local state. These can only be our own ones, so we
        // don't need to look for conflicts and call State::add_endorsements directly.
        for effect in effects.iter() {
            if let Effect::NewVertex(vv) = effect {
                if let Some(e) = vv.endorsements() {
                    self.state.add_endorsements(e.clone());
                }
            }
        }
        // Gossip `Evidence` only if we just learned about faults by the validator.
        effects.extend(vec![Effect::NewVertex(ValidVertex(Vertex::Evidence(
            evidence,
        )))]);
        effects
    }

    /// Applies `f` if this is an active validator, otherwise returns `None`.
    ///
    /// Newly created vertices are added to the state. If an equivocation of this validator is
    /// detected, it gets deactivated.
    fn map_active_validator<F>(&mut self, f: F, timestamp: Timestamp) -> Option<Vec<Effect<C>>>
    where
        F: FnOnce(&mut ActiveValidator<C>, &State<C>) -> Vec<Effect<C>>,
    {
        let effects = f(self.active_validator.as_mut()?, &self.state);
        Some(self.add_new_own_vertices(effects, timestamp))
    }

    /// Handles all `NewVertex` effects and adds the vertices to the protocol state.
    ///
    /// This needs to be applied to all effects created by `ActiveValidator`, so that new vertices
    /// are not interpreted as coming from a doppelgänger.
    fn add_new_own_vertices(
        &mut self,
        effects: Vec<Effect<C>>,
        timestamp: Timestamp,
    ) -> Vec<Effect<C>> {
        let mut result = Vec::with_capacity(effects.len());
        for effect in &effects {
            match effect {
                Effect::NewVertex(vv) => {
                    result.extend(self.add_valid_vertex(vv.clone(), timestamp))
                }
                Effect::WeAreFaulty(_) => self.deactivate_validator(),
                Effect::ScheduleTimer(_) | Effect::RequestNewBlock(_) => (),
            }
        }
        result.extend(effects);
        result
    }

    /// Performs initial validation and returns an error if `vertex` is invalid. (See
    /// `PreValidatedVertex` and `validate_vertex`.)
    fn do_pre_validate_vertex(&self, vertex: &Vertex<C>) -> Result<(), VertexError> {
        match vertex {
            Vertex::Unit(unit) => {
                let creator = unit.wire_unit().creator;
                let v_id = self.validators.id(creator).ok_or(UnitError::Creator)?;
                if unit.wire_unit().instance_id != self.instance_id {
                    return Err(UnitError::InstanceId.into());
                }
                if !C::verify_signature(&unit.hash(), v_id, &unit.signature) {
                    return Err(UnitError::Signature.into());
                }
                Ok(self.state.pre_validate_unit(unit)?)
            }
            Vertex::Evidence(evidence) => {
                Ok(evidence.validate(&self.validators, &self.instance_id, self.state.params())?)
            }
            Vertex::Endorsements(endorsements) => {
                let unit = *endorsements.unit();
                if endorsements.endorsers.is_empty() {
                    return Err(EndorsementError::Empty.into());
                }
                for (creator, signature) in endorsements.endorsers.iter() {
                    let v_id = self
                        .validators
                        .id(*creator)
                        .ok_or(EndorsementError::Creator)?;
                    if self.state.maybe_fault(*creator) == Some(&Fault::Banned) {
                        return Err(EndorsementError::Banned.into());
                    }
                    let endorsement: Endorsement<C> = Endorsement::new(unit, *creator);
                    if !C::verify_signature(&endorsement.hash(), v_id, signature) {
                        return Err(EndorsementError::Signature.into());
                    }
                }
                Ok(())
            }
            Vertex::Ping(ping) => ping.validate(&self.validators, &self.instance_id),
        }
    }

    /// Validates `vertex` and returns an error if it is invalid.
    /// This requires all dependencies to be present.
    fn do_validate_vertex(&self, vertex: &Vertex<C>) -> Result<(), VertexError> {
        match vertex {
            Vertex::Unit(unit) => Ok(self.state.validate_unit(unit)?),
            Vertex::Evidence(_) | Vertex::Endorsements(_) | Vertex::Ping(_) => Ok(()),
        }
    }

    /// Adds evidence to the protocol state.
    /// Gossip the evidence if it's the first equivocation from the creator.
    fn add_evidence(&mut self, evidence: Evidence<C>) -> Vec<Effect<C>> {
        if self.state.add_evidence(evidence.clone()) {
            self.on_new_evidence(evidence)
        } else {
            vec![]
        }
    }

    /// Adds a valid unit to the protocol state.
    ///
    /// Validity must be checked before calling this! Adding an invalid unit will result in a panic
    /// or an inconsistent state.
    fn add_valid_unit(&mut self, swunit: SignedWireUnit<C>, now: Timestamp) -> Vec<Effect<C>> {
        let unit_hash = swunit.hash();
        let creator = swunit.wire_unit().creator;
        let was_honest = !self.state.is_faulty(creator);
        self.state.add_valid_unit(swunit);
        self.log_if_missing_proposal(&unit_hash);
        let mut evidence_effects = self
            .state
            .maybe_evidence(creator)
            .cloned()
            .map(|ev| {
                if was_honest {
                    self.on_new_evidence(ev)
                } else {
                    vec![]
                }
            })
            .unwrap_or_default();
        evidence_effects.extend(self.on_new_unit(&unit_hash, now));
        evidence_effects.extend(self.add_own_last_unit(now));
        evidence_effects
    }

    /// If validator's protocol state is synchronized, adds its own last unit (if any) to the
    /// protocol state
    fn add_own_last_unit(&mut self, now: Timestamp) -> Vec<Effect<C>> {
        self.map_active_validator(
            |av, state| {
                if av.is_own_last_unit_panorama_sync(state) {
                    if let Some(own_last_unit) = av.take_own_last_unit() {
                        vec![Effect::NewVertex(ValidVertex(Vertex::Unit(own_last_unit)))]
                    } else {
                        vec![]
                    }
                } else {
                    vec![]
                }
            },
            now,
        )
        .unwrap_or_default()
    }

    /// Adds endorsements to the state. If there are conflicting endorsements, `NewVertex` effects
    /// are returned containing evidence to prove them faulty.
    fn add_endorsements(&mut self, endorsements: Endorsements<C>) -> Vec<Effect<C>> {
        let evidence = self
            .state
            .find_conflicting_endorsements(&endorsements, &self.instance_id);
        self.state.add_endorsements(endorsements);
        let add_and_create_effect = |ev: Evidence<C>| {
            self.state.add_evidence(ev.clone());
            Effect::NewVertex(ValidVertex(Vertex::Evidence(ev)))
        };
        evidence.into_iter().map(add_and_create_effect).collect()
    }

    /// Adds a ping to the state.
    fn add_ping(&mut self, ping: Ping<C>) -> Vec<Effect<C>> {
        self.state.add_ping(ping.creator(), ping.timestamp());
        vec![]
    }

    /// Checks whether the unit was created by a doppelganger.
    pub(crate) fn is_doppelganger_vertex(&self, vertex: &Vertex<C>) -> bool {
        self.active_validator
            .as_ref()
            .map_or(false, |av| av.is_doppelganger_vertex(vertex, &self.state))
    }

    /// Returns whether this instance of protocol is an active validator.
    pub(crate) fn is_active(&self) -> bool {
        self.active_validator.is_some()
    }

    /// Returns the instance ID of this Highway instance.
    pub(crate) fn instance_id(&self) -> &C::InstanceId {
        &self.instance_id
    }

    pub(crate) fn next_round_length(&self) -> Option<TimeDiff> {
        self.active_validator
            .as_ref()
            .map(|av| av.next_round_length())
    }

    /// Logs a message if this is a block and any previous blocks were skipped.
    fn log_if_missing_proposal(&self, unit_hash: &C::Hash) {
        let state = &self.state;
        let unit = state.unit(unit_hash);
        let r_id = unit.round_id();
        if unit.timestamp != r_id
            || unit.block != *unit_hash
            || state.leader(r_id) != unit.creator
            || state.is_faulty(unit.creator)
        {
            return; // Not a block by an honest validator. (Don't let faulty validators spam logs.)
        }

        // Iterate over all rounds since the parent — or since the start time, if there is none.
        let parent_timestamp = if let Some(parent_hash) = state.block(unit_hash).parent() {
            state.unit(parent_hash).timestamp
        } else {
            state.params().start_timestamp()
        };
        for skipped_r_id in (1..=MAX_SKIPPED_PROPOSAL_LOGS)
            .map(|i| r_id.saturating_sub(state.params().min_round_length() * i))
            .take_while(|skipped_r_id| *skipped_r_id > parent_timestamp)
        {
            let leader_index = state.leader(skipped_r_id);
            let leader_id = match self.validators.id(leader_index) {
                None => {
                    error!(?leader_index, "missing leader validator ID");
                    return;
                }
                Some(leader_id) => leader_id,
            };
            if state.is_faulty(leader_index) {
                trace!(
                    ?leader_index, %leader_id, round_id = %skipped_r_id,
                    "missing proposal: faulty leader was skipped",
                );
            } else {
                let reason = state.panorama()[leader_index]
                    .correct()
                    .and_then(|leader_hash| {
                        state
                            .swimlane(leader_hash)
                            .find(|(_, unit)| unit.timestamp <= skipped_r_id)
                            .filter(|(_, unit)| unit.timestamp == skipped_r_id)
                    })
                    .map_or("the leader missed their turn", |_| {
                        "the leader's proposal got orphaned"
                    });
                info!(
                    ?leader_index, %leader_id, round_id = %skipped_r_id,
                    "missing proposal: {}", reason,
                );
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{collections::BTreeSet, iter::FromIterator};

    use casper_types::Timestamp;

    use crate::components::consensus::{
        highway_core::{
            evidence::{Evidence, EvidenceError},
            highway::{
                vertex::Ping, Dependency, Highway, SignedWireUnit, UnitError, Vertex, VertexError,
                WireUnit,
            },
            highway_testing::TEST_INSTANCE_ID,
            state::{tests::*, Panorama, State},
            validators::Validators,
        },
        traits::ValidatorSecret,
    };

    pub(crate) fn test_validators() -> Validators<u32> {
        let vid_weights: Vec<(u32, u64)> =
            vec![(ALICE_SEC, ALICE), (BOB_SEC, BOB), (CAROL_SEC, CAROL)]
                .into_iter()
                .map(|(sk, vid)| {
                    assert_eq!(sk.0, vid.0);
                    (sk.0, WEIGHTS[vid.0 as usize].0)
                })
                .collect();
        Validators::from_iter(vid_weights)
    }

    #[test]
    fn invalid_signature_error() {
        let now: Timestamp = 500.into();

        let state: State<TestContext> = State::new_test(WEIGHTS, 0);
        let mut highway = Highway {
            instance_id: TEST_INSTANCE_ID,
            validators: test_validators(),
            state,
            active_validator: None,
        };
        let wunit = WireUnit {
            panorama: Panorama::new(WEIGHTS.len()),
            creator: CAROL,
            instance_id: highway.instance_id,
            value: Some(0),
            seq_number: 0,
            timestamp: Timestamp::zero(),
            round_exp: 4,
            endorsed: BTreeSet::new(),
        };
        let invalid_signature = 1u64;
        let invalid_signature_unit = SignedWireUnit {
            hashed_wire_unit: wunit.clone().into_hashed(),
            signature: invalid_signature,
        };
        let invalid_vertex = Vertex::Unit(invalid_signature_unit);
        let err = VertexError::Unit(UnitError::Signature);
        let expected = (invalid_vertex.clone(), err);
        assert_eq!(Err(expected), highway.pre_validate_vertex(invalid_vertex));

        let hwunit = wunit.into_hashed();
        let valid_signature = CAROL_SEC.sign(&hwunit.hash());
        let correct_signature_unit = SignedWireUnit {
            hashed_wire_unit: hwunit,
            signature: valid_signature,
        };
        let valid_vertex = Vertex::Unit(correct_signature_unit);
        let pvv = highway.pre_validate_vertex(valid_vertex).unwrap();
        assert_eq!(None, highway.missing_dependency(&pvv));
        let vv = highway.validate_vertex(pvv).unwrap();
        assert!(highway.add_valid_vertex(vv, now).is_empty());
    }

    #[test]
    fn missing_dependency() -> Result<(), AddUnitError<TestContext>> {
        let mut state = State::new_test(WEIGHTS, 0);
        let now: Timestamp = 500.into();

        add_unit!(state, CAROL, 0xC0; N, N, N)?;
        add_unit!(state, CAROL, 0xC1; N, N, N)?;
        let a = add_unit!(state, ALICE, 0xA; N, N, N)?;
        endorse!(state, a; ALICE, BOB, CAROL);
        // Bob's unit depends on Alice's unit, an endorsement of Alice's unit, and evidence against
        // Carol.
        let b = add_unit!(state, BOB, 0xB; a, N, F; a)?;

        let end_a = state.maybe_endorsements(&a).expect("unit a is endorsed");
        let ev_c = state.maybe_evidence(CAROL).unwrap().clone();
        let wunit_a = state.wire_unit(&a, TEST_INSTANCE_ID).unwrap();
        let wunit_b = state.wire_unit(&b, TEST_INSTANCE_ID).unwrap();

        let mut highway = Highway {
            instance_id: TEST_INSTANCE_ID,
            validators: test_validators(),
            state: State::new_test(WEIGHTS, 0),
            active_validator: None,
        };

        let vertex_end_a = Vertex::Endorsements(end_a);
        let pvv_a = highway.pre_validate_vertex(Vertex::Unit(wunit_a)).unwrap();
        let pvv_end_a = highway.pre_validate_vertex(vertex_end_a).unwrap();
        let pvv_ev_c = highway.pre_validate_vertex(Vertex::Evidence(ev_c)).unwrap();
        let pvv_b = highway.pre_validate_vertex(Vertex::Unit(wunit_b)).unwrap();

        assert_eq!(
            Some(Dependency::Unit(a)),
            highway.missing_dependency(&pvv_b)
        );
        assert_eq!(
            Some(Dependency::Unit(a)),
            highway.missing_dependency(&pvv_end_a)
        );
        assert_eq!(None, highway.missing_dependency(&pvv_a));
        let vv_a = highway.validate_vertex(pvv_a).unwrap();
        highway.add_valid_vertex(vv_a, now);

        assert_eq!(None, highway.missing_dependency(&pvv_end_a));
        assert_eq!(
            Some(Dependency::Evidence(CAROL)),
            highway.missing_dependency(&pvv_b)
        );
        assert_eq!(None, highway.missing_dependency(&pvv_ev_c));
        let vv_ev_c = highway.validate_vertex(pvv_ev_c).unwrap();
        highway.add_valid_vertex(vv_ev_c, now);

        assert_eq!(
            Some(Dependency::Endorsement(a)),
            highway.missing_dependency(&pvv_b)
        );
        assert_eq!(None, highway.missing_dependency(&pvv_end_a));
        let vv_end_a = highway.validate_vertex(pvv_end_a).unwrap();
        highway.add_valid_vertex(vv_end_a, now);

        assert_eq!(None, highway.missing_dependency(&pvv_b));
        let vv_b = highway.validate_vertex(pvv_b).unwrap();
        highway.add_valid_vertex(vv_b, now);

        Ok(())
    }

    #[test]
    fn invalid_evidence() {
        let state: State<TestContext> = State::new_test(WEIGHTS, 0);
        let highway = Highway {
            instance_id: TEST_INSTANCE_ID,
            validators: test_validators(),
            state,
            active_validator: None,
        };

        let validate = |wunit0: &WireUnit<TestContext>,
                        signer0: &TestSecret,
                        wunit1: &WireUnit<TestContext>,
                        signer1: &TestSecret| {
            let hwunit0 = wunit0.clone().into_hashed();
            let swunit0 = SignedWireUnit::new(hwunit0, signer0);
            let hwunit1 = wunit1.clone().into_hashed();
            let swunit1 = SignedWireUnit::new(hwunit1, signer1);
            let evidence = Evidence::Equivocation(swunit0, swunit1);
            let vertex = Vertex::Evidence(evidence);
            highway
                .pre_validate_vertex(vertex.clone())
                .map_err(|(v, err)| {
                    assert_eq!(v, vertex);
                    err
                })
        };

        // Two units with different values and the same sequence number. Carol equivocated!
        let mut wunit0 = WireUnit {
            panorama: Panorama::new(WEIGHTS.len()),
            creator: CAROL,
            instance_id: highway.instance_id,
            value: Some(0),
            seq_number: 0,
            timestamp: Timestamp::zero(),
            round_exp: 4,
            endorsed: BTreeSet::new(),
        };
        let wunit1 = WireUnit {
            panorama: Panorama::new(WEIGHTS.len()),
            creator: CAROL,
            instance_id: highway.instance_id,
            value: Some(1),
            seq_number: 0,
            timestamp: Timestamp::zero(),
            round_exp: 4,
            endorsed: BTreeSet::new(),
        };

        assert!(validate(&wunit0, &CAROL_SEC, &wunit1, &CAROL_SEC,).is_ok());

        // It's only an equivocation if the two units are different.
        assert_eq!(
            Err(VertexError::Evidence(EvidenceError::EquivocationSameUnit)),
            validate(&wunit0, &CAROL_SEC, &wunit0, &CAROL_SEC)
        );

        // Both units have Carol as their creator; Bob's signature would be invalid.
        assert_eq!(
            Err(VertexError::Evidence(EvidenceError::Signature)),
            validate(&wunit0, &CAROL_SEC, &wunit1, &BOB_SEC)
        );
        assert_eq!(
            Err(VertexError::Evidence(EvidenceError::Signature)),
            validate(&wunit0, &BOB_SEC, &wunit1, &CAROL_SEC)
        );

        // If the first unit was actually Bob's and the second Carol's, nobody equivocated.
        wunit0.creator = BOB;
        assert_eq!(
            Err(VertexError::Evidence(
                EvidenceError::EquivocationDifferentCreators
            )),
            validate(&wunit0, &BOB_SEC, &wunit1, &CAROL_SEC)
        );
        wunit0.creator = CAROL;

        // If the units have different sequence numbers they might belong to the same fork.
        wunit0.seq_number = 1;
        assert_eq!(
            Err(VertexError::Evidence(
                EvidenceError::EquivocationDifferentSeqNumbers
            )),
            validate(&wunit0, &CAROL_SEC, &wunit1, &CAROL_SEC)
        );
        wunit0.seq_number = 0;

        // If the units are from a different network or era we don't accept the evidence.
        wunit0.instance_id = TEST_INSTANCE_ID + 1;
        assert_eq!(
            Err(VertexError::Evidence(EvidenceError::EquivocationInstanceId)),
            validate(&wunit0, &CAROL_SEC, &wunit1, &CAROL_SEC)
        );
    }

    #[test]
    fn invalid_ping_ndrs1077_regression() {
        let now: Timestamp = 500.into();

        let state: State<TestContext> = State::new_test(WEIGHTS, 0);
        let highway = Highway {
            instance_id: TEST_INSTANCE_ID,
            validators: test_validators(),
            state,
            active_validator: None,
        };

        // Ping by validator that is not bonded, with an index that is outside of boundaries of the
        // state.
        let ping: Vertex<TestContext> =
            Vertex::Ping(Ping::new(DAN, now, TEST_INSTANCE_ID, &DAN_SEC));
        assert!(
            DAN.0 >= WEIGHTS.len() as u32,
            "should use validator that is not bonded"
        );
        // Verify that sending a Ping from a non-existing validator does not panic.
        assert!(!highway.has_vertex(&ping));
    }

    #[test]
    fn own_initial_ping_is_not_from_doppelganger() {
        let now: Timestamp = 500.into();
        let later = 501.into();

        let state: State<TestContext> = State::new_test(WEIGHTS, 0);
        let target_ftt = state.total_weight() / 3;
        let mut highway = Highway {
            instance_id: TEST_INSTANCE_ID,
            validators: test_validators(),
            state,
            active_validator: None,
        };

        let _effects =
            highway.activate_validator(ALICE.0, ALICE_SEC.clone(), now, None, target_ftt);

        let ping = Vertex::Ping(Ping::new(ALICE, now, TEST_INSTANCE_ID, &ALICE_SEC));
        assert!(!highway.is_doppelganger_vertex(&ping));
        let ping = Vertex::Ping(Ping::new(ALICE, later, TEST_INSTANCE_ID, &ALICE_SEC));
        assert!(highway.is_doppelganger_vertex(&ping));
    }
}
