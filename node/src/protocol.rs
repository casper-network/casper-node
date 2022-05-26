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

use crate::{
    components::{
        consensus,
        fetcher::FetchedOrNotFound,
        gossiper,
        small_network::{EstimatorWeights, FromIncoming, GossipedAddress, MessageKind, Payload},
    },
    effect::{
        incoming::{
            ConsensusMessageIncoming, FinalitySignatureIncoming, GossiperIncoming, NetRequest,
            NetRequestIncoming, NetResponse, NetResponseIncoming, TrieDemand, TrieRequest,
            TrieRequestIncoming, TrieResponse, TrieResponseIncoming,
        },
        AutoClosingResponder, EffectBuilder,
    },
    types::{Deploy, FinalitySignature, Item, NodeId, Tag},
};

/// Reactor message.
#[derive(Clone, From, Serialize, Deserialize)]
pub(crate) enum Message {
    /// Consensus component message.
    #[from]
    Consensus(consensus::ConsensusMessage),
    /// Deploy gossiper component message.
    #[from]
    DeployGossiper(gossiper::Message<Deploy>),
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
    fn classify(&self) -> MessageKind {
        match self {
            Message::Consensus(_) => MessageKind::Consensus,
            Message::DeployGossiper(_) => MessageKind::DeployGossip,
            Message::AddressGossiper(_) => MessageKind::AddressGossip,
            Message::GetRequest { tag, .. } | Message::GetResponse { tag, .. } => {
                match tag {
                    Tag::Deploy => MessageKind::DeployTransfer,
                    Tag::FinalizedApprovals => MessageKind::FinalizedApprovalsTransfer,
                    Tag::Block => MessageKind::BlockTransfer,
                    // This is a weird message, which we should not encounter here?
                    Tag::GossipedAddress => MessageKind::Other,
                    Tag::BlockAndMetadataByHeight => MessageKind::BlockTransfer,
                    Tag::BlockHeaderByHash => MessageKind::BlockTransfer,
                    Tag::BlockHeaderAndFinalitySignaturesByHeight => MessageKind::BlockTransfer,
                    Tag::TrieOrChunk => MessageKind::TrieTransfer,
                    Tag::BlockAndDeploysByHash => MessageKind::BlockTransfer,
                }
            }
            Message::FinalitySignature(_) => MessageKind::Consensus,
        }
    }

    fn is_low_priority(&self) -> bool {
        // We only deprioritize requested trie nodes, as they are the most commonly requested item
        // during fast sync.
        match self {
            Message::Consensus(_) => false,
            Message::DeployGossiper(_) => false,
            Message::AddressGossiper(_) => false,
            Message::GetRequest { tag, .. } if *tag == Tag::TrieOrChunk => true,
            Message::GetRequest { .. } => false,
            Message::GetResponse { .. } => false,
            Message::FinalitySignature(_) => false,
        }
    }

    #[inline]
    fn incoming_resource_estimate(&self, weights: &EstimatorWeights) -> u32 {
        match self {
            Message::Consensus(_) => weights.consensus,
            Message::DeployGossiper(_) => weights.gossip,
            Message::AddressGossiper(_) => weights.gossip,
            Message::GetRequest { tag, .. } => match tag {
                Tag::Deploy => weights.deploy_requests,
                Tag::FinalizedApprovals => weights.finalized_approvals_requests,
                Tag::Block => weights.block_requests,
                Tag::GossipedAddress => weights.gossip,
                Tag::BlockAndMetadataByHeight => weights.block_requests,
                Tag::BlockHeaderByHash => weights.block_requests,
                Tag::BlockHeaderAndFinalitySignaturesByHeight => weights.block_requests,
                Tag::TrieOrChunk => weights.trie_requests,
                Tag::BlockAndDeploysByHash => weights.block_requests,
            },
            Message::GetResponse { tag, .. } => match tag {
                Tag::Deploy => weights.deploy_responses,
                Tag::FinalizedApprovals => weights.finalized_approvals_responses,
                Tag::Block => weights.block_responses,
                Tag::GossipedAddress => weights.gossip,
                Tag::BlockAndMetadataByHeight => weights.block_responses,
                Tag::BlockHeaderByHash => weights.block_responses,
                Tag::BlockHeaderAndFinalitySignaturesByHeight => weights.block_responses,
                Tag::TrieOrChunk => weights.trie_responses,
                Tag::BlockAndDeploysByHash => weights.block_requests,
            },
            Message::FinalitySignature(_) => weights.finality_signatures,
        }
    }
}

impl Message {
    pub(crate) fn new_get_request<T: Item>(id: &T::Id) -> Result<Self, bincode::Error> {
        Ok(Message::GetRequest {
            tag: T::TAG,
            serialized_id: bincode::serialize(id)?,
        })
    }

