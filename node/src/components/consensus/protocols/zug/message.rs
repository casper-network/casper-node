use std::{collections::BTreeMap, fmt::Debug};

use datasize::DataSize;
use serde::{Deserialize, Serialize};

use either::Either;

use crate::{
    components::consensus::{
    	highway_core::validators::ValidatorIndex,
        protocols::zug::{Proposal, RoundId},
        traits::{Context, ValidatorSecret},
    },
    utils::ds,
};

/// The content of a message in the main protocol, as opposed to the proposal, and to sync messages,
/// which are somewhat decoupled from the rest of the protocol. These messages, along with the
/// instance and round ID, are signed by the active validators.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, Hash, DataSize)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) enum Content<C>
where
    C: Context,
{
    /// By signing the echo of a proposal hash a validator affirms that this is the first (and
    /// usually only) proposal by the round leader that they have received. A quorum of echoes is a
    /// requirement for a proposal to become accepted.
    Echo(C::Hash),
    /// By signing a `true` vote a validator confirms that they have accepted a proposal in this
    /// round before the timeout. If there is a quorum of `true` votes, the proposal becomes
    /// finalized, together with its ancestors.
    ///
    /// A `false` vote means they timed out waiting for a proposal to get accepted. A quorum of
    /// `false` votes allows the next round's leader to make a proposal without waiting for this
    /// round's.
    Vote(bool),
}

impl<C: Context> Content<C> {
    /// Returns whether the two contents contradict each other. A correct validator is expected to
    /// never sign two contradictory contents in the same round.
    pub(crate) fn contradicts(&self, other: &Content<C>) -> bool {
        match (self, other) {
            (Content::Vote(vote0), Content::Vote(vote1)) => vote0 != vote1,
            (Content::Echo(hash0), Content::Echo(hash1)) => hash0 != hash1,
            _ => false,
        }
    }
}

/// A vote or echo with a signature.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, DataSize)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) struct SignedMessage<C>
where
    C: Context,
{
    pub(super) round_id: RoundId,
    pub(super) instance_id: C::InstanceId,
    pub(super) content: Content<C>,
    pub(super) validator_idx: ValidatorIndex,
    pub(super) signature: C::Signature,
}

impl<C: Context> SignedMessage<C> {
    /// Creates a new signed message with a valid signature.
    pub(crate) fn sign_new(
        round_id: RoundId,
        instance_id: C::InstanceId,
        content: Content<C>,
        validator_idx: ValidatorIndex,
        secret: &C::ValidatorSecret,
    ) -> SignedMessage<C> {
        let hash = Self::hash_fields(round_id, &instance_id, &content, validator_idx);
        SignedMessage {
            round_id,
            instance_id,
            content,
            validator_idx,
            signature: secret.sign(&hash),
        }
    }

    /// Creates a new signed message with the alternative content and signature.
    pub(crate) fn with(&self, content: Content<C>, signature: C::Signature) -> SignedMessage<C> {
        SignedMessage {
            content,
            signature,
            ..*self
        }
    }

    /// Returns whether the signature is valid.
    pub(crate) fn verify_signature(&self, validator_id: &C::ValidatorId) -> bool {
        let hash = Self::hash_fields(
            self.round_id,
            &self.instance_id,
            &self.content,
            self.validator_idx,
        );
        C::verify_signature(&hash, validator_id, &self.signature)
    }

    /// Returns the hash of all fields except the signature.
    fn hash_fields(
        round_id: RoundId,
        instance_id: &C::InstanceId,
        content: &Content<C>,
        validator_idx: ValidatorIndex,
    ) -> C::Hash {
        let serialized_fields =
            bincode::serialize(&(round_id, instance_id, content, validator_idx))
                .expect("failed to serialize fields");
        <C as Context>::hash(&serialized_fields)
    }
}

