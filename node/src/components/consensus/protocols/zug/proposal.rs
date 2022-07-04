use std::{collections::BTreeSet, fmt::Debug};

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::Timestamp;

use crate::components::consensus::{
    consensus_protocol::ProposedBlock, highway_core::validators::ValidatorIndex,
    protocols::zug::RoundId, traits::Context,
};

/// A proposal in the consensus protocol.
#[derive(Clone, Hash, Serialize, Deserialize, Debug, PartialEq, Eq, DataSize)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) struct Proposal<C>
where
    C: Context,
{
    /// The timestamp when the proposal was created. If finalized, this will be the block's
    /// timestamp.
    pub(super) timestamp: Timestamp,
    /// The proposed block. This must be `None` after the switch block.
    pub(super) maybe_block: Option<C::ConsensusValue>,
    /// The parent round. This is `None` if the proposed block has no parent in this era.
    pub(super) maybe_parent_round_id: Option<RoundId>,
    /// The set of validators that appear to be inactive in this era.
    /// This is `None` in round 0 and in dummy blocks.
    pub(super) inactive: Option<BTreeSet<ValidatorIndex>>,
}

impl<C: Context> Proposal<C> {
    /// Creates a new proposal with no block. This must be used if an ancestor would be the
    /// switch block, since no blocks can come after the switch block.
    pub(super) fn dummy(timestamp: Timestamp, parent_round_id: RoundId) -> Self {
        Proposal {
            timestamp,
            maybe_block: None,
            maybe_parent_round_id: Some(parent_round_id),
            inactive: None,
        }
    }

    /// Creates a new proposal with the given block and parent round. If the parent round is none
    /// it is proposed as the first block in this era.
    pub(super) fn with_block(
        proposed_block: &ProposedBlock<C>,
        maybe_parent_round_id: Option<RoundId>,
        inactive: impl Iterator<Item = ValidatorIndex>,
    ) -> Self {
        Proposal {
            maybe_block: Some(proposed_block.value().clone()),
            timestamp: proposed_block.context().timestamp(),
            maybe_parent_round_id,
            inactive: maybe_parent_round_id.map(|_| inactive.collect()),
        }
    }

    /// Returns the proposal hash. Memoizes the hash, so it is only computed the first time this is called.
    #[cfg(test)] // Only used in tests; in production use HashedProposal below.
    pub(super) fn hash(&self) -> C::Hash {
        let serialized = bincode::serialize(&self).expect("failed to serialize fields");
        <C as Context>::hash(&serialized)
    }
}

/// A proposal with its memoized hash.
#[derive(Clone, Hash, Debug, PartialEq, Eq, DataSize)]
pub(crate) struct HashedProposal<C>
where
    C: Context,
{
    hash: C::Hash,
    proposal: Proposal<C>,
}

impl<C: Context> HashedProposal<C> {
    pub(crate) fn new(proposal: Proposal<C>) -> Self {
        let serialized = bincode::serialize(&proposal).expect("failed to serialize fields");
        let hash = <C as Context>::hash(&serialized);
        HashedProposal { hash, proposal }
    }

    pub(crate) fn hash(&self) -> &C::Hash {
        &self.hash
    }

    pub(crate) fn inner(&self) -> &Proposal<C> {
        &self.proposal
    }

    pub(crate) fn into_inner(self) -> Proposal<C> {
        self.proposal
    }

    pub(crate) fn maybe_block(&self) -> Option<&C::ConsensusValue> {
        self.proposal.maybe_block.as_ref()
    }

    pub(crate) fn timestamp(&self) -> Timestamp {
        self.proposal.timestamp
    }

    pub(crate) fn inactive(&self) -> Option<&BTreeSet<ValidatorIndex>> {
        self.proposal.inactive.as_ref()
    }

    pub(crate) fn maybe_parent_round_id(&self) -> Option<RoundId> {
        self.proposal.maybe_parent_round_id
    }
}
