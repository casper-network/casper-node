use std::{
    any::Any,
    collections::BTreeMap,
    fmt::{self, Debug, Display, Formatter},
    path::PathBuf,
};

use anyhow::Error;
use datasize::DataSize;
use serde::{Deserialize, Serialize};

use casper_hashing::Digest;
use casper_types::{bytesrepr::ToBytes, TimeDiff, Timestamp};

use crate::{
    components::consensus::{traits::Context, ActionId, TimerId},
    types::NodeId,
    NodeRng,
};

/// Information about the context in which a new block is created.
#[derive(Clone, DataSize, Eq, PartialEq, Debug, Ord, PartialOrd, Hash)]
pub struct BlockContext<C>
where
    C: Context,
{
    timestamp: Timestamp,
    ancestor_values: Vec<C::ConsensusValue>,
}

impl<C: Context> BlockContext<C> {
    /// Constructs a new `BlockContext`.
    pub(crate) fn new(timestamp: Timestamp, ancestor_values: Vec<C::ConsensusValue>) -> Self {
        BlockContext {
            timestamp,
            ancestor_values,
        }
    }

    /// The block's timestamp.
    pub(crate) fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// The block's relative height, i.e. the number of ancestors in the current era.
    pub(crate) fn height(&self) -> u64 {
        self.ancestor_values.len() as u64
    }

    /// The values of the block's ancestors.
    pub(crate) fn ancestor_values(&self) -> &[C::ConsensusValue] {
        &self.ancestor_values
    }
}

/// A proposed block, with context.
#[derive(Clone, DataSize, Eq, PartialEq, Debug, Ord, PartialOrd, Hash)]
pub struct ProposedBlock<C>
where
    C: Context,
{
    value: C::ConsensusValue,
    context: BlockContext<C>,
}

impl<C: Context> ProposedBlock<C> {
    pub(crate) fn new(value: C::ConsensusValue, context: BlockContext<C>) -> Self {
        ProposedBlock { value, context }
    }

    pub(crate) fn value(&self) -> &C::ConsensusValue {
        &self.value
    }

    pub(crate) fn context(&self) -> &BlockContext<C> {
        &self.context
    }

    pub(crate) fn destructure(self) -> (C::ConsensusValue, BlockContext<C>) {
        (self.value, self.context)
    }
}

impl<C: Context> Display for ProposedBlock<C>
where
    C::ConsensusValue: Display,
{
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "proposed block at {}: {}",
            self.context.timestamp(),
            self.value
        )
    }
}

/// Equivocation and reward information to be included in the terminal finalized block.
#[derive(Clone, DataSize, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(bound(
    serialize = "VID: Ord + Serialize",
    deserialize = "VID: Ord + Deserialize<'de>",
))]
pub struct EraReport<VID> {
    /// The set of equivocators.
    pub(crate) equivocators: Vec<VID>,
    /// Rewards for finalization of earlier blocks.
    ///
    /// This is a measure of the value of each validator's contribution to consensus, in
    /// fractions of the configured maximum block reward.
    pub(crate) rewards: BTreeMap<VID, u64>,
    /// Validators that haven't produced any unit during the era.
    pub(crate) inactive_validators: Vec<VID>,
}

impl<VID> Default for EraReport<VID>
where
    VID: Ord,
{
    fn default() -> Self {
        EraReport {
            equivocators: vec![],
            rewards: BTreeMap::new(),
            inactive_validators: vec![],
        }
    }
}

