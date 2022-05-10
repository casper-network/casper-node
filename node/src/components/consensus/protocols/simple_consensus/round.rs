use std::{
    collections::{BTreeMap, HashMap},
    fmt::Debug,
};

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::{
    components::consensus::{
        protocols::simple_consensus::message::{Content, Proposal},
        traits::Context,
        utils::{ValidatorIndex, ValidatorMap},
    },
    utils::ds,
};

/// The protocol proceeds in rounds, for each of which we must
/// keep track of proposals, echos, votes, and the current outcome
/// of the round.
#[derive(Debug, DataSize)]
pub(crate) struct Round<C>
where
    C: Context,
{
    /// All of the proposals sent to us this round from the leader
    #[data_size(with = ds::hashmap_sample)]
    pub(super) proposals: HashMap<C::Hash, (Proposal<C>, C::Signature)>,
    /// The echos we've received for each proposal so far.
    #[data_size(with = ds::hashmap_sample)]
    pub(super) echos: HashMap<C::Hash, BTreeMap<ValidatorIndex, C::Signature>>,
    /// The votes we've received for this round so far.
    pub(super) votes: BTreeMap<bool, ValidatorMap<Option<C::Signature>>>,
    /// The memoized results in this round.
    pub(super) outcome: RoundOutcome<C>,
}

impl<C: Context> Round<C> {
    /// Creates a new [`Round`] with no proposals, echos, votes, and empty
    /// round outcome.
    pub(super) fn new(validator_count: usize) -> Round<C> {
        let mut votes = BTreeMap::new();
        votes.insert(false, vec![None; validator_count].into());
        votes.insert(true, vec![None; validator_count].into());
        Round {
            proposals: HashMap::new(),
            echos: HashMap::new(),
            votes,
            outcome: RoundOutcome::default(),
        }
    }

    /// Inserts a `Proposal` and returns `false` if we already had it.
    pub(super) fn insert_proposal(
        &mut self,
        proposal: Proposal<C>,
        signature: C::Signature,
    ) -> bool {
        let hash = proposal.hash();
        self.proposals.insert(hash, (proposal, signature)).is_none()
    }

    /// Inserts an `Echo`; returns `false` if we already had it.
    pub(super) fn insert_echo(
        &mut self,
        hash: C::Hash,
        validator_idx: ValidatorIndex,
        signature: C::Signature,
    ) -> bool {
        self.echos
            .entry(hash)
            .or_insert_with(BTreeMap::new)
            .insert(validator_idx, signature)
            .is_none()
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

    /// Returns the accepted proposal, if any, together with its height.
    pub(super) fn accepted_proposal(&self) -> Option<(u64, &Proposal<C>)> {
        let height = self.outcome.accepted_proposal_height?;
        let hash = self.outcome.quorum_echos?;
        let (proposal, _signature) = self.proposals.get(&hash)?;
        Some((height, proposal))
    }

    /// Check if the round has already received this message.
    pub(super) fn contains(&self, content: &Content<C>, validator_idx: ValidatorIndex) -> bool {
        match content {
            Content::Proposal(proposal) => self.proposals.contains_key(&proposal.hash()),
            Content::Echo(hash) => self
                .echos
                .get(hash)
                .map_or(false, |echo_map| echo_map.contains_key(&validator_idx)),
            Content::Vote(vote) => self.votes[vote][validator_idx].is_some(),
        }
    }
}

/// Indicates the outcome of a given round.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) struct RoundOutcome<C>
where
    C: Context,
{
    /// This is `Some(h)` if there is an accepted proposal with relative height `h`, i.e. there is
    /// a quorum of echos, `h` accepted ancestors, and all rounds since the parent's are skippable.
    pub(super) accepted_proposal_height: Option<u64>,
    pub(super) quorum_echos: Option<C::Hash>,
    pub(super) quorum_votes: Option<bool>,
}

impl<C: Context> Default for RoundOutcome<C> {
    fn default() -> RoundOutcome<C> {
        RoundOutcome {
            accepted_proposal_height: None,
            quorum_echos: None,
            quorum_votes: None,
        }
    }
}
