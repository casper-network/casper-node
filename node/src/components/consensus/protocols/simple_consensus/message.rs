use std::{collections::BTreeSet, fmt::Debug};

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_types::Timestamp;

use crate::components::consensus::{
    consensus_protocol::ProposedBlock, protocols::simple_consensus::RoundId, traits::Context,
    utils::ValidatorIndex,
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
    pub(super) timestamp: Timestamp,
    pub(super) maybe_block: Option<C::ConsensusValue>,
    pub(super) maybe_parent_round_id: Option<RoundId>,
    /// The set of validators that appear to be inactive in this era.
    /// This is `None` in round 0 and in dummy blocks.
    pub(super) inactive: Option<BTreeSet<ValidatorIndex>>,
}

impl<C: Context> Proposal<C> {
    pub(super) fn dummy(timestamp: Timestamp, parent_round_id: RoundId) -> Self {
        Proposal {
            timestamp,
            maybe_block: None,
            maybe_parent_round_id: Some(parent_round_id),
            inactive: None,
        }
    }

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

    pub(super) fn hash(&self) -> C::Hash {
        let serialized = bincode::serialize(&self).expect("failed to serialize fields");
        <C as Context>::hash(&serialized)
    }
}

/// The content of a message in the main protocol, as opposed to the
/// sync messages, which are somewhat decoupled from the rest of the
/// protocol. This message, along with the instance and round ID,
/// are what are signed by the active validators.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) enum Content<C: Context> {
    Proposal(Proposal<C>),
    Echo(C::Hash),
    Vote(bool),
}

impl<C: Context> Content<C> {
    pub(super) fn is_proposal(&self) -> bool {
        matches!(self, Content::Proposal(_))
    }
}

/// All messages of the protocol.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) enum Message<C: Context> {
    /// Partial information about the sender's protocol state. The receiver should send missing
    /// data.
    ///
    /// The sender chooses a random peer and a random era, and includes in its `SyncState` message
    /// information about received proposals, echoes and votes. The idea is to set the `i`-th bit
    /// in the `u128` fields to `1` if we have a signature from the `i`-th validator.
    ///
    /// To keep the size of these messages constant even if there are more than 128 validators, a
    /// random interval is selected and only information about validators in that interval is
    /// included: The bit with the lowest significance corresponds to validator number
    /// `first_validator_idx`, and the one with the highest to
    /// `(first_validator_idx + 127) % validator_count`.
    ///
    /// For example if there are 500 validators and `first_validator_idx` is 450, the `u128`'s bits
    /// refer to validators 450, 451, ..., 499, 0, 1, ..., 77.
    SyncState {
        /// The round the information refers to.
        round_id: RoundId,
        /// The proposal hash with the most echoes (by weight).
        proposal_hash: Option<C::Hash>,
        /// Whether the sender has the proposal with that hash.
        proposal: bool,
        /// The index of the first validator covered by the bit fields below.
        first_validator_idx: ValidatorIndex,
        /// A bit field with 1 for every validator the sender has an echo from.
        echoes: u128,
        /// A bit field with 1 for every validator the sender has a `true` vote from.
        true_votes: u128,
        /// A bit field with 1 for every validator the sender has a `false` vote from.
        false_votes: u128,
        /// A bit field with 1 for every validator the sender has evidence against.
        faulty: u128,
        instance_id: C::InstanceId,
    },
    Signed {
        round_id: RoundId,
        instance_id: C::InstanceId,
        content: Content<C>,
        validator_idx: ValidatorIndex,
        signature: C::Signature,
    },
}

impl<C: Context> Message<C> {
    pub(super) fn new_empty_round_sync_state(
        round_id: RoundId,
        first_validator_idx: ValidatorIndex,
        faulty: u128,
        instance_id: C::InstanceId,
    ) -> Self {
        Message::SyncState {
            round_id,
            proposal_hash: None,
            proposal: false,
            first_validator_idx,
            echoes: 0,
            true_votes: 0,
            false_votes: 0,
            faulty,
            instance_id,
        }
    }

    pub(super) fn serialize(&self) -> Vec<u8> {
        bincode::serialize(self).expect("should serialize message")
    }

    pub(super) fn instance_id(&self) -> &C::InstanceId {
        match self {
            Message::SyncState { instance_id, .. } | Message::Signed { instance_id, .. } => {
                instance_id
            }
        }
    }
}
