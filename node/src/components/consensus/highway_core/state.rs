mod block;
mod panorama;
mod params;
mod tallies;
mod unit;
mod weight;

#[cfg(test)]
pub(crate) mod tests;

pub(crate) use params::Params;
use quanta::Clock;
pub(crate) use weight::Weight;

pub(super) use panorama::{Observation, Panorama};
pub(super) use unit::Unit;

use std::{
    borrow::Borrow,
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    convert::identity,
    iter,
};

use itertools::Itertools;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use thiserror::Error;
use tracing::{error, info, trace};

use crate::{
    components::consensus::{
        highway_core::{
            endorsement::{Endorsement, SignedEndorsement},
            evidence::Evidence,
            highway::{Endorsements, SignedWireUnit, WireUnit},
            validators::{ValidatorIndex, ValidatorMap},
        },
        traits::Context,
    },
    types::{TimeDiff, Timestamp},
    utils::weighted_median,
};
use block::Block;
use tallies::Tallies;

#[derive(Debug, Error, PartialEq, Clone)]
pub(crate) enum UnitError {
    #[error("The unit is a ballot but doesn't cite any block.")]
    MissingBlock,
    #[error("The panorama's length {} doesn't match the number of validators.", _0)]
    PanoramaLength(usize),
    #[error("The unit accuses its own creator as faulty.")]
    FaultyCreator,
    #[error("The panorama has a unit from {:?} in the slot for {:?}.", _0, _1)]
    PanoramaIndex(ValidatorIndex, ValidatorIndex),
    #[error("The panorama is missing units indirectly cited via {:?}.", _0)]
    InconsistentPanorama(ValidatorIndex),
    #[error("The unit contains the wrong sequence number.")]
    SequenceNumber,
    #[error("The unit's timestamp is older than a justification's.")]
    Timestamps,
    #[error("The creator is not a validator.")]
    Creator,
    #[error("The unit was created for a wrong instance ID.")]
    InstanceId,
    #[error("The signature is invalid.")]
    Signature,
    #[error("The round length exponent has somehow changed within a round.")]
    RoundLengthExpChangedWithinRound,
    #[error("The round length exponent is less than the minimum allowed by the chain-spec.")]
    RoundLengthExpLessThanMinimum,
    #[error("The round length exponent is greater than the maximum allowed by the chain-spec.")]
    RoundLengthExpGreaterThanMaximum,
    #[error("This would be the third unit in that round. Only two are allowed.")]
    ThreeUnitsInRound,
    #[error(
        "A block must be the leader's ({:?}) first unit, at the beginning of the round.",
        _0
    )]
    NonLeaderBlock(ValidatorIndex),
    #[error("The unit is a block, but its parent is already a terminal block.")]
    ValueAfterTerminalBlock,
    #[error("The unit's creator is banned.")]
    Banned,
    #[error("The unit's endorsed units were not a superset of its justifications.")]
    EndorsementsNotMonotonic,
    #[error("The LNC rule was violated. Vote cited ({:?}) naively.", _0)]
    LncNaiveCitation(ValidatorIndex),
    #[error(
        "Wire unit endorses hash but does not see it. Hash: {:?}; Wire unit: {:?}",
        hash,
        wire_unit
    )]
    EndorsedButUnseen { hash: String, wire_unit: String },
}

/// A reason for a validator to be marked as faulty.
///
/// The `Banned` state is fixed from the beginning and can't be replaced. However, `Indirect` can
/// be replaced with `Direct` evidence, which has the same effect but doesn't rely on information
/// from other consensus protocol instances.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Fault<C: Context> {
    /// The validator was known to be faulty from the beginning. All their messages are considered
    /// invalid in this Highway instance.
    Banned,
    /// We have direct evidence of the validator's fault.
    Direct(Evidence<C>),
    /// The validator is known to be faulty, but the evidence is not in this Highway instance.
    Indirect,
}

