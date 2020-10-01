use std::{any::Any, collections::BTreeMap, fmt::Debug};

use anyhow::Error;
use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::{
    components::consensus::traits::ConsensusValueT,
    types::{CryptoRngCore, Timestamp},
};

/// Information about the context in which a new block is created.
#[derive(Clone, DataSize, Eq, PartialEq, Debug, Ord, PartialOrd)]
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

    /// The block's relative height within the current era.
    // TODO - remove once used
    #[allow(dead_code)]
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
}

/// A finalized block. All nodes are guaranteed to see the same sequence of blocks, and to agree
/// about all the information contained in this type, as long as the total weight of faulty
/// validators remains below the threshold.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct FinalizedBlock<C: ConsensusValueT, VID> {
    /// The finalized value.
    pub(crate) value: C,
    /// The timestamp at which this value was proposed.
    pub(crate) timestamp: Timestamp,
    /// The relative height in this instance of the protocol.
    pub(crate) height: u64,
    /// If this is a terminal block, i.e. the last one to be finalized, this includes reward and
    /// equivocator information.
    pub(crate) era_end: Option<EraEnd<VID>>,
    /// Proposer of this value
    pub(crate) proposer: VID,
}

#[derive(Debug)]
pub(crate) enum ConsensusProtocolResult<I, C: ConsensusValueT, VID> {
    CreatedGossipMessage(Vec<u8>),
    CreatedTargetedMessage(Vec<u8>, I),
    InvalidIncomingMessage(Vec<u8>, I, Error),
    ScheduleTimer(Timestamp),
    /// Request deploys for a new block, whose timestamp will be the given `u64`.
    /// TODO: Add more details that are necessary for block creation.
    CreateNewBlock {
        block_context: BlockContext,
    },
    /// A block was finalized.
    FinalizedBlock(FinalizedBlock<C, VID>),
    /// Request validation of the consensus value, contained in a message received from the given
    /// node.
    ///
    /// The domain logic should verify any intrinsic validity conditions of consensus values, e.g.
    /// that it has the expected structure, or that deploys that are mentioned by hash actually
    /// exist, and then call `ConsensusProtocol::resolve_validity`.
    ValidateConsensusValue(I, C),
}

/// An API for a single instance of the consensus.
pub(crate) trait ConsensusProtocol<I, C: ConsensusValueT, VID> {
    /// Upcasts consensus protocol into `dyn Any`.
    ///
    /// Typically called on a boxed trait object for downcasting afterwards.
    fn as_any(&self) -> &dyn Any;

    /// Handles an incoming message (like NewVote, RequestDependency).
    fn handle_message(
        &mut self,
        sender: I,
        msg: Vec<u8>,
        rng: &mut dyn CryptoRngCore,
    ) -> Result<Vec<ConsensusProtocolResult<I, C, VID>>, Error>;

    /// Triggers consensus' timer.
    fn handle_timer(
        &mut self,
        timerstamp: Timestamp,
        rng: &mut dyn CryptoRngCore,
    ) -> Result<Vec<ConsensusProtocolResult<I, C, VID>>, Error>;

    /// Proposes a new value for consensus.
    fn propose(
        &mut self,
        value: C,
        block_context: BlockContext,
        rng: &mut dyn CryptoRngCore,
    ) -> Result<Vec<ConsensusProtocolResult<I, C, VID>>, Error>;

    /// Marks the `value` as valid or invalid, based on validation requested via
    /// `ConsensusProtocolResult::ValidateConsensusvalue`.
    fn resolve_validity(
        &mut self,
        value: &C,
        valid: bool,
        rng: &mut dyn CryptoRngCore,
    ) -> Result<Vec<ConsensusProtocolResult<I, C, VID>>, Error>;

    /// Turns this instance into a passive observer, that does not create any new vertices.
    fn deactivate_validator(&mut self);
}
