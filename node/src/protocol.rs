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

use casper_types::{BlockV2, FinalitySignatureV2, Transaction};

use crate::{
    components::{
        consensus,
        fetcher::{FetchItem, FetchResponse, Tag},
        gossiper,
        network::{EstimatorWeights, FromIncoming, GossipedAddress, MessageKind, Payload},
    },
    effect::{
        incoming::{
            ConsensusDemand, ConsensusMessageIncoming, FinalitySignatureIncoming, GossiperIncoming,
            NetRequest, NetRequestIncoming, NetResponse, NetResponseIncoming, TrieDemand,
            TrieRequest, TrieRequestIncoming, TrieResponse, TrieResponseIncoming,
        },
        AutoClosingResponder, EffectBuilder,
    },
    types::NodeId,
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
    BlockGossiper(gossiper::Message<BlockV2>),
    /// Deploy gossiper component message.
    #[from]
    TransactionGossiper(gossiper::Message<Transaction>),
    #[from]
    FinalitySignatureGossiper(gossiper::Message<FinalitySignatureV2>),
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
    FinalitySignature(Box<FinalitySignatureV2>),
}

impl Payload for Message {
    #[inline]
    fn message_kind(&self) -> MessageKind {
        match self {
            Message::Consensus(_) => MessageKind::Consensus,
            Message::ConsensusRequest(_) => MessageKind::Consensus,
            Message::BlockGossiper(_) => MessageKind::BlockGossip,
            Message::TransactionGossiper(_) => MessageKind::TransactionGossip,
            Message::AddressGossiper(_) => MessageKind::AddressGossip,
            Message::GetRequest { tag, .. } | Message::GetResponse { tag, .. } => match tag {
                Tag::Transaction | Tag::LegacyDeploy => MessageKind::TransactionTransfer,
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
            Message::TransactionGossiper(_) => false,
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
    fn incoming_resource_estimate(&self, weights: &EstimatorWeights) -> u32 {
        match self {
            Message::Consensus(_) => weights.consensus,
            Message::ConsensusRequest(_) => weights.consensus,
            Message::BlockGossiper(_) => weights.block_gossip,
            Message::TransactionGossiper(_) => weights.transaction_gossip,
            Message::FinalitySignatureGossiper(_) => weights.finality_signature_gossip,
            Message::AddressGossiper(_) => weights.address_gossip,
            Message::GetRequest { tag, .. } => match tag {
                Tag::Transaction => weights.transaction_requests,
                Tag::LegacyDeploy => weights.legacy_deploy_requests,
                Tag::Block => weights.block_requests,
                Tag::BlockHeader => weights.block_header_requests,
                Tag::TrieOrChunk => weights.trie_requests,
                Tag::FinalitySignature => weights.finality_signature_requests,
                Tag::SyncLeap => weights.sync_leap_requests,
                Tag::ApprovalsHashes => weights.approvals_hashes_requests,
                Tag::BlockExecutionResults => weights.execution_results_requests,
            },
            Message::GetResponse { tag, .. } => match tag {
                Tag::Transaction => weights.transaction_responses,
                Tag::LegacyDeploy => weights.legacy_deploy_responses,
                Tag::Block => weights.block_responses,
                Tag::BlockHeader => weights.block_header_responses,
                Tag::TrieOrChunk => weights.trie_responses,
                Tag::FinalitySignature => weights.finality_signature_responses,
                Tag::SyncLeap => weights.sync_leap_responses,
                Tag::ApprovalsHashes => weights.approvals_hashes_responses,
                Tag::BlockExecutionResults => weights.execution_results_responses,
            },
            Message::FinalitySignature(_) => weights.finality_signature_broadcasts,
        }
    }

    fn is_unsafe_for_syncing_peers(&self) -> bool {
        match self {
            Message::Consensus(_) => false,
            Message::ConsensusRequest(_) => false,
            Message::BlockGossiper(_) => false,
            Message::TransactionGossiper(_) => false,
            Message::FinalitySignatureGossiper(_) => false,
            Message::AddressGossiper(_) => false,
            // Trie requests can deadlock between syncing nodes.
            Message::GetRequest { tag, .. } if *tag == Tag::TrieOrChunk => true,
            Message::GetRequest { .. } => false,
            Message::GetResponse { .. } => false,
            Message::FinalitySignature(_) => false,
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
            Message::TransactionGossiper(dg) => f.debug_tuple("DeployGossiper").field(&dg).finish(),
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
mod specimen_support {
    use crate::utils::specimen::{
        largest_get_request, largest_get_response, largest_variant, Cache, LargestSpecimen,
        SizeEstimator,
    };

    use super::{Message, MessageDiscriminants};

    impl LargestSpecimen for Message {
        fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
            largest_variant::<Self, MessageDiscriminants, _, _>(
                estimator,
                |variant| match variant {
                    MessageDiscriminants::Consensus => {
                        Message::Consensus(LargestSpecimen::largest_specimen(estimator, cache))
                    }
                    MessageDiscriminants::ConsensusRequest => Message::ConsensusRequest(
                        LargestSpecimen::largest_specimen(estimator, cache),
                    ),
                    MessageDiscriminants::BlockGossiper => {
                        Message::BlockGossiper(LargestSpecimen::largest_specimen(estimator, cache))
                    }
                    MessageDiscriminants::TransactionGossiper => Message::TransactionGossiper(
                        LargestSpecimen::largest_specimen(estimator, cache),
                    ),
                    MessageDiscriminants::FinalitySignatureGossiper => {
                        Message::FinalitySignatureGossiper(LargestSpecimen::largest_specimen(
                            estimator, cache,
                        ))
                    }
                    MessageDiscriminants::AddressGossiper => Message::AddressGossiper(
                        LargestSpecimen::largest_specimen(estimator, cache),
                    ),
                    MessageDiscriminants::GetRequest => largest_get_request(estimator, cache),
                    MessageDiscriminants::GetResponse => largest_get_response(estimator, cache),
                    MessageDiscriminants::FinalitySignature => Message::FinalitySignature(
                        LargestSpecimen::largest_specimen(estimator, cache),
                    ),
                },
            )
        }
    }
}

impl Display for Message {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Message::Consensus(consensus) => write!(f, "Consensus::{}", consensus),
            Message::ConsensusRequest(consensus) => write!(f, "ConsensusRequest({})", consensus),
            Message::BlockGossiper(deploy) => write!(f, "BlockGossiper::{}", deploy),
            Message::TransactionGossiper(txn) => write!(f, "TransactionGossiper::{}", txn),
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
        + From<GossiperIncoming<BlockV2>>
        + From<GossiperIncoming<Transaction>>
        + From<GossiperIncoming<FinalitySignatureV2>>
        + From<GossiperIncoming<GossipedAddress>>
        + From<NetRequestIncoming>
        + From<NetResponseIncoming>
        + From<TrieRequestIncoming>
        + From<TrieDemand>
        + From<TrieResponseIncoming>
        + From<FinalitySignatureIncoming>,
{
    fn from_incoming(sender: NodeId, payload: Message) -> Self {
        match payload {
            Message::Consensus(message) => ConsensusMessageIncoming {
                sender,
                message: Box::new(message),
            }
            .into(),
            Message::ConsensusRequest(_message) => {
                // TODO: Remove this once from_incoming and try_demand_from_incoming are unified.
                unreachable!("called from_incoming with a consensus request")
            }
            Message::BlockGossiper(message) => GossiperIncoming {
                sender,
                message: Box::new(message),
            }
            .into(),
            Message::TransactionGossiper(message) => GossiperIncoming {
                sender,
                message: Box::new(message),
            }
            .into(),
            Message::FinalitySignatureGossiper(message) => GossiperIncoming {
                sender,
                message: Box::new(message),
            }
            .into(),
            Message::AddressGossiper(message) => GossiperIncoming {
                sender,
                message: Box::new(message),
            }
            .into(),
            Message::GetRequest { tag, serialized_id } => match tag {
                Tag::Transaction => NetRequestIncoming {
                    sender,
                    message: Box::new(NetRequest::Transaction(serialized_id)),
                }
                .into(),
                Tag::LegacyDeploy => NetRequestIncoming {
                    sender,
                    message: Box::new(NetRequest::LegacyDeploy(serialized_id)),
                }
                .into(),
                Tag::Block => NetRequestIncoming {
                    sender,
                    message: Box::new(NetRequest::Block(serialized_id)),
                }
                .into(),
                Tag::BlockHeader => NetRequestIncoming {
                    sender,
                    message: Box::new(NetRequest::BlockHeader(serialized_id)),
                }
                .into(),
                Tag::TrieOrChunk => TrieRequestIncoming {
                    sender,
                    message: Box::new(TrieRequest(serialized_id)),
                }
                .into(),
                Tag::FinalitySignature => NetRequestIncoming {
                    sender,
                    message: Box::new(NetRequest::FinalitySignature(serialized_id)),
                }
                .into(),
                Tag::SyncLeap => NetRequestIncoming {
                    sender,
                    message: Box::new(NetRequest::SyncLeap(serialized_id)),
                }
                .into(),
                Tag::ApprovalsHashes => NetRequestIncoming {
                    sender,
                    message: Box::new(NetRequest::ApprovalsHashes(serialized_id)),
                }
                .into(),
                Tag::BlockExecutionResults => NetRequestIncoming {
                    sender,
                    message: Box::new(NetRequest::BlockExecutionResults(serialized_id)),
                }
                .into(),
            },
            Message::GetResponse {
                tag,
                serialized_item,
            } => match tag {
                Tag::Transaction => NetResponseIncoming {
                    sender,
                    message: Box::new(NetResponse::Transaction(serialized_item)),
                }
                .into(),
                Tag::LegacyDeploy => NetResponseIncoming {
                    sender,
                    message: Box::new(NetResponse::LegacyDeploy(serialized_item)),
                }
                .into(),
                Tag::Block => NetResponseIncoming {
                    sender,
                    message: Box::new(NetResponse::Block(serialized_item)),
                }
                .into(),
                Tag::BlockHeader => NetResponseIncoming {
                    sender,
                    message: Box::new(NetResponse::BlockHeader(serialized_item)),
                }
                .into(),
                Tag::TrieOrChunk => TrieResponseIncoming {
                    sender,
                    message: Box::new(TrieResponse(serialized_item.to_vec())),
                }
                .into(),
                Tag::FinalitySignature => NetResponseIncoming {
                    sender,
                    message: Box::new(NetResponse::FinalitySignature(serialized_item)),
                }
                .into(),
                Tag::SyncLeap => NetResponseIncoming {
                    sender,
                    message: Box::new(NetResponse::SyncLeap(serialized_item)),
                }
                .into(),
                Tag::ApprovalsHashes => NetResponseIncoming {
                    sender,
                    message: Box::new(NetResponse::ApprovalsHashes(serialized_item)),
                }
                .into(),
                Tag::BlockExecutionResults => NetResponseIncoming {
                    sender,
                    message: Box::new(NetResponse::BlockExecutionResults(serialized_item)),
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
