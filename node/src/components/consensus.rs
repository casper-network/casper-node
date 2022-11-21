//! The consensus component. Provides distributed consensus among the nodes in the network.

#![warn(clippy::integer_arithmetic)]

mod cl_context;
mod config;
mod consensus_protocol;
mod era_supervisor;
#[macro_use]
mod highway_core;
pub(crate) mod error;
mod leader_sequence;
mod metrics;
mod protocols;
#[cfg(test)]
pub(crate) mod tests;
mod traits;
mod validator_change;

use std::{
    borrow::Cow,
    fmt::{self, Debug, Display, Formatter},
    sync::Arc,
    time::Duration,
};

use datasize::DataSize;
use derive_more::From;
use serde::{Deserialize, Serialize};
use tracing::{info, trace};

use casper_types::{EraId, PublicKey, Timestamp};

use crate::{
    components::Component,
    effect::{
        announcements::{ConsensusAnnouncement, FatalAnnouncement, PeerBehaviorAnnouncement},
        diagnostics_port::DumpConsensusStateRequest,
        incoming::{ConsensusDemand, ConsensusMessageIncoming},
        requests::{
            BlockValidationRequest, ChainspecRawBytesRequest, ConsensusRequest,
            ContractRuntimeRequest, DeployBufferRequest, NetworkInfoRequest, NetworkRequest,
            StorageRequest,
        },
        EffectBuilder, EffectExt, Effects,
    },
    protocol::Message,
    reactor::ReactorEvent,
    types::{BlockHash, BlockHeader, BlockPayload, NodeId},
    NodeRng,
};
use protocols::{highway::HighwayProtocol, zug::Zug};
use traits::Context;

pub(crate) use cl_context::ClContext;
pub(crate) use config::{ChainspecConsensusExt, Config};
pub(crate) use consensus_protocol::{BlockContext, EraReport, ProposedBlock};
pub(crate) use era_supervisor::{debug::EraDump, EraSupervisor};
pub(crate) use leader_sequence::LeaderSequence;
pub(crate) use validator_change::ValidatorChange;

/// A message to be handled by the consensus protocol instance in a particular era.
#[derive(DataSize, Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum EraMessage<C>
where
    C: Context,
{
    Zug(Box<protocols::zug::Message<C>>),
    Highway(Box<protocols::highway::HighwayMessage<C>>),
}

impl<C: Context> EraMessage<C> {
    /// Returns the message for the Zug protocol, or an error if it is for a different protocol.
    fn try_into_zug(self) -> Result<protocols::zug::Message<C>, Self> {
        match self {
            EraMessage::Zug(msg) => Ok(*msg),
            other => Err(other),
        }
    }

    /// Returns the message for the Highway protocol, or an error if it is for a different
    /// protocol.
    fn try_into_highway(self) -> Result<protocols::highway::HighwayMessage<C>, Self> {
        match self {
            EraMessage::Highway(msg) => Ok(*msg),
            other => Err(other),
        }
    }
}

impl<C: Context> From<protocols::zug::Message<C>> for EraMessage<C> {
    fn from(msg: protocols::zug::Message<C>) -> EraMessage<C> {
        EraMessage::Zug(Box::new(msg))
    }
}

impl<C: Context> From<protocols::highway::HighwayMessage<C>> for EraMessage<C> {
    fn from(msg: protocols::highway::HighwayMessage<C>) -> EraMessage<C> {
        EraMessage::Highway(Box::new(msg))
    }
}

/// A request to be handled by the consensus protocol instance in a particular era.
#[derive(DataSize, Debug, Clone, Serialize, Deserialize, PartialEq, Eq, From)]
pub(crate) enum EraRequest<C>
where
    C: Context,
{
    Zug(protocols::zug::SyncRequest<C>),
}

impl<C: Context> EraRequest<C> {
    /// Returns the request for the Zug protocol, or an error if it is for a different protocol.
    fn try_into_zug(self) -> Result<protocols::zug::SyncRequest<C>, Self> {
        match self {
            EraRequest::Zug(msg) => Ok(msg),
        }
    }
}

#[derive(DataSize, Clone, Serialize, Deserialize)]
pub(crate) enum ConsensusMessage {
    /// A protocol message, to be handled by the instance in the specified era.
    Protocol {
        era_id: EraId,
        payload: EraMessage<ClContext>,
    },
    /// A request for evidence against the specified validator, from any era that is still bonded
    /// in `era_id`.
    EvidenceRequest { era_id: EraId, pub_key: PublicKey },
}