/// Partial information about the sender's protocol state. The receiver should send missing data.
///
/// The sender chooses a random peer and a random era, and includes in its `SyncRequest` message
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
#[derive(DataSize, Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) struct SyncRequest<C>
where
    C: Context,
{
    /// The round the information refers to.
    pub(crate) round_id: RoundId,
    /// The proposal hash with the most echoes (by weight).
    pub(crate) proposal_hash: Option<C::Hash>,
    /// Whether the sender has the proposal with that hash.
    pub(crate) has_proposal: bool,
    /// The index of the first validator covered by the bit fields below.
    pub(crate) first_validator_idx: ValidatorIndex,
    /// A bit field with 1 for every validator the sender has an echo from.
    pub(crate) echoes: u128,
    /// A bit field with 1 for every validator the sender has a `true` vote from.
    pub(crate) true_votes: u128,
    /// A bit field with 1 for every validator the sender has a `false` vote from.
    pub(crate) false_votes: u128,
    /// A bit field with 1 for every validator the sender has any signed message from.
    pub(crate) active: u128,
    /// A bit field with 1 for every validator the sender has evidence against.
    pub(crate) faulty: u128,
    pub(crate) instance_id: C::InstanceId,
}

impl<C: Context> SyncRequest<C> {
    /// Creates a `SyncRequest` for a round in which we haven't received any messages yet.
    pub(super) fn new_empty_round(
        round_id: RoundId,
        first_validator_idx: ValidatorIndex,
        faulty: u128,
        active: u128,
        instance_id: C::InstanceId,
    ) -> Self {
        SyncRequest {
            round_id,
            proposal_hash: None,
            has_proposal: false,
            first_validator_idx,
            echoes: 0,
            true_votes: 0,
            false_votes: 0,
            active,
            faulty,
            instance_id,
        }
    }
}

/// The response to a `SyncRequest`, containing proposals, signatures and evidence the requester is
/// missing.
#[derive(DataSize, Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) struct SyncResponse<C>
where
    C: Context,
{
    /// The round the information refers to.
    pub(crate) round_id: RoundId,
    /// The proposal in this round, or its hash.
    #[data_size(with = ds::maybe_either)]
    pub(crate) proposal_or_hash: Option<Either<Proposal<C>, C::Hash>>,
    /// Echo signatures the requester is missing.
    pub(crate) echo_sigs: BTreeMap<ValidatorIndex, C::Signature>,
    /// Vote signatures for `true` the requester is missing.
    pub(crate) true_vote_sigs: BTreeMap<ValidatorIndex, C::Signature>,
    /// Vote signatures for `false` the requester is missing.
    pub(crate) false_vote_sigs: BTreeMap<ValidatorIndex, C::Signature>,
    /// Signed messages that prove that a validator was active.
    pub(crate) signed_messages: Vec<SignedMessage<C>>,
    /// Evidence against faulty validators.
    pub(crate) evidence: Vec<(SignedMessage<C>, Content<C>, C::Signature)>,
    pub(crate) instance_id: C::InstanceId,
}

/// All messages of the protocol.
#[derive(DataSize, Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) enum Message<C>
where
    C: Context,
{
    /// Signatures, proposals and evidence the requester was missing.
    SyncResponse(SyncResponse<C>),
    /// A proposal for a new block. This does not contain any signature; instead, the proposer is
    /// expected to sign an echo with the proposal hash. Validators will drop any proposal they
    /// receive unless they either have a signed echo by the proposer and the proposer has not
    /// double-signed, or they have a quorum of echoes.
    Proposal {
        round_id: RoundId,
        instance_id: C::InstanceId,
        proposal: Proposal<C>,
    },
    /// An echo or vote signed by an active validator.
    Signed(SignedMessage<C>),
    /// Two conflicting signatures by the same validator.
    Evidence(SignedMessage<C>, Content<C>, C::Signature),
}

impl<C: Context> Message<C> {
    pub(super) fn instance_id(&self) -> &C::InstanceId {
        match self {
            Message::SyncResponse(SyncResponse { instance_id, .. })
            | Message::Signed(SignedMessage { instance_id, .. })
            | Message::Proposal { instance_id, .. }
            | Message::Evidence(SignedMessage { instance_id, .. }, ..) => instance_id,
        }
    }
}
