//! The consensus component. Provides distributed consensus among the nodes in the network.

#![warn(clippy::integer_arithmetic)]

mod cl_context;
mod config;
mod consensus_protocol;
mod era_supervisor;
#[macro_use]
mod highway_core;
pub(crate) mod error;
mod metrics;
mod protocols;
#[cfg(test)]
mod tests;
mod traits;
mod utils;
mod validator_change;

use std::{
    borrow::Cow,
    convert::Infallible,
    fmt::{self, Debug, Display, Formatter},
    sync::Arc,
    time::Duration,
};

use datasize::DataSize;
use derive_more::From;
use hex_fmt::HexFmt;
use serde::{Deserialize, Serialize};
use tracing::info;

use casper_types::{EraId, PublicKey};

use crate::{
    components::Component,
    effect::{
        announcements::{BlocklistAnnouncement, ConsensusAnnouncement},
        diagnostics_port::DumpConsensusStateRequest,
        incoming::ConsensusMessageIncoming,
        requests::{
            BlockProposerRequest, BlockValidationRequest, ChainspecLoaderRequest, ConsensusRequest,
            ContractRuntimeRequest, NetworkInfoRequest, NetworkRequest, StorageRequest,
        },
        EffectBuilder, EffectExt, Effects,
    },
    protocol::Message,
    reactor::ReactorEvent,
    types::{ActivationPoint, BlockHash, BlockHeader, BlockPayload, NodeId, Timestamp},
    NodeRng,
};

pub(crate) use cl_context::ClContext;
pub(crate) use config::{ChainspecConsensusExt, Config};
pub(crate) use consensus_protocol::{BlockContext, EraReport, ProposedBlock};
pub(crate) use era_supervisor::{debug::EraDump, EraSupervisor};
pub(crate) use protocols::highway::HighwayProtocol;

pub(crate) use utils::check_sufficient_finality_signatures;
pub(crate) use validator_change::ValidatorChange;

#[derive(DataSize, Clone, Serialize, Deserialize)]
pub(crate) enum ConsensusMessage {
    /// A protocol message, to be handled by the instance in the specified era.
    Protocol { era_id: EraId, payload: Vec<u8> },
    /// A request for evidence against the specified validator, from any era that is still bonded
    /// in `era_id`.
    EvidenceRequest { era_id: EraId, pub_key: PublicKey },
}

/// An ID to distinguish different timers. What they are used for is specific to each consensus
/// protocol implementation.
#[derive(DataSize, Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimerId(pub u8);

/// An ID to distinguish queued actions. What they are used for is specific to each consensus
/// protocol implementation.
#[derive(DataSize, Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActionId(pub u8);

#[derive(DataSize, Debug, From)]
pub struct NewBlockPayload {
    era_id: EraId,
    block_payload: Arc<BlockPayload>,
    block_context: BlockContext<ClContext>,
}

#[derive(DataSize, Debug, From)]
pub struct ResolveValidity {
    era_id: EraId,
    sender: NodeId,
    proposed_block: ProposedBlock<ClContext>,
    valid: bool,
}

/// Consensus component event.
#[derive(DataSize, Debug, From)]
pub(crate) enum Event {
    #[from]
    /// An incoming network message.
    Incoming(ConsensusMessageIncoming),
    /// A scheduled event to be handled by a specified era.
    Timer {
        era_id: EraId,
        timestamp: Timestamp,
        timer_id: TimerId,
    },
    /// A queued action to be handled by a specific era.
    Action { era_id: EraId, action_id: ActionId },
    /// We are receiving the data we require to propose a new block.
    NewBlockPayload(NewBlockPayload),
    #[from]
    ConsensusRequest(ConsensusRequest),
    /// A new block has been added to the linear chain.
    BlockAdded {
        header: Box<BlockHeader>,
        header_hash: BlockHash,
    },
    /// The proto-block has been validated.
    ResolveValidity(ResolveValidity),
    /// Deactivate the era with the given ID, unless the number of faulty validators increases.
    DeactivateEra {
        era_id: EraId,
        faulty_num: usize,
        delay: Duration,
    },
    /// Event raised when a new era should be created because a new switch block is available.
    CreateNewEra {
        /// The most recent switch block headers
        switch_blocks: Vec<BlockHeader>,
    },
    /// Got the result of checking for an upgrade activation point.
    GotUpgradeActivationPoint(ActivationPoint),
    /// Dump state for debugging purposes.
    #[from]
    DumpState(DumpConsensusStateRequest),
}

impl Debug for ConsensusMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ConsensusMessage::Protocol { era_id, payload: _ } => {
                write!(f, "Protocol {{ era_id: {:?}, .. }}", era_id)
            }
            ConsensusMessage::EvidenceRequest { era_id, pub_key } => f
                .debug_struct("EvidenceRequest")
                .field("era_id", era_id)
                .field("pub_key", pub_key)
                .finish(),
        }
    }
}