impl<C: Context> Fault<C> {
    pub(crate) fn evidence(&self) -> Option<&Evidence<C>> {
        match self {
            Fault::Banned | Fault::Indirect => None,
            Fault::Direct(ev) => Some(ev),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Panoramas<C: Context> {
    /// The full panorama, corresponding to the complete protocol state.
    panorama: Panorama<C>,
    /// Panorama used when creating new units.
    /// In the presence of faults may lag behind `panorama`.
    /// Units, that if cited by a new unit would make that unit violate the LNC, are not added to
    /// `citable_panorama`.
    citable_panorama: Panorama<C>,
}

impl<C: Context> Panoramas<C> {
    fn new(panorama: Panorama<C>, citable_panorama: Panorama<C>) -> Self {
        Panoramas {
            panorama,
            citable_panorama,
        }
    }

    /// Returns the complete protocol state's latest panorama.
    pub(crate) fn panorama(&self) -> &Panorama<C> {
        &self.panorama
    }

    /// Returns the citable panorama.
    pub(crate) fn citable_panorama(&self) -> &Panorama<C> {
        &self.citable_panorama
    }

    /// Marks validator at `idx` as faulty.
    pub(crate) fn mark_faulty(&mut self, idx: ValidatorIndex) {
        self.panorama[idx] = Observation::Faulty;
        self.citable_panorama[idx] = Observation::Faulty;
    }

    /// Updates latest observation of `creator` as `Correct(unit)`.
    /// Once a unit is a endorsed it is safe to cite it.
    pub(crate) fn endorsed(&mut self, _creator: ValidatorIndex, _unit: C::Hash) {
        //TODO
    }

    /// Updates panoramas with the new observation.
    /// `citable_panorama` will be updated only if it won't violate the LNC.
    pub(crate) fn update_panorama(&mut self, creator: ValidatorIndex, new_obs: Observation<C>) {
        // TODO(HWY-167): Decide whether new unit should be accepted to the `citable_panorama`.
        self.citable_panorama[creator] = new_obs.clone();
        self.panorama[creator] = new_obs;
    }

    /// Returns the latest observation of `validator`.
    pub(crate) fn get(&self, validator: ValidatorIndex) -> &Observation<C> {
        self.panorama.get(validator)
    }
}

/// A passive instance of the Highway protocol, containing its local state.
///
/// Both observers and active validators must instantiate this, pass in all incoming vertices from
/// peers, and use a [FinalityDetector](../finality_detector/struct.FinalityDetector.html) to
/// determine the outcome of the consensus process.
#[derive(Debug, Clone)]
pub(crate) struct State<C: Context> {
    /// The fixed parameters.
    params: Params,
    /// The validator's voting weights.
    weights: ValidatorMap<Weight>,
    /// Cumulative validator weights: Entry `i` contains the sum of the weights of validators `0`
    /// through `i`.
    cumulative_w: ValidatorMap<Weight>,
    /// All units imported so far, by hash.
    // TODO: HashMaps prevent deterministic tests.
    units: HashMap<C::Hash, Unit<C>>,
    /// All blocks, by hash.
    blocks: HashMap<C::Hash, Block<C>>,
    /// List of faulty validators and their type of fault.
    faults: HashMap<ValidatorIndex, Fault<C>>,
    /// Panoramas that the protocol observed.
    panoramas: Panoramas<C>,
    /// All currently endorsed units, by hash.
    endorsements: HashMap<C::Hash, ValidatorMap<Option<C::Signature>>>,
    /// Units that don't yet have 2/3 of stake endorsing them.
    /// Signatures are stored in a map so that a single validator sending lots of signatures for
    /// different units doesn't cause us to allocate a lot of memory.
    incomplete_endorsements: HashMap<C::Hash, BTreeMap<ValidatorIndex, C::Signature>>,
    /// Clock to track fork choice
    clock: Clock,
}

impl<C: Context> State<C> {
    pub(crate) fn new<I, IB>(weights: I, params: Params, banned: IB) -> State<C>
    where
        I: IntoIterator,
        I::Item: Borrow<Weight>,
        IB: IntoIterator<Item = ValidatorIndex>,
    {
        let weights = ValidatorMap::from(weights.into_iter().map(|w| *w.borrow()).collect_vec());
        assert!(
            weights.len() > 0,
            "cannot initialize Highway with no validators"
        );
        let mut sum = Weight(0);
        let add = |w: &Weight| {
            sum = sum.checked_add(*w).expect("total weight must be < 2^64");
            sum
        };
        let cumulative_w = weights.iter().map(add).collect();
        assert!(sum > Weight(0), "total weight must not be zero");
        let mut panorama = Panorama::new(weights.len());
        let faults: HashMap<_, _> = banned.into_iter().map(|idx| (idx, Fault::Banned)).collect();
        for idx in faults.keys() {
            assert!(
                idx.0 < weights.len() as u32,
                "invalid banned validator index"
            );
            panorama[*idx] = Observation::Faulty;
        }
        let panoramas = Panoramas::new(panorama.clone(), panorama);
        State {
            params,
            weights,
            cumulative_w,
            units: HashMap::new(),
            blocks: HashMap::new(),
            faults,
            panoramas,
            endorsements: HashMap::new(),
            incomplete_endorsements: HashMap::new(),
            clock: Clock::new(),
        }
    }

    /// Returns the fixed parameters.
    pub(crate) fn params(&self) -> &Params {
        &self.params
    }

    /// Returns the number of validators.
    pub(crate) fn validator_count(&self) -> usize {
        self.weights.len()
    }

    /// Returns the `idx`th validator's voting weight.
    pub(crate) fn weight(&self, idx: ValidatorIndex) -> Weight {
        self.weights[idx]
    }

    /// Returns the map of validator weights.
    pub(crate) fn weights(&self) -> &ValidatorMap<Weight> {
        &self.weights
    }

    /// Returns the total weight of all validators marked faulty in this panorama.
    pub(crate) fn faulty_weight_in(&self, panorama: &Panorama<C>) -> Weight {
        panorama
            .iter()
            .zip(&self.weights)
            .filter(|(obs, _)| **obs == Observation::Faulty)
            .map(|(_, w)| *w)
            .sum()
    }

    /// Returns the total weight of all known-faulty validators.
    pub(crate) fn faulty_weight(&self) -> Weight {
        self.faulty_weight_in(self.panorama())
    }

    /// Returns the sum of all validators' voting weights.
    pub(crate) fn total_weight(&self) -> Weight {
        *self
            .cumulative_w
            .as_ref()
            .last()
            .expect("weight list cannot be empty")
    }

    /// Returns evidence against validator nr. `idx`, if present.
    pub(crate) fn opt_evidence(&self, idx: ValidatorIndex) -> Option<&Evidence<C>> {
        self.opt_fault(idx).and_then(Fault::evidence)
    }

    /// Returns endorsements for `unit`, if any.
    pub(crate) fn opt_endorsements(&self, unit: &C::Hash) -> Option<Vec<SignedEndorsement<C>>> {
        self.endorsements.get(unit).map(|signatures| {
            signatures
                .iter_some()
                .map(|(vidx, sig)| SignedEndorsement::new(Endorsement::new(*unit, vidx), *sig))
                .collect()
        })
    }

    /// Returns whether evidence against validator nr. `idx` is known.
    pub(crate) fn has_evidence(&self, idx: ValidatorIndex) -> bool {
        self.opt_evidence(idx).is_some()
    }

    /// Returns whether we have all endorsements for `unit`.
    pub(crate) fn has_all_endorsements<'a, I: IntoIterator<Item = &'a ValidatorIndex>>(
        &self,
        unit: &C::Hash,
        v_ids: I,
    ) -> bool {
        if self.endorsements.contains_key(unit) {
            true // We have enough endorsements for this unit.
        } else if let Some(sigs) = self.incomplete_endorsements.get(unit) {
            v_ids.into_iter().all(|v_id| sigs.contains_key(v_id))
        } else {
            v_ids.into_iter().next().is_none()
        }
    }

    /// Returns whether we have seen enough endorsements for the unit.
    /// Unit is endorsed when it, or its descendant, has more than ≥ ⅔ of units (by weight).
    pub(crate) fn is_endorsed(&self, hash: &C::Hash) -> bool {
        self.endorsements.contains_key(hash)
        // TODO: check if any descendant (from the same creator) of `hash` is endorsed.
    }

    /// Returns hash of unit that needs to be endorsed.
    pub(crate) fn needs_endorsements(&self, unit: &SignedWireUnit<C>) -> Option<C::Hash> {
        unit.wire_unit
            .endorsed
            .iter()
            .find(|hash| !self.endorsements.contains_key(&hash))
            .cloned()
    }

    /// Marks the given validator as faulty, unless it is already banned or we have direct evidence.
    pub(crate) fn mark_faulty(&mut self, idx: ValidatorIndex) {
        self.panoramas.mark_faulty(idx);
        self.faults.entry(idx).or_insert(Fault::Indirect);
    }

    /// Returns the fault type of validator nr. `idx`, if it is known to be faulty.
    pub(crate) fn opt_fault(&self, idx: ValidatorIndex) -> Option<&Fault<C>> {
        self.faults.get(&idx)
    }

    /// Returns whether validator nr. `idx` is known to be faulty.
    pub(crate) fn is_faulty(&self, idx: ValidatorIndex) -> bool {
        self.faults.contains_key(&idx)
    }

    /// Returns an iterator over all faulty validators.
    pub(crate) fn faulty_validators(&self) -> impl Iterator<Item = ValidatorIndex> + '_ {
        self.faults.keys().cloned()
    }

    /// Returns an iterator over latest unit hashes from honest validators.
    pub(crate) fn iter_correct_hashes(&self) -> impl Iterator<Item = &C::Hash> {
        self.panorama().iter_correct_hashes()
    }

    /// Returns the unit with the given hash, if present.
    pub(crate) fn opt_unit(&self, hash: &C::Hash) -> Option<&Unit<C>> {
        self.units.get(hash)
    }

    /// Returns whether the unit with the given hash is known.
    pub(crate) fn has_unit(&self, hash: &C::Hash) -> bool {
        self.units.contains_key(hash)
    }

    /// Returns the unit with the given hash. Panics if not found.
    pub(crate) fn unit(&self, hash: &C::Hash) -> &Unit<C> {
        self.opt_unit(hash).expect("unit hash must exist")
    }

    /// Returns the block contained in the unit with the given hash, if present.
    pub(crate) fn opt_block(&self, hash: &C::Hash) -> Option<&Block<C>> {
        self.blocks.get(hash)
    }

    /// Returns the block contained in the unit with the given hash. Panics if not found.
    pub(crate) fn block(&self, hash: &C::Hash) -> &Block<C> {
        self.opt_block(hash).expect("block hash must exist")
    }

    /// Returns the complete protocol state's latest panorama.
    pub(crate) fn panorama(&self) -> &Panorama<C> {
        &self.panoramas.panorama()
    }

    /// Returns the "safe" panorama, that can be used when creating new units.
    pub(crate) fn citable_panorama(&self) -> &Panorama<C> {
        &self.panoramas.citable_panorama()
    }

    /// Returns the leader in the specified time slot.
    pub(crate) fn leader(&self, timestamp: Timestamp) -> ValidatorIndex {
        let seed = self.params.seed().wrapping_add(timestamp.millis());
        // We select a random one out of the `total_weight` weight units, starting numbering at 1.
        let r = Weight(leader_prng(self.total_weight().0, seed));
        // The weight units are subdivided into intervals that belong to some validator.
        // `cumulative_w[i]` denotes the last weight unit that belongs to validator `i`.
        // `binary_search` returns the first `i` with `cumulative_w[i] >= r`, i.e. the validator
        // who owns the randomly selected weight unit.
        self.cumulative_w.binary_search(&r).unwrap_or_else(identity)
    }

    /// Adds the unit to the protocol state.
    ///
    /// The unit must be valid, and its dependencies satisfied.
    pub(crate) fn add_valid_unit(&mut self, swunit: SignedWireUnit<C>) {
        let wunit = &swunit.wire_unit;
        self.update_panorama(&swunit);
        let hash = wunit.hash();
        let fork_choice = self.fork_choice(&wunit.panorama).cloned();
        let (unit, opt_value) = Unit::new(swunit, fork_choice.as_ref(), self);
        if let Some(value) = opt_value {
            let block = Block::new(fork_choice, value, self);
            self.blocks.insert(hash, block);
        }
        self.units.insert(hash, unit);
    }

    /// Adds direct evidence proving a validator to be faulty, unless that validators is already
    /// banned or we already have other direct evidence.
    pub(crate) fn add_evidence(&mut self, evidence: Evidence<C>) -> bool {
        let idx = evidence.perpetrator();
        match self.faults.get(&idx) {
            Some(&Fault::Banned) | Some(&Fault::Direct(_)) => return false,
            None | Some(&Fault::Indirect) => (),
        }
        // TODO: Should use Display, not Debug!
        trace!(?evidence, "marking validator #{} as faulty", idx.0);
        self.faults.insert(idx, Fault::Direct(evidence));
        self.panoramas.mark_faulty(idx);
        true
    }

    /// Add set of endorsements to the state.
    /// If, after adding, we have collected enough endorsements to consider unit _endorsed_,
    /// it will be *upgraded* to fully endorsed.
    pub(crate) fn add_endorsements(&mut self, endorsements: Endorsements<C>) {
        let unit = *endorsements.unit();
        if self.endorsements.contains_key(&unit) {
            return; // We already have a sufficient number of endorsements.
        }
        info!("Received endorsements of {:?}", unit);
        self.incomplete_endorsements
            .entry(unit)
            .or_default()
            .extend(endorsements.endorsers);
        let endorsed: Weight = self.incomplete_endorsements[&unit]
            .keys()
            .map(|vidx| self.weight(*vidx))
            .sum();
        // Stake required to consider unit to be endorsed.
        let threshold = self.total_weight() / 2;
        if endorsed > threshold {
            info!(%unit, "Unit endorsed by at least 1/2 of validators.");
            let mut fully_endorsed = self.incomplete_endorsements.remove(&unit).unwrap();
            let endorsed_map = self
                .weights()
                .keys()
                .map(|vidx| fully_endorsed.remove(&vidx))
                .collect();
            self.endorsements.insert(unit, endorsed_map);
            // When unit gets endorsed, it becomes safe to cite.
            let creator = self.unit(&unit).creator;
            self.panoramas.endorsed(creator, unit);
        }
    }

    pub(crate) fn wire_unit(
        &self,
        hash: &C::Hash,
        instance_id: C::InstanceId,
    ) -> Option<SignedWireUnit<C>> {
        let unit = self.opt_unit(hash)?.clone();
        let opt_block = self.opt_block(hash);
        let value = opt_block.map(|block| block.value.clone());
        let endorsed = unit.claims_endorsed().cloned().collect();
        let wunit = WireUnit {
            panorama: unit.panorama.clone(),
            creator: unit.creator,
            instance_id,
            value,
            seq_number: unit.seq_number,
            timestamp: unit.timestamp,
            round_exp: unit.round_exp,
            endorsed,
        };
        Some(SignedWireUnit {
            wire_unit: wunit,
            signature: unit.signature,
        })
    }

    /// Returns the fork choice from `pan`'s view, or `None` if there are no blocks yet.
    ///
    /// The correct validators' latest units count as votes for the block they point to, as well as
    /// all of its ancestors. At each level the block with the highest score is selected from the
    /// children of the previously selected block (or from all blocks at height 0), until a block
    /// is reached that has no children with any votes.
    pub(crate) fn fork_choice<'a>(&'a self, pan: &Panorama<C>) -> Option<&'a C::Hash> {
        let start = self.clock.start();
        // Collect all correct votes in a `Tallies` map, sorted by height.
        let to_entry = |(obs, w): (&Observation<C>, &Weight)| {
            let bhash = &self.unit(obs.correct()?).block;
            Some((self.block(bhash).height, bhash, *w))
        };
        let mut tallies: Tallies<C> = pan.iter().zip(&self.weights).filter_map(to_entry).collect();
        loop {
            // Find the highest block that we know is an ancestor of the fork choice.
            let (height, bhash) = tallies.find_decided(self)?;
            // Drop all votes that are not descendants of `bhash`.
            tallies = tallies.filter_descendants(height, bhash, self);
            // If there are no blocks left, `bhash` itself is the fork choice. Otherwise repeat.
            if tallies.is_empty() {
                let end = self.clock.end();
                let delta = self.clock.delta(start, end).as_nanos();
                trace!(%delta,"Time taken for fork-choice to run");
                return Some(bhash);
            }
        }
    }

