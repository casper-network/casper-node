use std::{any::Any, collections::BTreeMap, fmt::Debug, path::PathBuf};

use anyhow::Error;
use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::{
    components::consensus::{traits::Context, ActionId, TimerId},
    types::Timestamp,
    NodeRng,
};

/// Information about the context in which a new block is created.
#[derive(Clone, DataSize, Eq, PartialEq, Debug, Ord, PartialOrd, Hash)]
pub struct BlockContext {
    timestamp: Timestamp,
    height: u64,
}

impl BlockContext {
    /// Constructs a new `BlockContext`.
    pub(crate) fn new(timestamp: Timestamp, height: u64) -> Self {
        BlockContext { timestamp, height }
    }

    /// The block's timestamp.
    pub(crate) fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    #[cfg(test)]
    pub(crate) fn height(&self) -> u64 {
        self.height
    }
}

/// Equivocation and reward information to be included in the terminal finalized block.
#[derive(Clone, DataSize, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(bound(
    serialize = "VID: Ord + Serialize",
    deserialize = "VID: Ord + Deserialize<'de>",
))]
pub struct EraEnd<VID> {
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
    pub(crate) height: u64,
    /// The validators known to be faulty as seen by this block.
    pub(crate) equivocators: Vec<C::ValidatorId>,
    /// If this is a terminal block, i.e. the last one to be finalized, this contains additional
    /// data like rewards and inactive validators.
    pub(crate) terminal_block_data: Option<TerminalBlockData<C>>,
    /// Proposer of this value
    pub(crate) proposer: C::ValidatorId,
}

// TODO: get rid of anyhow::Error; use variant and derive Clone and PartialEq. This is for testing.
#[derive(Debug)]
pub(crate) enum ProtocolOutcome<I, C: Context> {
    CreatedGossipMessage(Vec<u8>),
    CreatedTargetedMessage(Vec<u8>, I),
    InvalidIncomingMessage(Vec<u8>, I, Error),
    ScheduleTimer(Timestamp, TimerId),
    QueueAction(ActionId),
    /// Request deploys for a new block, providing the necessary context.
    CreateNewBlock {
        block_context: BlockContext,
        past_values: Vec<C::ConsensusValue>,
    },
    /// A block was finalized.
    FinalizedBlock(FinalizedBlock<C>),
    /// Request validation of the consensus value, contained in a message received from the given
    /// node.
    ///
    /// The domain logic should verify any intrinsic validity conditions of consensus values, e.g.
    /// that it has the expected structure, or that deploys that are mentioned by hash actually
    /// exist, and then call `ConsensusProtocol::resolve_validity`.
    ValidateConsensusValue(I, C::ConsensusValue, Timestamp),
    /// New direct evidence was added against the given validator.
    NewEvidence(C::ValidatorId),
    /// Send evidence about the validator from an earlier era to the peer.
    SendEvidence(I, C::ValidatorId),
    /// We've detected an equivocation our own node has made.
    WeAreFaulty,
    /// We've received a unit from a doppelganger.
    DoppelgangerDetected,
    /// We want to disconnect from a sender of invalid data.
    Disconnect(I),
}

/// An API for a single instance of the consensus.
pub(crate) trait ConsensusProtocol<I, C: Context>: Send {
    /// Upcasts consensus protocol into `dyn Any`.
    ///
    /// Typically called on a boxed trait object for downcasting afterwards.
    fn as_any(&self) -> &dyn Any;

    /// Handles an incoming message (like NewUnit, RequestDependency).
    fn handle_message(
        &mut self,
        sender: I,
        msg: Vec<u8>,
        evidence_only: bool,
        rng: &mut NodeRng,
    ) -> Vec<ProtocolOutcome<I, C>>;

    /// Handles new connection to a peer.
    fn handle_new_peer(&mut self, peer_id: I) -> Vec<ProtocolOutcome<I, C>>;

    /// Triggers consensus' timer.
    fn handle_timer(
        &mut self,
        timestamp: Timestamp,
        timer_id: TimerId,
        rng: &mut NodeRng,
    ) -> Vec<ProtocolOutcome<I, C>>;

    /// Triggers a queued action.
    fn handle_action(
        &mut self,
        action_id: ActionId,
        rng: &mut NodeRng,
    ) -> Vec<ProtocolOutcome<I, C>>;

    /// Proposes a new value for consensus.
    fn propose(
        &mut self,
        value: C::ConsensusValue,
        block_context: BlockContext,
        rng: &mut NodeRng,
    ) -> Vec<ProtocolOutcome<I, C>>;

    /// Marks the `value` as valid or invalid, based on validation requested via
    /// `ProtocolOutcome::ValidateConsensusvalue`.
    fn resolve_validity(
        &mut self,
        value: &C::ConsensusValue,
        valid: bool,
        rng: &mut NodeRng,
    ) -> Vec<ProtocolOutcome<I, C>>;

    /// Turns this instance into an active validator, that participates in the consensus protocol.
    fn activate_validator(
        &mut self,
        our_id: C::ValidatorId,
        secret: C::ValidatorSecret,
        timestamp: Timestamp,
        unit_hash_file: Option<PathBuf>,
    ) -> Vec<ProtocolOutcome<I, C>>;

    /// Turns this instance into a passive observer, that does not create any new vertices.
    fn deactivate_validator(&mut self);

    /// Returns whether the validator `vid` is known to be faulty.
    fn has_evidence(&self, vid: &C::ValidatorId) -> bool;

    /// Marks the validator `vid` as faulty, based on evidence from a different instance.
    fn mark_faulty(&mut self, vid: &C::ValidatorId);

    /// Sends evidence for a faulty of validator `vid` to the `sender` of the request.
    fn request_evidence(&self, sender: I, vid: &C::ValidatorId) -> Vec<ProtocolOutcome<I, C>>;

    /// Returns the list of all validators that were observed as faulty in this consensus instance.
    fn validators_with_evidence(&self) -> Vec<&C::ValidatorId>;

    /// Returns true if the protocol has received some messages since initialization.
    fn has_received_messages(&self) -> bool;

    /// Returns whether this instance of a protocol is an active validator.
    fn is_active(&self) -> bool;

    /// Returns the instance ID of this instance.
    fn instance_id(&self) -> &C::InstanceId;

    /// Returns the protocol outcomes for all the required timers.
    fn recreate_timers(&self) -> Vec<ProtocolOutcome<I, C>>;
}
