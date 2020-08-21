use std::{collections::BTreeMap, fmt::Debug};

use anyhow::Error;

use crate::{components::consensus::traits::ConsensusValueT, types::Timestamp};

mod protocol_state;
pub(crate) mod synchronizer;

pub(crate) use protocol_state::{ProtocolState, VertexTrait};

/// Information about the context in which a new block is created.
#[derive(Clone, Eq, PartialEq, Debug, Ord, PartialOrd)]
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
        opt_parent: Option<C>,
    },
    /// A block was finalized. The timestamp is from when the block was proposed.
    FinalizedBlock {
        value: C,
        new_equivocators: Vec<VID>,
        rewards: BTreeMap<VID, u64>,
        timestamp: Timestamp,
        height: u64,
        switch_block: bool,
    },
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
    /// Handles an incoming message (like NewVote, RequestDependency).
    fn handle_message(
        &mut self,
        sender: I,
        msg: Vec<u8>,
    ) -> Result<Vec<ConsensusProtocolResult<I, C, VID>>, Error>;

    /// Triggers consensus' timer.
    fn handle_timer(
        &mut self,
        timerstamp: Timestamp,
    ) -> Result<Vec<ConsensusProtocolResult<I, C, VID>>, Error>;

    /// Proposes a new value for consensus.
    fn propose(
        &mut self,
        value: C,
        block_context: BlockContext,
    ) -> Result<Vec<ConsensusProtocolResult<I, C, VID>>, Error>;

    /// Marks the `value` as valid or invalid, based on validation requested via
    /// `ConsensusProtocolResult::ValidateConsensusvalue`.
    fn resolve_validity(
        &mut self,
        value: &C,
        valid: bool,
    ) -> Result<Vec<ConsensusProtocolResult<I, C, VID>>, Error>;

    /// Turns this instance into a passive observer, that does not create any new vertices.
    fn deactivate_validator(&mut self);
}