    /// Returns the ancestor of the block with the given `hash`, on the specified `height`, or
    /// `None` if the block's height is lower than that.
    pub(crate) fn find_ancestor<'a>(
        &'a self,
        hash: &'a C::Hash,
        height: u64,
    ) -> Option<&'a C::Hash> {
        let block = self.block(hash);
        if block.height < height {
            return None;
        }
        if block.height == height {
            return Some(hash);
        }
        let diff = block.height - height;
        // We want to make the greatest step 2^i such that 2^i <= diff.
        let max_i = log2(diff) as usize;
        let i = max_i.min(block.skip_idx.len() - 1);
        self.find_ancestor(&block.skip_idx[i], height)
    }

    /// Returns an error if `swunit` is invalid. This can be called even if the dependencies are
    /// not present yet.
    pub(crate) fn pre_validate_unit(&self, swunit: &SignedWireUnit<C>) -> Result<(), UnitError> {
        let wunit = &swunit.wire_unit;
        let creator = wunit.creator;
        if creator.0 as usize >= self.validator_count() {
            error!("Nonexistent validator should be rejected in Highway::pre_validate_unit.");
            return Err(UnitError::Creator); // Should be unreachable.
        }
        if Some(&Fault::Banned) == self.faults.get(&creator) {
            return Err(UnitError::Banned);
        }
        if wunit.round_exp < self.params.min_round_exp() {
            return Err(UnitError::RoundLengthExpLessThanMinimum);
        }
        if wunit.round_exp > self.params.max_round_exp() {
            return Err(UnitError::RoundLengthExpGreaterThanMaximum);
        }
        if wunit.value.is_none() && !wunit.panorama.has_correct() {
            return Err(UnitError::MissingBlock);
        }
        if wunit.panorama.len() != self.validator_count() {
            return Err(UnitError::PanoramaLength(wunit.panorama.len()));
        }
        if wunit.panorama.get(creator).is_faulty() {
            return Err(UnitError::FaultyCreator);
        }
        Ok(())
    }

    /// Returns an error if `swunit` is invalid. Must only be called once all dependencies have
    /// been added to the state.
    pub(crate) fn validate_unit(&self, swunit: &SignedWireUnit<C>) -> Result<(), UnitError> {
        let wunit = &swunit.wire_unit;
        let creator = wunit.creator;
        let panorama = &wunit.panorama;
        let timestamp = wunit.timestamp;
        panorama.validate(self)?;
        if panorama.iter_correct(self).any(|v| v.timestamp > timestamp) {
            return Err(UnitError::Timestamps);
        }
        if wunit.seq_number != panorama.next_seq_num(self, creator) {
            return Err(UnitError::SequenceNumber);
        }
        let r_id = round_id(timestamp, wunit.round_exp);
        let opt_prev_unit = panorama[creator].correct().map(|vh| self.unit(vh));
        if let Some(prev_unit) = opt_prev_unit {
            if prev_unit.round_exp != wunit.round_exp {
                // The round exponent must not change within a round: Even with respect to the
                // greater of the two exponents, a round boundary must be between the units.
                let max_re = prev_unit.round_exp.max(wunit.round_exp);
                if prev_unit.timestamp >> max_re == timestamp >> max_re {
                    return Err(UnitError::RoundLengthExpChangedWithinRound);
                }
            }
            // There can be at most two units per round: proposal/confirmation and witness.
            if let Some(prev2_unit) = prev_unit.previous().map(|h2| self.unit(h2)) {
                if prev2_unit.round_id() == r_id {
                    return Err(UnitError::ThreeUnitsInRound);
                }
            }
        }
        // All endorsed units from the panorama of this wunit.
        let endorsements_in_panorama = panorama
            .iter_correct_hashes()
            .flat_map(|hash| self.unit(hash).claims_endorsed())
            .collect::<HashSet<_>>();
        if endorsements_in_panorama
            .iter()
            .any(|&e| !wunit.endorsed.iter().any(|h| h == e))
        {
            return Err(UnitError::EndorsementsNotMonotonic);
        }
        if wunit.value.is_some() {
            // If this unit is a block, it must be the first unit in this round, its timestamp must
            // match the round ID, and the creator must be the round leader.
            if opt_prev_unit.map_or(false, |pv| pv.round_id() == r_id)
                || timestamp != r_id
                || self.leader(r_id) != creator
            {
                return Err(UnitError::NonLeaderBlock(self.leader(r_id)));
            }
            // It's not allowed to create a child block of a terminal block.
            let is_terminal = |hash: &C::Hash| self.is_terminal_block(hash);
            if self.fork_choice(panorama).map_or(false, is_terminal) {
                return Err(UnitError::ValueAfterTerminalBlock);
            }
        }
        for hash in &wunit.endorsed {
            if !wunit.panorama._sees(self, hash) {
                return Err(UnitError::EndorsedButUnseen {
                    hash: format!("{:?}", hash),
                    wire_unit: format!("{:?}", wunit),
                });
            }
        }
        match self.validate_lnc(wunit) {
            None => Ok(()),
            Some(vidx) => Err(UnitError::LncNaiveCitation(vidx)),
        }
    }

    /// Returns `true` if the `bhash` is a block that can have no children.
    pub(crate) fn is_terminal_block(&self, bhash: &C::Hash) -> bool {
        self.blocks.get(bhash).map_or(false, |block| {
            block.height + 1 >= self.params.end_height()
                && self.unit(bhash).timestamp >= self.params.end_timestamp()
        })
    }

    /// Updates `self.panorama` with an incoming unit. Panics if dependencies are missing.
    ///
    /// If the new unit is valid, it will just add `Observation::Correct(wunit.hash())` to the
    /// panorama. If it represents an equivocation, it adds `Observation::Faulty` and updates
    /// `self.faults`.
    ///
    /// Panics unless all dependencies of `wunit` have already been added to `self`.
    fn update_panorama(&mut self, swunit: &SignedWireUnit<C>) {
        let wunit = &swunit.wire_unit;
        let creator = wunit.creator;
        let new_obs = match (self.panoramas.get(creator), wunit.panorama.get(creator)) {
            (Observation::Faulty, _) => Observation::Faulty,
            (obs0, obs1) if obs0 == obs1 => Observation::Correct(wunit.hash()),
            (Observation::None, _) => panic!("missing creator's previous unit"),
            (Observation::Correct(hash0), _) => {
                // If we have all dependencies of wunit and still see the sender as correct, the
                // predecessor of wunit must be a predecessor of hash0. So we already have a
                // conflicting unit with the same sequence number:
                let prev0 = self.find_in_swimlane(hash0, wunit.seq_number).unwrap();
                let wunit0 = self.wire_unit(prev0, wunit.instance_id).unwrap();
                self.add_evidence(Evidence::Equivocation(wunit0, swunit.clone()));
                Observation::Faulty
            }
        };
        self.panoramas.update_panorama(wunit.creator, new_obs);
    }

    /// Returns `true` if this is a proposal and the creator is not faulty.
    pub(super) fn is_correct_proposal(&self, unit: &Unit<C>) -> bool {
        !self.is_faulty(unit.creator)
            && self.leader(unit.timestamp) == unit.creator
            && unit.timestamp == round_id(unit.timestamp, unit.round_exp)
    }

    /// Returns the hash of the message with the given sequence number from the creator of `hash`,
    /// or `None` if the sequence number is higher than that of the unit with `hash`.
    fn find_in_swimlane<'a>(&'a self, hash: &'a C::Hash, seq_number: u64) -> Option<&'a C::Hash> {
        let unit = self.unit(hash);
        match unit.seq_number.cmp(&seq_number) {
            Ordering::Equal => Some(hash),
            Ordering::Less => None,
            Ordering::Greater => {
                let diff = unit.seq_number - seq_number;
                // We want to make the greatest step 2^i such that 2^i <= diff.
                let max_i = log2(diff) as usize;
                let i = max_i.min(unit.skip_idx.len() - 1);
                self.find_in_swimlane(&unit.skip_idx[i], seq_number)
            }
        }
    }

    /// Returns an iterator over units (with hashes) by the same creator, in reverse chronological
    /// order, starting with the specified unit.
    pub(crate) fn swimlane<'a>(
        &'a self,
        vhash: &'a C::Hash,
    ) -> impl Iterator<Item = (&'a C::Hash, &'a Unit<C>)> {
        let mut next = Some(vhash);
        iter::from_fn(move || {
            let current = next?;
            let unit = self.unit(current);
            next = unit.previous();
            Some((current, unit))
        })
    }

    /// Returns an iterator over all hashes of ancestors of the block `bhash`, excluding `bhash`
    /// itself. Panics if `bhash` is not the hash of a known block.
    pub(crate) fn ancestor_hashes<'a>(
        &'a self,
        bhash: &'a C::Hash,
    ) -> impl Iterator<Item = &'a C::Hash> {
        let mut next = self.block(bhash).parent();
        iter::from_fn(move || {
            let current = next?;
            next = self.block(current).parent();
            Some(current)
        })
    }

    /// Returns the median round exponent of all the validators that haven't been observed to be
    /// malicious, as seen by the current panorama.
    /// Returns `None` if there are no correct validators in the panorama.
    pub(crate) fn median_round_exp(&self) -> Option<u8> {
        weighted_median(
            self.panorama()
                .iter_correct(self)
                .map(|unit| (unit.round_exp, self.weight(unit.creator))),
        )
    }

    /// Returns `true` if the state contains no units.
    pub(crate) fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    /// Returns the set of units (by hash) that are endorsed and seen from the panorama.
    pub(crate) fn seen_endorsed(&self, pan: &Panorama<C>) -> BTreeSet<C::Hash> {
        // First we collect all units that were already seen as endorsed by earlier units.
        let mut result: BTreeSet<C::Hash> = pan
            .iter_correct_hashes()
            .flat_map(|hash| self.unit(hash).endorsed.iter().cloned())
            .collect();
        // Now add all remaining endorsed units. Since the pan.sees check is expensive, do it only
        // for the ones that are actually new.
        for hash in self.endorsements.keys() {
            if !result.contains(hash) && pan._sees(self, hash) {
                result.insert(*hash);
            }
        }
        result
    }

    /// Validates whether `wunit` violates the Limited Naïveté Criterion (LNC).
    /// Returns index of the first equivocator that was cited naively in violation of the LNC, or
    /// `None` if the LNC is satisfied.
    fn validate_lnc(&self, wunit: &WireUnit<C>) -> Option<ValidatorIndex> {
        let violates_lnc = |eq_idx: &ValidatorIndex| !self.satisfies_lnc_for(wunit, *eq_idx);
        wunit.panorama.iter_faulty().find(violates_lnc)
    }

    // Stub, replace with `_satisfies_lnc_for`
    fn satisfies_lnc_for(&self, _wunit: &WireUnit<C>, _eq_idx: ValidatorIndex) -> bool {
        true
    }

    /// Returns `true` if there is at most one fork by the validator `eq_idx` that is cited naively
    /// by `wunit` or earlier units by the same creator.
    fn _satisfies_lnc_for(&self, wunit: &WireUnit<C>, eq_idx: ValidatorIndex) -> bool {
        // Find all forks by eq_idx that are cited naively by wunit itself.
        // * If it's more than one, return false: the LNC is violated.
        // * If it's none, return true: If the LNC were violated, it would be because of two naive
        //   citations by wunit.creator's earlier units. So the latest of those earlier units would
        //   already be violating the LNC itself, and thus would not have been added to the state.
        // * Otherwise store the unique naively cited fork in naive_by_wunit.
        let mut opt_naive_by_wunit = None;
        {
            // Returns true if any endorsed (according to wunit) unit cites the given unit.
            let seen_by_endorsed =
                |hash| wunit.endorsed.iter().any(|e_hash| self._sees(e_hash, hash));

            // Iterate over all units cited by wunit.
            let mut to_visit: Vec<_> = wunit.panorama.iter_correct_hashes().collect();
            // This set is a filter so that units don't get added to to_visit twice.
            let mut added_to_to_visit: HashSet<_> = to_visit.iter().cloned().collect();
            while let Some(hash) = to_visit.pop() {
                if seen_by_endorsed(hash) {
                    continue; // This unit and everything below is not cited naively.
                }
                let unit = self.unit(hash);
                match &unit.panorama[eq_idx] {
                    Observation::Correct(eq_hash) => {
                        // The unit (and everything it cites) can only see a single fork.
                        // No need to traverse further downward.
                        if !seen_by_endorsed(eq_hash) {
                            // The fork is cited naively!
                            match opt_naive_by_wunit {
                                // It's the first naively cited fork we found.
                                None => opt_naive_by_wunit = Some(eq_hash),
                                Some(other_hash) => {
                                    // If eq_hash is later than other_hash, it is the tip of the
                                    // same fork. If it is earlier, then other_hash is the tip.
                                    if self._sees_correct(eq_hash, other_hash) {
                                        opt_naive_by_wunit = Some(eq_hash);
                                    } else if !self._sees_correct(other_hash, eq_hash) {
                                        return false; // We found two incompatible forks!
                                    }
                                }
                            }
                        }
                    }
                    // No forks are cited by this unit. No need to traverse further.
                    Observation::None => (),
                    // The unit still sees the equivocator as faulty: We need to traverse further
                    // down the graph to find all cited forks.
                    Observation::Faulty => to_visit.extend(
                        unit.panorama
                            .iter_correct_hashes()
                            .filter(|hash| added_to_to_visit.insert(hash)),
                    ),
                }
            }
        }
        let naive_by_wunit = match opt_naive_by_wunit {
            None => return true, // No forks are cited naively by wunit.
            Some(naive_by_wunit) => naive_by_wunit,
        };

        // Iterate over all earlier units by wunit.creator, and find all forks by eq_idx they
        // naively cite. If any of those forks are incompatible with naive_by_wunit, the LNC is
        // violated.
        let mut opt_pred_hash = wunit.panorama[wunit.creator].correct();
        while let Some(pred_hash) = opt_pred_hash {
            let pred_unit = self.unit(pred_hash);
            // Returns true if any endorsed (according to pred_unit) unit cites the given unit.
            let seen_by_endorsed = |hash| {
                pred_unit
                    .endorsed
                    .iter()
                    .any(|e_hash| self._sees(e_hash, hash))
            };
            // Iterate over all units seen by pred_unit.
            let mut to_visit = vec![pred_hash];
            // This set is a filter so that units don't get added to to_visit twice.
            let mut added_to_to_visit: HashSet<_> = to_visit.iter().cloned().collect();
            while let Some(hash) = to_visit.pop() {
                if seen_by_endorsed(hash) {
                    continue; // This unit and everything below is not cited naively.
                }
                let unit = self.unit(hash);
                match &unit.panorama[eq_idx] {
                    Observation::Correct(eq_hash) => {
                        if !seen_by_endorsed(eq_hash)
                            && !self._is_compatible(eq_hash, naive_by_wunit)
                        {
                            return false;
                        }
                    }
                    // No forks are cited by this unit. No need to traverse further.
                    Observation::None => (),
                    // The unit still sees the equivocator as faulty: We need to traverse further
                    // down the graph to find all cited forks.
                    Observation::Faulty => to_visit.extend(
                        unit.panorama
                            .iter_correct_hashes()
                            .filter(|hash| added_to_to_visit.insert(hash)),
                    ),
                }
            }
            if !pred_unit.panorama[eq_idx].is_faulty() {
                // This unit and everything below sees only a single fork of the equivocator. If we
                // haven't found conflicting naively cited forks yet, there are none.
                return true;
            }
            opt_pred_hash = pred_unit.panorama[wunit.creator].correct();
        }
        true // No earlier messages, so no conflicting naively cited forks.
    }

    /// Returns whether the unit with `hash0` sees the one with `hash1` (i.e. `hash0 ≥ hash1`),
    /// and sees `hash1`'s creator as correct.
    fn _sees_correct(&self, hash0: &C::Hash, hash1: &C::Hash) -> bool {
        hash0 == hash1 || self.unit(hash0).panorama._sees_correct(self, hash1)
    }

    /// Returns whether the unit with `hash0` sees the one with `hash1` (i.e. `hash0 ≥ hash1`).
    fn _sees(&self, hash0: &C::Hash, hash1: &C::Hash) -> bool {
        hash0 == hash1 || self.unit(hash0).panorama._sees(self, hash1)
    }

    // Returns whether the units with `hash0` and `hash1` see each other or are equal.
    fn _is_compatible(&self, hash0: &C::Hash, hash1: &C::Hash) -> bool {
        hash0 == hash1
            || self.unit(hash0).panorama._sees(self, hash1)
            || self.unit(hash1).panorama._sees(self, hash0)
    }

    /// Returns the panorama of the confirmation for the leader unit `vhash`.
    pub(crate) fn confirmation_panorama(
        &self,
        own_idx: ValidatorIndex,
        vhash: &C::Hash,
    ) -> Panorama<C> {
        let unit = self.unit(vhash);
        let mut panorama;
        // TODO(HWY-167): Confirmation panorama.
        if let Some(prev_hash) = self.panorama().get(own_idx).correct().cloned() {
            let own_unit = self.unit(&prev_hash);
            panorama = unit.panorama.merge(self, &own_unit.panorama);
            panorama[own_idx] = Observation::Correct(prev_hash);
        } else {
            panorama = unit.panorama.clone();
        }
        panorama[unit.creator] = Observation::Correct(*vhash);
        for faulty_v in self.faulty_validators() {
            panorama[faulty_v] = Observation::Faulty;
        }
        panorama
    }
}

