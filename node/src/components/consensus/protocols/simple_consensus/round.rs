use std::{
    collections::{BTreeMap, HashMap},
    fmt::Debug,
};

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::{
    components::consensus::{
        highway_core::validators::{ValidatorIndex, ValidatorMap},
        protocols::simple_consensus::message::{Content, Proposal},
        traits::Context,
    },
    utils::ds,
};

/// The protocol proceeds in rounds, for each of which we must
/// keep track of proposals, echoes, votes, and the current outcome
/// of the round.
#[derive(Debug, DataSize, PartialEq)]
pub(crate) struct Round<C>
where
    C: Context,
{
    /// The leader, who is allowed to create a proposal in this round.
    leader_idx: ValidatorIndex,
    /// The unique proposal signed by the leader, or the unique proposal with a quorum of echoes.
    proposal: Option<Proposal<C>>,
    /// The echoes we've received for each proposal so far.
    #[data_size(with = ds::hashmap_sample)]
    echoes: HashMap<C::Hash, BTreeMap<ValidatorIndex, C::Signature>>,
    /// The votes we've received for this round so far.
    votes: BTreeMap<bool, ValidatorMap<Option<C::Signature>>>,
    /// The memoized results in this round.
    outcome: RoundOutcome<C>,
}

impl<C: Context> Round<C> {
    /// Creates a new [`Round`] with no proposals, echoes, votes, and empty
    /// round outcome.
    pub(super) fn new(validator_count: usize, leader_idx: ValidatorIndex) -> Round<C> {
        let mut votes = BTreeMap::new();
        votes.insert(false, vec![None; validator_count].into());
        votes.insert(true, vec![None; validator_count].into());
        Round {
            leader_idx,
            proposal: None,
            echoes: HashMap::new(),
            votes,
            outcome: RoundOutcome::default(),
        }
    }

    /// Returns the map of all proposals sent to us this round from the leader
    pub(super) fn proposal(&self) -> Option<&Proposal<C>> {
        self.proposal.as_ref()
    }

    /// Returns whether we have received at least one proposal.
    pub(super) fn has_proposal(&self) -> bool {
        self.proposal.is_some()
    }

    /// Returns whether this proposal is justified by an echo signature from the round leader or by
    /// a quorum of echoes.
    pub(super) fn has_echoes_for_proposal(&self, hash: &C::Hash) -> bool {
        match (self.quorum_echoes(), self.echoes.get(hash)) {
            (Some(quorum_hash), _) => quorum_hash == *hash,
            (None, Some(echo_map)) => echo_map.contains_key(&self.leader_idx),
            (None, None) => false,
        }
    }

    /// Inserts a `Proposal` and returns `false` if we already had it or it cannot be added due to
    /// missing echoes.
    pub(super) fn insert_proposal(&mut self, proposal: Proposal<C>) -> bool {
        let hash = proposal.hash();
        if self.has_echoes_for_proposal(&hash) && self.proposal.as_ref() != Some(&proposal) {
            self.proposal = Some(proposal);
            true
        } else {
            false
        }
    }

    /// Returns the echoes we've received for each proposal so far.
    pub(super) fn echoes(&self) -> &HashMap<C::Hash, BTreeMap<ValidatorIndex, C::Signature>> {
        &self.echoes
    }

    /// Inserts an `Echo`; returns `false` if we already had it.
    pub(super) fn insert_echo(
        &mut self,
        hash: C::Hash,
        validator_idx: ValidatorIndex,
        signature: C::Signature,
    ) -> bool {
        self.echoes
            .entry(hash)
            .or_insert_with(BTreeMap::new)
            .insert(validator_idx, signature)
            .is_none()
    }

    /// Returns whether the validator has already sent an `Echo` in this round.
    pub(super) fn has_echoed(&self, validator_idx: ValidatorIndex) -> bool {
        self.echoes
            .values()
            .any(|echo_map| echo_map.contains_key(&validator_idx))
    }