/// A protocol request message, to be handled by the instance in the specified era.
#[derive(DataSize, Clone, Serialize, Deserialize)]
pub(crate) struct ConsensusRequestMessage {
    era_id: EraId,
    payload: EraRequest<ClContext>,
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
    /// An incoming network message.
    #[from]
    Incoming(ConsensusMessageIncoming),
    /// An incoming demand message.
    #[from]
    DemandIncoming(ConsensusDemand),
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
    /// The proposed block has been validated.
    ResolveValidity(ResolveValidity),
    /// Deactivate the era with the given ID, unless the number of faulty validators increases.
    DeactivateEra {
        era_id: EraId,
        faulty_num: usize,
        delay: Duration,
    },
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
                write!(f, "protocol message {:?} in {}", payload, era_id)
            }
            ConsensusMessage::EvidenceRequest { era_id, pub_key } => write!(
                f,
                "request for evidence of fault by {} in {} or earlier",
                pub_key, era_id,
            ),
        }
    }
}

impl Debug for ConsensusRequestMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ConsensusRequestMessage {{ era_id: {:?}, .. }}",
            self.era_id
        )
    }
}

impl Display for ConsensusRequestMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "protocol request {:?} in {}", self.payload, self.era_id)
    }
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Incoming(ConsensusMessageIncoming { sender, message }) => {
                write!(f, "message from {:?}: {}", sender, message)
            }
            Event::DemandIncoming(demand) => {
                write!(f, "demand from {:?}: {}", demand.sender, demand.request_msg)
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
                "New proposed block for era {:?}: {:?}, {:?}",
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
    + From<ConsensusDemand>
    + From<NetworkInfoRequest>
    + From<DeployBufferRequest>
    + From<ConsensusAnnouncement>
    + From<BlockValidationRequest>
    + From<StorageRequest>
    + From<ContractRuntimeRequest>
    + From<ChainspecRawBytesRequest>
    + From<PeerBehaviorAnnouncement>
    + From<FatalAnnouncement>
{
}

impl<REv> ReactorEventT for REv where
    REv: ReactorEvent
        + From<Event>
        + Send
        + From<ConsensusDemand>
        + From<NetworkRequest<Message>>
        + From<NetworkInfoRequest>
        + From<DeployBufferRequest>
        + From<ConsensusAnnouncement>
        + From<BlockValidationRequest>
        + From<StorageRequest>
        + From<ContractRuntimeRequest>
        + From<ChainspecRawBytesRequest>
        + From<PeerBehaviorAnnouncement>
        + From<FatalAnnouncement>
{
}

impl<REv> Component<REv> for EraSupervisor
where
    REv: ReactorEventT,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        trace!("{:?}", event);
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
            Event::DemandIncoming(ConsensusDemand {
                sender,
                request_msg: demand,
                auto_closing_responder,
            }) => self.handle_demand(effect_builder, rng, sender, demand, auto_closing_responder),
            Event::NewBlockPayload(new_block_payload) => {
                self.handle_new_block_payload(effect_builder, rng, new_block_payload)
            }
            Event::BlockAdded {
                header,
                header_hash: _,
            } => self.handle_block_added(effect_builder, rng, *header),
            Event::ResolveValidity(resolve_validity) => {
                self.resolve_validity(effect_builder, rng, resolve_validity)
            }
            Event::DeactivateEra {
                era_id,
                faulty_num,
                delay,
            } => self.handle_deactivate_era(effect_builder, era_id, faulty_num, delay),
            Event::ConsensusRequest(ConsensusRequest::Status(responder)) => self.status(responder),
            Event::ConsensusRequest(ConsensusRequest::ValidatorChanges(responder)) => {
                let validator_changes = self.get_validator_changes();
                responder.respond(validator_changes).ignore()
            }
            Event::DumpState(req @ DumpConsensusStateRequest { era_id, .. }) => {
                let current_era = match self.current_era() {
                    None => {
                        return req
                            .answer(Err(Cow::Owned("consensus not initialized".to_string())))
                            .ignore()
                    }
                    Some(era_id) => era_id,
                };

                let requested_era = era_id.unwrap_or(current_era);

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