    pub(crate) fn new_get_response<T>(
        item: &FetchedOrNotFound<T, T::Id>,
    ) -> Result<Self, bincode::Error>
    where
        T: Item,
    {
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
            Message::DeployGossiper(dg) => f.debug_tuple("DeployGossiper").field(&dg).finish(),
            Message::AddressGossiper(ga) => f.debug_tuple("AddressGossiper").field(&ga).finish(),
            Message::GetRequest { tag, serialized_id } => f
                .debug_struct("GetRequest")
                .field("tag", tag)
                .field("serialized_item", &HexFmt(serialized_id))
                .finish(),
            Message::GetResponse {
                tag,
                serialized_item,
            } => f
                .debug_struct("GetResponse")
                .field("tag", tag)
                .field("serialized_item", &HexFmt(serialized_item))
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
            Message::DeployGossiper(deploy) => write!(f, "DeployGossiper::{}", deploy),
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
        + From<GossiperIncoming<Deploy>>
        + From<GossiperIncoming<GossipedAddress>>
        + From<NetRequestIncoming>
        + From<NetResponseIncoming>
        + From<TrieRequestIncoming>
        + From<TrieDemand>
        + From<TrieResponseIncoming>
        + From<FinalitySignatureIncoming>,
{
    // fn from_incoming(sender: NodeId, payload: Message, effect_builder: EffectBuilder<REv>) ->
    // Self {
    fn from_incoming(sender: NodeId, payload: Message) -> Self {
        match payload {
            Message::Consensus(message) => ConsensusMessageIncoming { sender, message }.into(),
            Message::DeployGossiper(message) => GossiperIncoming { sender, message }.into(),
            Message::AddressGossiper(message) => GossiperIncoming { sender, message }.into(),
            Message::GetRequest { tag, serialized_id } => match tag {
                Tag::Deploy => NetRequestIncoming {
                    sender,
                    message: NetRequest::Deploy(serialized_id),
                }
                .into(),
                Tag::FinalizedApprovals => NetRequestIncoming {
                    sender,
                    message: NetRequest::FinalizedApprovals(serialized_id),
                }
                .into(),
                Tag::Block => NetRequestIncoming {
                    sender,
                    message: NetRequest::Block(serialized_id),
                }
                .into(),
                Tag::GossipedAddress => NetRequestIncoming {
                    sender,
                    message: NetRequest::GossipedAddress(serialized_id),
                }
                .into(),
                Tag::BlockAndMetadataByHeight => NetRequestIncoming {
                    sender,
                    message: NetRequest::BlockAndMetadataByHeight(serialized_id),
                }
                .into(),
                Tag::BlockHeaderByHash => NetRequestIncoming {
                    sender,
                    message: NetRequest::BlockHeaderByHash(serialized_id),
                }
                .into(),
                Tag::BlockHeaderAndFinalitySignaturesByHeight => NetRequestIncoming {
                    sender,
                    message: NetRequest::BlockHeaderAndFinalitySignaturesByHeight(serialized_id),
                }
                .into(),
                Tag::TrieOrChunk => TrieRequestIncoming {
                    sender,
                    message: TrieRequest(serialized_id),
                }
                .into(),
                Tag::BlockAndDeploysByHash => NetRequestIncoming {
                    sender,
                    message: NetRequest::BlockAndDeploys(serialized_id),
                }
                .into(),
            },
            Message::GetResponse {
                tag,
                serialized_item,
            } => match tag {
                Tag::Deploy => NetResponseIncoming {
                    sender,
                    message: NetResponse::Deploy(serialized_item),
                }
                .into(),
                Tag::FinalizedApprovals => NetResponseIncoming {
                    sender,
                    message: NetResponse::FinalizedApprovals(serialized_item),
                }
                .into(),
                Tag::Block => NetResponseIncoming {
                    sender,
                    message: NetResponse::Block(serialized_item),
                }
                .into(),
                Tag::GossipedAddress => NetResponseIncoming {
                    sender,
                    message: NetResponse::GossipedAddress(serialized_item),
                }
                .into(),
                Tag::BlockAndMetadataByHeight => NetResponseIncoming {
                    sender,
                    message: NetResponse::BlockAndMetadataByHeight(serialized_item),
                }
                .into(),
                Tag::BlockHeaderByHash => NetResponseIncoming {
                    sender,
                    message: NetResponse::BlockHeaderByHash(serialized_item),
                }
                .into(),
                Tag::BlockHeaderAndFinalitySignaturesByHeight => NetResponseIncoming {
                    sender,
                    message: NetResponse::BlockHeaderAndFinalitySignaturesByHeight(serialized_item),
                }
                .into(),
                Tag::TrieOrChunk => TrieResponseIncoming {
                    sender,
                    message: TrieResponse(serialized_item.to_vec()),
                }
                .into(),
                Tag::BlockAndDeploysByHash => NetResponseIncoming {
                    sender,
                    message: NetResponse::BlockAndDeploys(serialized_item),
                }
                .into(),
            },
            Message::FinalitySignature(message) => {
                FinalitySignatureIncoming { sender, message }.into()
            }
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
            Message::GetRequest { tag, serialized_id } if tag == Tag::TrieOrChunk => {
                let (ev, fut) = effect_builder.create_request_parts(move |responder| TrieDemand {
                    sender,
                    request_msg: TrieRequest(serialized_id),
                    auto_closing_responder: AutoClosingResponder::from_opt_responder(responder),
                });

                Ok((ev, fut.boxed()))
            }
            _ => Err(payload),
        }
    }
}
