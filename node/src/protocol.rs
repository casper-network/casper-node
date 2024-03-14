//! A network message type used for communication between nodes

use std::{
    fmt::{self, Display, Formatter},
    sync::Arc,
};

use derive_more::From;
use fmt::Debug;
use futures::{future::BoxFuture, FutureExt};
use hex_fmt::HexFmt;
use serde::{Deserialize, Serialize};
use strum::EnumDiscriminants;

use crate::{
    components::{
        consensus,
        fetcher::{FetchItem, FetchResponse, Tag},
        gossiper,
        network::{Channel, FromIncoming, GossipedAddress, MessageKind, Payload, Ticket},
    },
    effect::{
        incoming::{
            ConsensusDemand, ConsensusMessageIncoming, FinalitySignatureIncoming, GossiperIncoming,
            NetRequest, NetRequestIncoming, NetResponse, NetResponseIncoming, TrieDemand,
            TrieRequest, TrieRequestIncoming, TrieResponse, TrieResponseIncoming,
        },
        AutoClosingResponder, EffectBuilder,
    },
    types::{Block, Deploy, FinalitySignature, NodeId},
};

/// Reactor message.
#[derive(Clone, From, Serialize, Deserialize, EnumDiscriminants)]
#[strum_discriminants(derive(strum::EnumIter))]
pub(crate) enum Message {
    /// Consensus component message.
    #[from]
    Consensus(consensus::ConsensusMessage),
    /// Consensus component demand.
    #[from]
    ConsensusRequest(consensus::ConsensusRequestMessage),
    /// Block gossiper component message.
    #[from]
    BlockGossiper(gossiper::Message<Block>),
    /// Deploy gossiper component message.
    #[from]
    DeployGossiper(gossiper::Message<Deploy>),
    #[from]
    FinalitySignatureGossiper(gossiper::Message<FinalitySignature>),
    /// Address gossiper component message.
    #[from]
    AddressGossiper(gossiper::Message<GossipedAddress>),
    /// Request to get an item from a peer.
    GetRequest {
        /// The type tag of the requested item.
        tag: Tag,
        /// The serialized ID of the requested item.
        serialized_id: Vec<u8>,
    },
    /// Response to a `GetRequest`.
    GetResponse {
        /// The type tag of the contained item.
        tag: Tag,
        /// The serialized item.
        serialized_item: Arc<[u8]>,
    },
    /// Finality signature.
    #[from]
    FinalitySignature(Box<FinalitySignature>),
}

impl Payload for Message {
    #[inline]
    fn message_kind(&self) -> MessageKind {
        match self {
            Message::Consensus(_) => MessageKind::Consensus,
            Message::ConsensusRequest(_) => MessageKind::Consensus,
            Message::BlockGossiper(_) => MessageKind::BlockGossip,
            Message::DeployGossiper(_) => MessageKind::DeployGossip,
            Message::AddressGossiper(_) => MessageKind::AddressGossip,
            Message::GetRequest { tag, .. } | Message::GetResponse { tag, .. } => match tag {
                Tag::Deploy | Tag::LegacyDeploy => MessageKind::DeployTransfer,
                Tag::Block => MessageKind::BlockTransfer,
                Tag::BlockHeader => MessageKind::BlockTransfer,
                Tag::TrieOrChunk => MessageKind::TrieTransfer,
                Tag::FinalitySignature => MessageKind::Other,
                Tag::SyncLeap => MessageKind::BlockTransfer,
                Tag::ApprovalsHashes => MessageKind::BlockTransfer,
                Tag::BlockExecutionResults => MessageKind::BlockTransfer,
            },
            Message::FinalitySignature(_) => MessageKind::Consensus,
            Message::FinalitySignatureGossiper(_) => MessageKind::FinalitySignatureGossip,
        }
    }