impl Display for ConsensusMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ConsensusMessage::Protocol { era_id, payload } => {
                write!(f, "protocol message {:10} in {}", HexFmt(payload), era_id)
            }
            ConsensusMessage::EvidenceRequest { era_id, pub_key } => write!(
                f,
                "request for evidence of fault by {} in {} or earlier",
                pub_key, era_id,
            ),
        }
    }
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Incoming(ConsensusMessageIncoming { sender, message }) => {
                write!(f, "msg from {:?}: {}", sender, message)
            }
            Event::Timer {
                era_id,
                timestamp,
                timer_id,
            } => write!(
                f,
                "timer (ID {}) for {} scheduled for timestamp {}",
                timer_id.0, era_id, timestamp,
            ),
            Event::Action { era_id, action_id } => {
                write!(f, "action (ID {}) for {}", action_id.0, era_id)
            }
            Event::NewBlockPayload(NewBlockPayload {
                era_id,
                block_payload,
                block_context,
            }) => write!(
                f,
                "New proto-block for era {:?}: {:?}, {:?}",
                era_id, block_payload, block_context
            ),
            Event::ConsensusRequest(request) => write!(
                f,
                "A request for consensus component hash been received: {:?}",
                request
            ),
            Event::BlockAdded {
                header: _,
                header_hash,
            } => write!(
                f,
                "A block has been added to the linear chain: {}",
                header_hash,
            ),
            Event::ResolveValidity(ResolveValidity {
                era_id,
                sender,
                proposed_block,
                valid,
            }) => write!(
                f,
                "Proposed block received from {:?} for {} is {}: {:?}",
                sender,
                era_id,
                if *valid { "valid" } else { "invalid" },
                proposed_block,
            ),
            Event::DeactivateEra {
                era_id, faulty_num, ..
            } => write!(
                f,
                "Deactivate old {} unless additional faults are observed; faults so far: {}",
                era_id, faulty_num
            ),
            Event::CreateNewEra { switch_blocks } => write!(
                f,
                "New era should be created; switch blocks: {:?}",
                switch_blocks
            ),
            Event::GotUpgradeActivationPoint(activation_point) => {
                write!(f, "new upgrade activation point: {:?}", activation_point)
            }
            Event::DumpState(req) => Display::fmt(req, f),
        }
    }
}

/// A helper trait whose bounds represent the requirements for a reactor event that `EraSupervisor`
/// can work with.
pub(crate) trait ReactorEventT:
    ReactorEvent
    + From<Event>
    + Send
    + From<NetworkRequest<Message>>
    + From<NetworkInfoRequest>
    + From<BlockProposerRequest>
    + From<ConsensusAnnouncement>
    + From<BlockValidationRequest>
    + From<StorageRequest>
    + From<ContractRuntimeRequest>
    + From<ChainspecLoaderRequest>
    + From<BlocklistAnnouncement>
    + From<BlocklistAnnouncement>
{
}

impl<REv> ReactorEventT for REv where
    REv: ReactorEvent
        + From<Event>
        + Send
        + From<NetworkRequest<Message>>
        + From<NetworkInfoRequest>
        + From<BlockProposerRequest>
        + From<ConsensusAnnouncement>
        + From<BlockValidationRequest>
        + From<StorageRequest>
        + From<ContractRuntimeRequest>
        + From<ChainspecLoaderRequest>
        + From<BlocklistAnnouncement>
{
}

impl<REv> Component<REv> for EraSupervisor
where
    REv: ReactorEventT,
{
    type Event = Event;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Timer {
                era_id,
                timestamp,
                timer_id,
            } => self.handle_timer(effect_builder, rng, era_id, timestamp, timer_id),
            Event::Action { era_id, action_id } => {
                self.handle_action(effect_builder, rng, era_id, action_id)
            }
            Event::Incoming(ConsensusMessageIncoming { sender, message }) => {
                self.handle_message(effect_builder, rng, sender, message)
            }
            Event::NewBlockPayload(new_block_payload) => {
                self.handle_new_block_payload(effect_builder, rng, new_block_payload)
            }
            Event::BlockAdded {
                header,
                header_hash: _,
            } => self.handle_block_added(effect_builder, *header),
            Event::ResolveValidity(resolve_validity) => {
                self.resolve_validity(effect_builder, rng, resolve_validity)
            }
            Event::DeactivateEra {
                era_id,
                faulty_num,
                delay,
            } => self.handle_deactivate_era(effect_builder, era_id, faulty_num, delay),
            Event::CreateNewEra { switch_blocks } => {
                self.create_new_era_effects(effect_builder, rng, &switch_blocks)
            }
            Event::GotUpgradeActivationPoint(activation_point) => {
                self.got_upgrade_activation_point(activation_point)
            }
            Event::ConsensusRequest(ConsensusRequest::Status(responder)) => self.status(responder),
            Event::ConsensusRequest(ConsensusRequest::ValidatorChanges(responder)) => {
                let validator_changes = self.get_validator_changes();
                responder.respond(validator_changes).ignore()
            }
            Event::DumpState(req @ DumpConsensusStateRequest { era_id, .. }) => {
                let requested_era = era_id.unwrap_or_else(|| self.current_era());

                // We emit some log message to get some performance information and give the
                // operator a chance to find out why their node is busy.
                info!(era_id=%requested_era.value(), was_latest=era_id.is_none(), "dumping era via diagnostics port");

                let era_dump_result = self
                    .open_eras()
                    .get(&requested_era)
                    .ok_or_else(|| {
                        Cow::Owned(format!(
                            "could not dump consensus, {} not found",
                            requested_era
                        ))
                    })
                    .and_then(|era| EraDump::dump_era(era, requested_era));

                match era_dump_result {
                    Ok(dump) => req.answer(Ok(&dump)).ignore(),
                    Err(err) => req.answer(Err(err)).ignore(),
                }
            }
        }
    }
}
