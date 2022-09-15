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
        fetcher::FetchResponse,
        gossiper,
        small_network::{EstimatorWeights, FromIncoming, GossipedAddress, MessageKind, Payload},
    },
    effect::{
        incoming::{
            BlockAddedRequest, BlockAddedRequestIncoming, BlockAddedResponse,
            BlockAddedResponseIncoming, ConsensusMessageIncoming, FinalitySignatureIncoming,
            GossiperIncoming, NetRequest, NetRequestIncoming, NetResponse, NetResponseIncoming,
            SyncLeapRequest, SyncLeapRequestIncoming, SyncLeapResponse, SyncLeapResponseIncoming,
            TrieDemand, TrieRequest, TrieRequestIncoming, TrieResponse, TrieResponseIncoming,
        },
        AutoClosingResponder, EffectBuilder,
    },
    types::{BlockAdded, Deploy, FetcherItem, FinalitySignature, GossiperItem, NodeId, Tag},
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
    /// BlockAdded gossiper component message.
    #[from]
    BlockAddedGossiper(gossiper::Message<BlockAdded>),
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
            Message::DeployGossiper(_) => MessageKind::DeployGossip,
            Message::BlockAddedGossiper(_) => MessageKind::BlockGossip,
            Message::FinalitySignatureGossiper(_) => MessageKind::FinalitySignatureGossip,
            Message::AddressGossiper(_) => MessageKind::AddressGossip,
            Message::GetRequest { tag, .. } | Message::GetResponse { tag, .. } => {
                match tag {
                    Tag::Deploy => MessageKind::DeployTransfer,
                    Tag::FinalizedApprovals => MessageKind::FinalizedApprovalsTransfer,
                    Tag::Block => MessageKind::BlockTransfer,
                    Tag::BlockAndMetadataByHeight => MessageKind::BlockTransfer,
                    Tag::BlockHeaderByHash => MessageKind::BlockTransfer,
                    Tag::BlockHeaderAndFinalitySignaturesByHeight => MessageKind::BlockTransfer,
                    Tag::TrieOrChunk => MessageKind::TrieTransfer,
                    Tag::BlockAndDeploysByHash => MessageKind::BlockTransfer,
                    Tag::BlockHeaderBatch => MessageKind::BlockTransfer,
                    Tag::FinalitySignaturesByHash => MessageKind::BlockTransfer,
                    Tag::SyncLeap => MessageKind::BlockTransfer,
                    Tag::BlockAdded => MessageKind::BlockTransfer,

                    // The following tags should be unreachable.
                    Tag::FinalitySignature => MessageKind::Other,
                    Tag::GossipedAddress => MessageKind::Other,
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
            Message::BlockAddedGossiper(_) => false,
            Message::FinalitySignatureGossiper(_) => false,
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
            Message::BlockAddedGossiper(_) => weights.gossip,
            Message::FinalitySignatureGossiper(_) => weights.gossip,
            Message::AddressGossiper(_) => weights.gossip,
            Message::GetRequest { tag, .. } => match tag {
                Tag::Deploy => weights.deploy_requests,
                Tag::FinalizedApprovals => weights.finalized_approvals_requests,
                Tag::Block => weights.block_requests,
                Tag::FinalitySignature => weights.gossip,
                Tag::GossipedAddress => weights.gossip,
                Tag::BlockAndMetadataByHeight => weights.block_requests,
                Tag::BlockHeaderByHash => weights.block_requests,
                Tag::BlockHeaderAndFinalitySignaturesByHeight => weights.block_requests,
                Tag::TrieOrChunk => weights.trie_requests,
                Tag::BlockAndDeploysByHash => weights.block_requests,
                Tag::BlockHeaderBatch => weights.block_requests,
                Tag::FinalitySignaturesByHash => weights.block_requests,
                Tag::SyncLeap => weights.block_requests,
                Tag::BlockAdded => weights.block_requests,
            },
            Message::GetResponse { tag, .. } => match tag {
                Tag::Deploy => weights.deploy_responses,
                Tag::FinalizedApprovals => weights.finalized_approvals_responses,
                Tag::Block => weights.block_responses,
                Tag::FinalitySignature => weights.gossip,
                Tag::GossipedAddress => weights.gossip,
                Tag::BlockAndMetadataByHeight => weights.block_responses,
                Tag::BlockHeaderByHash => weights.block_responses,
                Tag::BlockHeaderAndFinalitySignaturesByHeight => weights.block_responses,
                Tag::TrieOrChunk => weights.trie_responses,
                Tag::BlockAndDeploysByHash => weights.block_requests,
                Tag::BlockHeaderBatch => weights.block_responses,
                Tag::FinalitySignaturesByHash => weights.block_responses,
                Tag::SyncLeap => weights.block_responses,
                Tag::BlockAdded => weights.block_responses,
            },
            Message::FinalitySignature(_) => weights.finality_signatures,
        }
    }

    fn is_unsafe_for_syncing_peers(&self) -> bool {
        match self {
            Message::Consensus(_) => false,
            Message::DeployGossiper(_) => false,
            Message::BlockAddedGossiper(_) => false,
            Message::FinalitySignatureGossiper(_) => false,
            Message::AddressGossiper(_) => false,
            // Trie requests can deadlock between syncing nodes.
            Message::GetRequest { tag, .. } if *tag == Tag::TrieOrChunk => true,
            Message::GetRequest { .. } => false,
            Message::GetResponse { .. } => false,
            Message::FinalitySignature(_) => false,
        }
    }

    /*// Checks the validity of the message.
    fn is_valid(&self, validator_sets: Arc<RwLock<ValidatorSets>>) -> Validity {
        match self {
            Message::Consensus(_) => Validity::Valid,
            Message::DeployGossiper(_) => Validity::Valid,
            Message::BlockGossiper(_) => Validity::Valid,
            Message::FinalitySignatureGossiper(gossiper::Message::Gossip(item_id))
            | Message::FinalitySignatureGossiper(gossiper::Message::GossipResponse {
                item_id,
                ..
            }) => {
                if let Ok(validator_sets) = validator_sets.read() {
                    match validator_sets.is_validator_in_era(item_id.era_id, &item_id.public_key) {
                        Some(false) => Validity::Malicious,
                        Some(true) => Validity::Valid,
                        None => Validity::NotValid,
                    }
                } else {
                    error!("could not get validators, lock poisoned");
                    Validity::NotValid
                }
            }
            Message::AddressGossiper(_) => Validity::Valid,
            Message::GetRequest { .. } => Validity::Valid,
            Message::GetResponse { .. } => Validity::Valid,
            Message::FinalitySignature(_) => Validity::Valid,
        }
    }*/
}