    /// Stores in the outcome that we have a quorum of echoes for this hash.
    pub(super) fn set_quorum_echoes(&mut self, hash: C::Hash) {
        self.outcome.quorum_echoes = Some(hash);
        if self
            .proposal
            .as_ref()
            .map_or(false, |proposal| proposal.hash() != hash)
        {
            self.proposal = None;
        }
    }

    /// Returns the hash for which we have a quorum of echoes, if any.
    pub(super) fn quorum_echoes(&self) -> Option<C::Hash> {
        self.outcome.quorum_echoes
    }

    /// Returns the votes we've received for this round so far.
    pub(super) fn votes(&self, vote: bool) -> &ValidatorMap<Option<C::Signature>> {
        &self.votes[&vote]
    }

    /// Inserts a `Vote`; returns `false` if we already had it.
    pub(super) fn insert_vote(
        &mut self,
        vote: bool,
        validator_idx: ValidatorIndex,
        signature: C::Signature,
    ) -> bool {
        // Safe to unwrap: Both `true` and `false` entries were created in `new`.
        let votes_map = self.votes.get_mut(&vote).unwrap();
        if votes_map[validator_idx].is_none() {
            votes_map[validator_idx] = Some(signature);
            true
        } else {
            false
        }
    }

    /// Returns whether the validator has already cast a `true` or `false` vote.
    pub(super) fn has_voted(&self, validator_idx: ValidatorIndex) -> bool {
        self.votes(true)[validator_idx].is_some() || self.votes(false)[validator_idx].is_some()
    }

    /// Stores in the outcome that we have a quorum of votes for this value.
    pub(super) fn set_quorum_votes(&mut self, vote: bool) {
        self.outcome.quorum_votes = Some(vote);
    }

    /// Returns the value for which we have a quorum of votes, if any.
    pub(super) fn quorum_votes(&self) -> Option<bool> {
        self.outcome.quorum_votes
    }

    /// Removes all votes and echoes from the given validator.
    pub(super) fn remove_votes_and_echoes(&mut self, validator_idx: ValidatorIndex) {
        self.votes.get_mut(&false).unwrap()[validator_idx] = None;
        self.votes.get_mut(&true).unwrap()[validator_idx] = None;
        self.echoes.retain(|_, echo_map| {
            echo_map.remove(&validator_idx);
            !echo_map.is_empty()
        });
    }

    /// Updates the outcome and marks the proposal that has a quorum of echoes as accepted. It also
    /// stores the proposal's block height.
    pub(super) fn set_accepted_proposal_height(&mut self, height: u64) {
        self.outcome.accepted_proposal_height = Some(height);
    }

    /// Returns the accepted proposal, if any, together with its height.
    pub(super) fn accepted_proposal(&self) -> Option<(u64, &Proposal<C>)> {
        let height = self.outcome.accepted_proposal_height?;
        let proposal = self.proposal.as_ref()?;
        Some((height, proposal))
    }

    /// Check if the round has already received this message.
    pub(super) fn contains(&self, content: &Content<C>, validator_idx: ValidatorIndex) -> bool {
        match content {
            Content::Echo(hash) => self
                .echoes
                .get(hash)
                .map_or(false, |echo_map| echo_map.contains_key(&validator_idx)),
            Content::Vote(vote) => self.votes[vote][validator_idx].is_some(),
        }
    }

    /// Removes the proposal: This round was skipped and will never become finalized.
    pub(super) fn prune_skipped(&mut self) {
        self.proposal = None;
        self.outcome.accepted_proposal_height = None;
    }
}

/// Indicates the outcome of a given round.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, DataSize)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) struct RoundOutcome<C>
where
    C: Context,
{
    /// This is `Some(h)` if there is an accepted proposal with relative height `h`, i.e. there is
    /// a quorum of echoes, `h` accepted ancestors, and all rounds since the parent's are
    /// skippable.
    accepted_proposal_height: Option<u64>,
    quorum_echoes: Option<C::Hash>,
    quorum_votes: Option<bool>,
}

impl<C: Context> Default for RoundOutcome<C> {
    fn default() -> RoundOutcome<C> {
        RoundOutcome {
            accepted_proposal_height: None,
            quorum_echoes: None,
            quorum_votes: None,
        }
    }
}