    fn is_low_priority(&self) -> bool {
        // We only deprioritize requested trie nodes, as they are the most commonly requested item
        // during fast sync.
        match self {
            Message::Consensus(_) => false,
            Message::ConsensusRequest(_) => false,
            Message::DeployGossiper(_) => false,
            Message::BlockGossiper(_) => false,
            Message::FinalitySignatureGossiper(_) => false,
            Message::AddressGossiper(_) => false,
            Message::GetRequest { tag, .. } if *tag == Tag::TrieOrChunk => true,
            Message::GetRequest { .. } => false,
            Message::GetResponse { .. } => false,
            Message::FinalitySignature(_) => false,
        }
    }

    #[inline]
    fn get_channel(&self) -> Channel {
        match self {
            Message::Consensus(_) => Channel::Consensus,
            Message::DeployGossiper(_) => Channel::BulkGossip,
            Message::AddressGossiper(_) => Channel::BulkGossip,
            Message::GetRequest {
                tag,
                serialized_id: _,
            } => match tag {
                Tag::Deploy => Channel::DataRequests,
                Tag::LegacyDeploy => Channel::SyncDataRequests,
                Tag::Block => Channel::SyncDataRequests,
                Tag::BlockHeader => Channel::SyncDataRequests,
                Tag::TrieOrChunk => Channel::SyncDataRequests,
                Tag::FinalitySignature => Channel::DataRequests,
                Tag::SyncLeap => Channel::SyncDataRequests,
                Tag::ApprovalsHashes => Channel::SyncDataRequests,
                Tag::BlockExecutionResults => Channel::SyncDataRequests,
            },
            Message::GetResponse {
                tag,
                serialized_item: _,
            } => match tag {
                // TODO: Verify which responses are for sync data.
                Tag::Deploy => Channel::DataResponses,
                Tag::LegacyDeploy => Channel::SyncDataResponses,
                Tag::Block => Channel::SyncDataResponses,
                Tag::BlockHeader => Channel::SyncDataResponses,
                Tag::TrieOrChunk => Channel::SyncDataResponses,
                Tag::FinalitySignature => Channel::DataResponses,
                Tag::SyncLeap => Channel::SyncDataResponses,
                Tag::ApprovalsHashes => Channel::SyncDataResponses,
                Tag::BlockExecutionResults => Channel::SyncDataResponses,
            },
            Message::FinalitySignature(_) => Channel::Consensus,
            Message::ConsensusRequest(_) => Channel::Consensus,
            Message::BlockGossiper(_) => Channel::BulkGossip,
            Message::FinalitySignatureGossiper(_) => Channel::BulkGossip,
        }
    }
}

impl Message {
    pub(crate) fn new_get_request<T: FetchItem>(id: &T::Id) -> Result<Self, bincode::Error> {
        Ok(Message::GetRequest {
            tag: T::TAG,
            serialized_id: bincode::serialize(id)?,
        })
    }

    pub(crate) fn new_get_response<T: FetchItem>(
        item: &FetchResponse<T, T::Id>,
    ) -> Result<Self, bincode::Error> {
        Ok(Message::GetResponse {
            tag: T::TAG,
            serialized_item: item.to_serialized()?.into(),
        })
    }

    /// Creates a new get response from already serialized data.
    pub(crate) fn new_get_response_from_serialized(tag: Tag, serialized_item: Arc<[u8]>) -> Self {
        Message::GetResponse {
            tag,
            serialized_item,
        }
    }
}

impl Debug for Message {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Message::Consensus(c) => f.debug_tuple("Consensus").field(&c).finish(),
            Message::ConsensusRequest(c) => f.debug_tuple("ConsensusRequest").field(&c).finish(),
            Message::BlockGossiper(dg) => f.debug_tuple("BlockGossiper").field(&dg).finish(),
            Message::DeployGossiper(dg) => f.debug_tuple("DeployGossiper").field(&dg).finish(),
            Message::FinalitySignatureGossiper(sig) => f
                .debug_tuple("FinalitySignatureGossiper")
                .field(&sig)
                .finish(),
            Message::AddressGossiper(ga) => f.debug_tuple("AddressGossiper").field(&ga).finish(),
            Message::GetRequest { tag, serialized_id } => f
                .debug_struct("GetRequest")
                .field("tag", tag)
                .field("serialized_id", &HexFmt(serialized_id))
                .finish(),
            Message::GetResponse {
                tag,
                serialized_item,
            } => f
                .debug_struct("GetResponse")
                .field("tag", tag)
                .field(
                    "serialized_item",
                    &format!("{} bytes", serialized_item.len()),
                )
                .finish(),
            Message::FinalitySignature(fs) => {
                f.debug_tuple("FinalitySignature").field(&fs).finish()
            }
        }
    }
}

