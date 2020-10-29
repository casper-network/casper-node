//! The consensus component. Provides distributed consensus among the nodes in the network.

mod candidate_block;
mod cl_context;
mod config;
mod consensus_protocol;
mod era_supervisor;
mod highway_core;
mod metrics;
mod protocols;
#[cfg(test)]
mod tests;
mod traits;

use std::{
    convert::Infallible,
    fmt::{self, Debug, Display, Formatter},
};

use datasize::DataSize;

use casper_execution_engine::core::engine_state::era_validators::GetEraValidatorsError;
use casper_types::auction::ValidatorWeights;

use crate::{
    components::{storage::Storage, Component},
    crypto::{asymmetric_key::PublicKey, hash::Digest},
    effect::{
        announcements::ConsensusAnnouncement,
        requests::{
            self, BlockExecutorRequest, BlockValidationRequest, ContractRuntimeRequest,
            DeployBufferRequest, NetworkRequest, StorageRequest,
        },
        EffectBuilder, Effects,
    },
    protocol::Message,
    types::{BlockHash, BlockHeader, CryptoRngCore, ProtoBlock, Timestamp},
};

pub use config::Config;
pub(crate) use consensus_protocol::{BlockContext, EraEnd};
use derive_more::From;
pub(crate) use era_supervisor::{EraId, EraSupervisor};
use hex_fmt::HexFmt;
use serde::{Deserialize, Serialize};
use tracing::error;
use traits::NodeIdT;

#[derive(Debug, DataSize, Clone, Serialize, Deserialize)]
pub enum ConsensusMessage {
    /// A protocol message, to be handled by the instance in the specified era.
    Protocol { era_id: EraId, payload: Vec<u8> },
    /// A request for evidence against the specified validator, from any era that is still bonded
    /// in `era_id`.
    EvidenceRequest { era_id: EraId, pub_key: PublicKey },
}

/// Consensus component event.
#[derive(DataSize, Debug, From)]
pub enum Event<I> {
    /// An incoming network message.
    MessageReceived { sender: I, msg: ConsensusMessage },
    /// A scheduled event to be handled by a specified era
    Timer { era_id: EraId, timestamp: Timestamp },
    /// We are receiving the data we require to propose a new block
    NewProtoBlock {
        era_id: EraId,
        proto_block: ProtoBlock,
        block_context: BlockContext,
    },
    #[from]
    ConsensusRequest(requests::ConsensusRequest),
    /// The proto-block has been validated
    ResolveValidity {
        era_id: EraId,
        sender: I,
        proto_block: ProtoBlock,
        valid: bool,
    },
    /// Event raised when a new era should be created: once we get the set of validators, the
    /// booking block hash and the seed from the key block
    CreateNewEra {
        /// The header of the switch block
        block_header: Box<BlockHeader>,
        /// Ok(block_hash) if the booking block was found, Err(height) if not
        booking_block_hash: Result<BlockHash, u64>,
        /// Ok(seed) if the key block was found, Err(height) if not
        key_block_seed: Result<Digest, u64>,
        get_validators_result: Result<Option<ValidatorWeights>, GetEraValidatorsError>,
    },
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

impl<I: Debug> Display for Event<I> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::MessageReceived { sender, msg } => write!(f, "msg from {:?}: {}", sender, msg),
            Event::Timer { era_id, timestamp } => write!(
                f,
                "timer for era {:?} scheduled for timestamp {}",
                era_id, timestamp
            ),
            Event::NewProtoBlock {
                era_id,
                proto_block,
                block_context,
            } => write!(
                f,
                "New proto-block for era {:?}: {:?}, {:?}",
                era_id, proto_block, block_context
            ),
            Event::ConsensusRequest(request) => write!(
                f,
                "A request for consensus component hash been receieved: {:?}",
                request
            ),
            Event::ResolveValidity {
                era_id,
                sender,
                proto_block,
                valid,
            } => write!(
                f,
                "Proto-block received from {:?} for {} is {}: {:?}",
                sender,
                era_id,
                if *valid { "valid" } else { "invalid" },
                proto_block
            ),
            Event::CreateNewEra {
                booking_block_hash,
                key_block_seed,
                get_validators_result,
                ..
            } => write!(
                f,
                "New era should be created; booking block hash: {:?}, key block seed: {:?}, \
                response to get_validators from the contract runtime: {:?}",
                booking_block_hash, key_block_seed, get_validators_result
            ),
        }
    }
}

/// A helper trait whose bounds represent the requirements for a reactor event that `EraSupervisor`
/// can work with.
pub trait ReactorEventT<I>:
    From<Event<I>>
    + Send
    + From<NetworkRequest<I, Message>>
    + From<DeployBufferRequest>
    + From<ConsensusAnnouncement>
    + From<BlockExecutorRequest>
    + From<BlockValidationRequest<ProtoBlock, I>>
    + From<StorageRequest<Storage>>
    + From<ContractRuntimeRequest>
{
}

impl<REv, I> ReactorEventT<I> for REv where
    REv: From<Event<I>>
        + Send
        + From<NetworkRequest<I, Message>>
        + From<DeployBufferRequest>
        + From<ConsensusAnnouncement>
        + From<BlockExecutorRequest>
        + From<BlockValidationRequest<ProtoBlock, I>>
        + From<StorageRequest<Storage>>
        + From<ContractRuntimeRequest>
{
}

impl<I, REv> Component<REv> for EraSupervisor<I>
where
    I: NodeIdT,
    REv: ReactorEventT<I>,
{
    type Event = Event<I>;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        mut rng: &mut dyn CryptoRngCore,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        let mut handling_es = self.handling_wrapper(effect_builder, &mut rng);
        match event {
            Event::Timer { era_id, timestamp } => handling_es.handle_timer(era_id, timestamp),
            Event::MessageReceived { sender, msg } => handling_es.handle_message(sender, msg),
            Event::NewProtoBlock {
                era_id,
                proto_block,
                block_context,
            } => handling_es.handle_new_proto_block(era_id, proto_block, block_context),
            Event::ConsensusRequest(requests::ConsensusRequest::HandleLinearBlock(
                block_header,
                responder,
            )) => handling_es.handle_linear_chain_block(*block_header, responder),
            Event::ResolveValidity {
                era_id,
                sender,
                proto_block,
                valid,
            } => handling_es.resolve_validity(era_id, sender, proto_block, valid),
            Event::CreateNewEra {
                block_header,
                booking_block_hash,
                key_block_seed,
                get_validators_result,
            } => {
                let booking_block_hash = booking_block_hash.unwrap_or_else(|height| {
                    error!(
                        "could not find the booking block at height {} for era {}",
                        height,
                        block_header.era_id().successor()
                    );
                    panic!("couldn't get the booking block hash");
                });
                let key_block_seed = key_block_seed.unwrap_or_else(|height| {
                    error!(
                        "could not find the key block at height {} for era {}",
                        height,
                        block_header.era_id().successor()
                    );
                    panic!("couldn't get the seed from the key block");
                });
                let validators = match get_validators_result {
                    Ok(Some(result)) => result,
                    result => {
                        error!(
                            ?result,
                            "get_validators in era {} returned an error: {:?}",
                            block_header.era_id(),
                            result
                        );
                        panic!("couldn't get validators");
                    }
                };
                handling_es.handle_create_new_era(
                    *block_header,
                    booking_block_hash,
                    key_block_seed,
                    validators,
                )
            }
        }
    }
}