impl<VID> EraReport<VID> {
    pub fn hash(&self) -> Digest
    where
        VID: ToBytes,
    {
        // Helper function to hash slice of validators
        fn hash_slice_of_validators<VID>(slice_of_validators: &[VID]) -> Digest
        where
            VID: ToBytes,
        {
            Digest::hash_merkle_tree(slice_of_validators.iter().map(|validator| {
                Digest::hash(validator.to_bytes().expect("Could not serialize validator"))
            }))
        }

        // Pattern match here leverages compiler to ensure every field is accounted for
        let EraReport {
            equivocators,
            inactive_validators,
            rewards,
        } = self;

        let hashed_equivocators = hash_slice_of_validators(equivocators);
        let hashed_inactive_validators = hash_slice_of_validators(inactive_validators);
        let hashed_rewards = Digest::hash_btree_map(rewards).expect("Could not hash rewards");

        Digest::hash_slice_rfold(&[
            hashed_equivocators,
            hashed_rewards,
            hashed_inactive_validators,
        ])
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TerminalBlockData<C: Context> {
    /// The rewards for participating in consensus.
    pub(crate) rewards: BTreeMap<C::ValidatorId, u64>,
    /// The list of validators that haven't produced any units.
    pub(crate) inactive_validators: Vec<C::ValidatorId>,
}

/// A finalized block. All nodes are guaranteed to see the same sequence of blocks, and to agree
/// about all the information contained in this type, as long as the total weight of faulty
/// validators remains below the threshold.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FinalizedBlock<C: Context> {
    /// The finalized value.
    pub(crate) value: C::ConsensusValue,
    /// The timestamp at which this value was proposed.
    pub(crate) timestamp: Timestamp,
    /// The relative height in this instance of the protocol.
    pub(crate) relative_height: u64,
    /// The validators known to be faulty as seen by this block.
    pub(crate) equivocators: Vec<C::ValidatorId>,
    /// If this is a terminal block, i.e. the last one to be finalized, this contains additional
    /// data like rewards and inactive validators.
    pub(crate) terminal_block_data: Option<TerminalBlockData<C>>,
    /// Proposer of this value
    pub(crate) proposer: C::ValidatorId,
}

pub(crate) type ProtocolOutcomes<C> = Vec<ProtocolOutcome<C>>;

// TODO: get rid of anyhow::Error; use variant and derive Clone and PartialEq. This is for testing.
#[derive(Debug)]
pub(crate) enum ProtocolOutcome<C: Context> {
    CreatedGossipMessage(Vec<u8>),
    CreatedTargetedMessage(Vec<u8>, NodeId),
    CreatedMessageToRandomPeer(Vec<u8>),
    InvalidIncomingMessage(Vec<u8>, NodeId, Error),
    ScheduleTimer(Timestamp, TimerId),
    QueueAction(ActionId),
    /// Request deploys for a new block, providing the necessary context.
    CreateNewBlock(BlockContext<C>),
    /// A block was finalized.
    FinalizedBlock(FinalizedBlock<C>),
    /// Request validation of the consensus value, contained in a message received from the given
    /// node.
    ///
    /// The domain logic should verify any intrinsic validity conditions of consensus values, e.g.
    /// that it has the expected structure, or that deploys that are mentioned by hash actually
    /// exist, and then call `ConsensusProtocol::resolve_validity`.
    ValidateConsensusValue {
        sender: NodeId,
        proposed_block: ProposedBlock<C>,
    },
    /// New direct evidence was added against the given validator.
    NewEvidence(C::ValidatorId),
    /// Send evidence about the validator from an earlier era to the peer.
    SendEvidence(NodeId, C::ValidatorId),
    /// We've detected an equivocation our own node has made.
    WeAreFaulty,
    /// We've received a unit from a doppelganger.
    DoppelgangerDetected,
    /// Too many faulty validators. The protocol's fault tolerance threshold has been exceeded and
    /// consensus cannot continue.
    FttExceeded,
    /// No progress has been made recently.
    StandstillAlert,
    /// We want to disconnect from a sender of invalid data.
    Disconnect(NodeId),
}

/// An API for a single instance of the consensus.
pub(crate) trait ConsensusProtocol<C: Context>: Send {
    /// Upcasts consensus protocol into `dyn Any`.
    ///
    /// Typically called on a boxed trait object for downcasting afterwards.
    fn as_any(&self) -> &dyn Any;

    /// Handles an incoming message (like NewUnit, RequestDependency).
    fn handle_message(
        &mut self,
        rng: &mut NodeRng,
        sender: NodeId,
        msg: Vec<u8>,
        now: Timestamp,
    ) -> ProtocolOutcomes<C>;

    /// Current instance of consensus protocol is latest era.
    fn handle_is_current(&self, now: Timestamp) -> ProtocolOutcomes<C>;

    /// Triggers consensus' timer.
    fn handle_timer(&mut self, timestamp: Timestamp, timer_id: TimerId) -> ProtocolOutcomes<C>;

    /// Triggers a queued action.
    fn handle_action(&mut self, action_id: ActionId, now: Timestamp) -> ProtocolOutcomes<C>;

    /// Proposes a new value for consensus.
    fn propose(&mut self, proposed_block: ProposedBlock<C>, now: Timestamp) -> ProtocolOutcomes<C>;

    /// Marks the `value` as valid or invalid, based on validation requested via
    /// `ProtocolOutcome::ValidateConsensusvalue`.
    fn resolve_validity(
        &mut self,
        proposed_block: ProposedBlock<C>,
        valid: bool,
        now: Timestamp,
    ) -> ProtocolOutcomes<C>;

    /// Turns this instance into an active validator, that participates in the consensus protocol.
    fn activate_validator(
        &mut self,
        our_id: C::ValidatorId,
        secret: C::ValidatorSecret,
        timestamp: Timestamp,
        unit_hash_file: Option<PathBuf>,
    ) -> ProtocolOutcomes<C>;

    /// Turns this instance into a passive observer, that does not create any new vertices.
    fn deactivate_validator(&mut self);

    /// Clears this instance and keeps only the information necessary to validate evidence.
    fn set_evidence_only(&mut self);

    /// Returns whether the validator `vid` is known to be faulty.
    fn has_evidence(&self, vid: &C::ValidatorId) -> bool;

    /// Marks the validator `vid` as faulty, based on evidence from a different instance.
    fn mark_faulty(&mut self, vid: &C::ValidatorId);

    /// Sends evidence for a faulty of validator `vid` to the `sender` of the request.
    fn request_evidence(&self, sender: NodeId, vid: &C::ValidatorId) -> ProtocolOutcomes<C>;

    /// Sets the pause status: While paused we don't create consensus messages other than pings.
    fn set_paused(&mut self, paused: bool);

    /// Returns the list of all validators that were observed as faulty in this consensus instance.
    fn validators_with_evidence(&self) -> Vec<&C::ValidatorId>;

    /// Returns true if the protocol has received some messages since initialization.
    fn has_received_messages(&self) -> bool;

    /// Returns whether this instance of a protocol is an active validator.
    fn is_active(&self) -> bool;

    /// Returns the instance ID of this instance.
    fn instance_id(&self) -> &C::InstanceId;

    // TODO: Make this less Highway-specific.
    fn next_round_length(&self) -> Option<TimeDiff>;
}