/// Returns the round length, given the round exponent.
pub(super) fn round_len(round_exp: u8) -> TimeDiff {
    TimeDiff::from(1 << round_exp)
}

/// Returns the time at which the round with the given timestamp and round exponent began.
///
/// The boundaries of rounds with length `1 << round_exp` are multiples of that length, in
/// milliseconds since the epoch. So the beginning of the current round is the greatest multiple
/// of `1 << round_exp` that is less or equal to `timestamp`.
pub(crate) fn round_id(timestamp: Timestamp, round_exp: u8) -> Timestamp {
    // The greatest multiple less or equal to the timestamp is the timestamp with the last
    // `round_exp` bits set to zero.
    (timestamp >> round_exp) << round_exp
}

/// Returns the base-2 logarithm of `x`, rounded down,
/// i.e. the greatest `i` such that `2.pow(i) <= x`.
fn log2(x: u64) -> u32 {
    // The least power of two that is strictly greater than x.
    let next_pow2 = (x + 1).next_power_of_two();
    // It's twice as big as the greatest power of two that is less or equal than x.
    let prev_pow2 = next_pow2 >> 1;
    // The number of trailing zeros is its base-2 logarithm.
    prev_pow2.trailing_zeros()
}

/// Returns a pseudorandom `u64` betweend `1` and `upper` (inclusive).
fn leader_prng(upper: u64, seed: u64) -> u64 {
    ChaCha8Rng::seed_from_u64(seed).gen_range(0, upper) + 1
}