impl Message {
    pub(crate) fn new_get_request<T: FetcherItem>(id: &T::Id) -> Result<Self, bincode::Error> {
        Ok(Message::GetRequest {
            tag: T::TAG,
            serialized_id: bincode::serialize(id)?,
        })
    }

    pub(crate) fn new_get_request_for_gossiper<T: GossiperItem>(
        id: &T::Id,
    ) -> Result<Self, bincode::Error> {
        Ok(Message::GetRequest {
            tag: T::TAG,
            serialized_id: bincode::serialize(id)?,
        })
    }

    pub(crate) fn new_get_response<T: FetcherItem>(
        item: &FetchResponse<T, T::Id>,
    ) -> Result<Self, bincode::Error> {
        Ok(Message::GetResponse {
            tag: T::TAG,
            serialized_item: item.to_serialized()?.into(),
        })
    }

    pub(crate) fn new_get_response_for_gossiper<T: GossiperItem>(
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
            Message::DeployGossiper(dg) => f.debug_tuple("DeployGossiper").field(&dg).finish(),
            Message::BlockAddedGossiper(bg) => f.debug_tuple("BlockGossiper").field(&bg).finish(),
            Message::FinalitySignatureGossiper(sig) => f
                .debug_tuple("FinalitySignatureGossiper")
                .field(&sig)
                .finish(),
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
            Message::BlockAddedGossiper(block) => write!(f, "BlockGossiper::{}", block),
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
        + From<GossiperIncoming<Deploy>>
        + From<GossiperIncoming<BlockAdded>>
        + From<GossiperIncoming<FinalitySignature>>
        + From<GossiperIncoming<GossipedAddress>>
        + From<NetRequestIncoming>
        + From<NetResponseIncoming>
        + From<TrieRequestIncoming>
        + From<TrieDemand>
        + From<TrieResponseIncoming>
        + From<SyncLeapRequestIncoming>
        + From<SyncLeapResponseIncoming>
        + From<BlockAddedRequestIncoming>
        + From<BlockAddedResponseIncoming>
        + From<FinalitySignatureIncoming>,
{
    // fn from_incoming(sender: NodeId, payload: Message, effect_builder: EffectBuilder<REv>) ->
    // Self {
    fn from_incoming(sender: NodeId, payload: Message) -> Self {
        match payload {
            Message::Consensus(message) => ConsensusMessageIncoming { sender, message }.into(),
            Message::DeployGossiper(message) => GossiperIncoming { sender, message }.into(),
            Message::BlockAddedGossiper(message) => GossiperIncoming { sender, message }.into(),
            Message::FinalitySignatureGossiper(message) => {
                GossiperIncoming { sender, message }.into()
            }
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
                Tag::BlockHeaderBatch => NetRequestIncoming {
                    sender,
                    message: NetRequest::BlockHeadersBatch(serialized_id),
                }
                .into(),
                Tag::FinalitySignature => NetRequestIncoming {
                    sender,
                    message: NetRequest::FinalitySignature(serialized_id),
                }
                .into(),
                Tag::FinalitySignaturesByHash => NetRequestIncoming {
                    sender,
                    message: NetRequest::FinalitySignatures(serialized_id),
                }
                .into(),
                Tag::SyncLeap => SyncLeapRequestIncoming {
                    sender,
                    message: SyncLeapRequest(serialized_id),
                }
                .into(),
                Tag::BlockAdded => BlockAddedRequestIncoming {
                    sender,
                    message: BlockAddedRequest(serialized_id),
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
                Tag::FinalitySignature => NetResponseIncoming {
                    sender,
                    message: NetResponse::FinalitySignature(serialized_item),
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
                Tag::BlockHeaderBatch => NetResponseIncoming {
                    sender,
                    message: NetResponse::BlockHeadersBatch(serialized_item),
                }
                .into(),
                Tag::FinalitySignaturesByHash => NetResponseIncoming {
                    sender,
                    message: NetResponse::FinalitySignatures(serialized_item),
                }
                .into(),
                Tag::SyncLeap => SyncLeapResponseIncoming {
                    sender,
                    message: SyncLeapResponse(serialized_item.to_vec()),
                }
                .into(),
                Tag::BlockAdded => BlockAddedResponseIncoming {
                    sender,
                    message: BlockAddedResponse(serialized_item.to_vec()),
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