impl Display for Message {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Message::Consensus(consensus) => write!(f, "Consensus::{}", consensus),
            Message::ConsensusRequest(consensus) => write!(f, "ConsensusRequest({})", consensus),
            Message::BlockGossiper(deploy) => write!(f, "BlockGossiper::{}", deploy),
            Message::DeployGossiper(deploy) => write!(f, "DeployGossiper::{}", deploy),
            Message::FinalitySignatureGossiper(sig) => {
                write!(f, "FinalitySignatureGossiper::{}", sig)
            }
            Message::AddressGossiper(gossiped_address) => {
                write!(f, "AddressGossiper::({})", gossiped_address)
            }
            Message::GetRequest { tag, serialized_id } => {
                write!(f, "GetRequest({}-{:10})", tag, HexFmt(serialized_id))
            }
            Message::GetResponse {
                tag,
                serialized_item,
            } => write!(f, "GetResponse({}-{:10})", tag, HexFmt(serialized_item)),
            Message::FinalitySignature(fs) => {
                write!(f, "FinalitySignature::({})", fs)
            }
        }
    }
}

impl<REv> FromIncoming<Message> for REv
where
    REv: From<ConsensusMessageIncoming>
        + From<ConsensusDemand>
        + From<GossiperIncoming<Block>>
        + From<GossiperIncoming<Deploy>>
        + From<GossiperIncoming<FinalitySignature>>
        + From<GossiperIncoming<GossipedAddress>>
        + From<NetRequestIncoming>
        + From<NetResponseIncoming>
        + From<TrieRequestIncoming>
        + From<TrieDemand>
        + From<TrieResponseIncoming>
        + From<FinalitySignatureIncoming>,
{
    fn from_incoming(sender: NodeId, payload: Message, ticket: Ticket) -> Self {
        let ticket = Arc::new(ticket);
        match payload {
            Message::Consensus(message) => ConsensusMessageIncoming {
                sender,
                message: Box::new(message),
                ticket,
            }
            .into(),
            Message::ConsensusRequest(_message) => {
                // TODO: Remove this once from_incoming and try_demand_from_incoming are unified.
                unreachable!("called from_incoming with a consensus request")
            }
            Message::BlockGossiper(message) => GossiperIncoming {
                sender,
                message: Box::new(message),
                ticket,
            }
            .into(),
            Message::DeployGossiper(message) => GossiperIncoming {
                sender,

                message: Box::new(message),
                ticket,
            }
            .into(),
            Message::FinalitySignatureGossiper(message) => GossiperIncoming {
                sender,
                message: Box::new(message),
                ticket,
            }
            .into(),
            Message::AddressGossiper(message) => GossiperIncoming {
                sender,

                message: Box::new(message),
                ticket,
            }
            .into(),
            Message::GetRequest { tag, serialized_id } => match tag {
                Tag::Deploy => NetRequestIncoming {
                    sender,
                    message: Box::new(NetRequest::Deploy(serialized_id)),
                    ticket,
                }
                .into(),
                Tag::LegacyDeploy => NetRequestIncoming {
                    sender,

                    message: Box::new(NetRequest::LegacyDeploy(serialized_id)),
                    ticket,
                }
                .into(),
                Tag::Block => NetRequestIncoming {
                    sender,
                    message: Box::new(NetRequest::Block(serialized_id)),
                    ticket,
                }
                .into(),
                Tag::BlockHeader => NetRequestIncoming {
                    sender,
                    message: Box::new(NetRequest::BlockHeader(serialized_id)),
                    ticket,
                }
                .into(),
                Tag::TrieOrChunk => TrieRequestIncoming {
                    sender,
                    message: Box::new(TrieRequest(serialized_id)),
                    ticket,
                }
                .into(),
                Tag::FinalitySignature => NetRequestIncoming {
                    sender,

                    message: Box::new(NetRequest::FinalitySignature(serialized_id)),
                    ticket,
                }
                .into(),
                Tag::SyncLeap => NetRequestIncoming {
                    sender,
                    message: Box::new(NetRequest::SyncLeap(serialized_id)),
                    ticket,
                }
                .into(),
                Tag::ApprovalsHashes => NetRequestIncoming {
                    sender,

                    message: Box::new(NetRequest::ApprovalsHashes(serialized_id)),
                    ticket,
                }
                .into(),
                Tag::BlockExecutionResults => NetRequestIncoming {
                    sender,

                    message: Box::new(NetRequest::BlockExecutionResults(serialized_id)),
                    ticket,
                }
                .into(),
            },
            Message::GetResponse {
                tag,
                serialized_item,
            } => match tag {
                Tag::Deploy => NetResponseIncoming {
                    sender,
                    message: Box::new(NetResponse::Deploy(serialized_item)),
                    ticket,
                }
                .into(),
                Tag::LegacyDeploy => NetResponseIncoming {
                    sender,

                    message: Box::new(NetResponse::LegacyDeploy(serialized_item)),
                    ticket,
                }
                .into(),
                Tag::Block => NetResponseIncoming {
                    sender,
                    message: Box::new(NetResponse::Block(serialized_item)),
                    ticket,
                }
                .into(),
                Tag::BlockHeader => NetResponseIncoming {
                    sender,

                    message: Box::new(NetResponse::BlockHeader(serialized_item)),
                    ticket,
                }
                .into(),
                Tag::TrieOrChunk => TrieResponseIncoming {
                    sender,

                    message: Box::new(TrieResponse(serialized_item.to_vec())),
                    ticket,
                }
                .into(),
                Tag::FinalitySignature => NetResponseIncoming {
                    sender,

                    message: Box::new(NetResponse::FinalitySignature(serialized_item)),
                    ticket,
                }
                .into(),
                Tag::SyncLeap => NetResponseIncoming {
                    sender,
                    message: Box::new(NetResponse::SyncLeap(serialized_item)),
                    ticket,
                }
                .into(),
                Tag::ApprovalsHashes => NetResponseIncoming {
                    sender,
                    message: Box::new(NetResponse::ApprovalsHashes(serialized_item)),
                    ticket,
                }
                .into(),
                Tag::BlockExecutionResults => NetResponseIncoming {
                    sender,
                    message: Box::new(NetResponse::BlockExecutionResults(serialized_item)),
                    ticket,
                }
                .into(),
            },
            Message::FinalitySignature(message) => FinalitySignatureIncoming {
                sender,
                message,
                ticket,
            }
            .into(),
        }
    }

    fn try_demand_from_incoming(
        effect_builder: EffectBuilder<REv>,
        sender: NodeId,
        payload: Message,
    ) -> Result<(Self, BoxFuture<'static, Option<Message>>), Message>
    where
        Self: Sized + Send,
    {
        match payload {
            Message::GetRequest {
                tag: Tag::TrieOrChunk,
                serialized_id,
            } => {
                let (ev, fut) = effect_builder.create_request_parts(move |responder| TrieDemand {
                    sender,
                    request_msg: Box::new(TrieRequest(serialized_id)),
                    auto_closing_responder: AutoClosingResponder::from_opt_responder(responder),
                });

                Ok((ev, fut.boxed()))
            }
            Message::ConsensusRequest(request_msg) => {
                let (ev, fut) =
                    effect_builder.create_request_parts(move |responder| ConsensusDemand {
                        sender,
                        request_msg: Box::new(request_msg),
                        auto_closing_responder: AutoClosingResponder::from_opt_responder(responder),
                    });

                Ok((ev, fut.boxed()))
            }
            _ => Err(payload),
        }
    }
}
