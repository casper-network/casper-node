use std::fmt::{self, Debug, Display, Formatter};

use derive_more::From;
use serde::Serialize;

use crate::{
    components::{
        block_proposer, block_validator, blocks_accumulator, chain_synchronizer,
        complete_block_synchronizer::{self, CompleteBlockSyncRequest},
        consensus, contract_runtime, deploy_acceptor, diagnostics_port, event_stream_server,
        fetcher, gossiper, linear_chain, rest_server, rpc_server,
        small_network::{self, GossipedAddress},
        storage, upgrade_watcher,
    },
    effect::{
        announcements::{
            BlockProposerAnnouncement, BlocklistAnnouncement, ChainSynchronizerAnnouncement,
            ConsensusAnnouncement, ContractRuntimeAnnouncement, ControlAnnouncement,
            DeployAcceptorAnnouncement, GossiperAnnouncement, LinearChainAnnouncement,
            RpcServerAnnouncement, UpgradeWatcherAnnouncement,
        },
        diagnostics_port::DumpConsensusStateRequest,
        incoming::{
            BlockAddedRequestIncoming, BlockAddedResponseIncoming, ConsensusMessageIncoming,
            FinalitySignatureIncoming, GossiperIncoming, NetRequestIncoming, NetResponseIncoming,
            SyncLeapRequestIncoming, SyncLeapResponseIncoming, TrieDemand, TrieRequestIncoming,
            TrieResponseIncoming,
        },
        requests::{
            BeginGossipRequest, BlockProposerRequest, BlockValidationRequest,
            ChainspecRawBytesRequest, ConsensusRequest, ContractRuntimeRequest, FetcherRequest,
            MarkBlockCompletedRequest, MetricsRequest, NetworkInfoRequest, NetworkRequest,
            NodeStateRequest, RestRequest, RpcRequest, StateStoreRequest, StorageRequest,
            UpgradeWatcherRequest,
        },
    },
    protocol::Message,
    reactor::ReactorEvent,
    types::{
        Block, BlockAdded, BlockAndDeploys, BlockHeader, BlockHeaderWithMetadata,
        BlockHeadersBatch, BlockSignatures, BlockWithMetadata, Deploy, FinalitySignature,
        FinalizedApprovalsWithId, SyncLeap, TrieOrChunk,
    },
};

// timing belt event
// CATCHING_UP
// match event {
//     ParticipatingEvent::Initialize(..) => {
//         // ctor puts into status and we handle pushing various init events here
//         self.storage.is_init() -> state / or effects if it requires work to be done
//     }
//     ParticipatingEvent::GetCaughtUp(..) => {
//         <- keep on catching up
//         > sync leap then have convo w accumul OR
//         <- transition to keeping up
//         <- fatal
//     }
//     ParticipatingEvent::StayCaughtUp(..) => {
//         <- keep on keeping up
//         <- maybe enuff cycles to attempt 1 sync block
//         <- get caught up
//         <- fatal
//     }
// }

/// Top-level event for the reactor.
#[derive(Debug, From, Serialize)]
#[must_use]
// Note: The large enum size must be reigned in eventually. This is a stopgap for now.
#[allow(clippy::large_enum_variant)]
pub(crate) enum ParticipatingEvent {
    // Control logic == Reactor self events
    // Shutdown the reactor, should only be raised by the reactor itself
    Shutdown(String),
    // Check the status of the reactor, should only be raised by the reactor itself
    CheckStatus,
    #[from]
    ChainspecRawBytesRequest(#[serde(skip_serializing)] ChainspecRawBytesRequest),

    // Coordination events == component to component(s) or component to reactor events
    #[from]
    ChainSynchronizer(chain_synchronizer::Event),
    #[from]
    SmallNetwork(small_network::Event<Message>),
    #[from]
    Storage(storage::Event),
    #[from]
    BlockProposer(#[serde(skip_serializing)] block_proposer::Event),
    #[from]
    RpcServer(#[serde(skip_serializing)] rpc_server::Event),
    #[from]
    RestServer(#[serde(skip_serializing)] rest_server::Event),
    #[from]
    EventStreamServer(#[serde(skip_serializing)] event_stream_server::Event),
    #[from]
    UpgradeWatcher(#[serde(skip_serializing)] upgrade_watcher::Event),
    #[from]
    Consensus(#[serde(skip_serializing)] consensus::Event),
    #[from]
    DeployAcceptor(#[serde(skip_serializing)] deploy_acceptor::Event),
    #[from]
    DeployFetcher(#[serde(skip_serializing)] fetcher::Event<Deploy>),
    #[from]
    DeployGossiper(#[serde(skip_serializing)] gossiper::Event<Deploy>),
    #[from]
    BlockAddedGossiper(#[serde(skip_serializing)] gossiper::Event<BlockAdded>),
    #[from]
    FinalitySignatureGossiper(#[serde(skip_serializing)] gossiper::Event<FinalitySignature>),
    #[from]
    AddressGossiper(gossiper::Event<GossipedAddress>),
    #[from]
    BlockValidator(#[serde(skip_serializing)] block_validator::Event),
    #[from]
    LinearChain(#[serde(skip_serializing)] linear_chain::Event),
    #[from]
    DiagnosticsPort(diagnostics_port::Event),
    #[from]
    ContractRuntime(contract_runtime::Event),
    #[from]
    BlockFetcher(#[serde(skip_serializing)] fetcher::Event<Block>),
    #[from]
    BlockHeaderFetcher(#[serde(skip_serializing)] fetcher::Event<BlockHeader>),
    #[from]
    TrieOrChunkFetcher(#[serde(skip_serializing)] fetcher::Event<TrieOrChunk>),
    #[from]
    BlockByHeightFetcher(#[serde(skip_serializing)] fetcher::Event<BlockWithMetadata>),
    #[from]
    BlockHeaderByHeightFetcher(#[serde(skip_serializing)] fetcher::Event<BlockHeaderWithMetadata>),
    #[from]
    BlockAndDeploysFetcher(#[serde(skip_serializing)] fetcher::Event<BlockAndDeploys>),
    #[from]
    FinalizedApprovalsFetcher(#[serde(skip_serializing)] fetcher::Event<FinalizedApprovalsWithId>),
    #[from]
    FinalitySignatureFetcher(#[serde(skip_serializing)] fetcher::Event<FinalitySignature>),
    #[from]
    BlockHeadersBatchFetcher(#[serde(skip_serializing)] fetcher::Event<BlockHeadersBatch>),
    #[from]
    FinalitySignaturesFetcher(#[serde(skip_serializing)] fetcher::Event<BlockSignatures>),
    #[from]
    SyncLeapFetcher(#[serde(skip_serializing)] fetcher::Event<SyncLeap>),
    #[from]
    BlockAddedFetcher(#[serde(skip_serializing)] fetcher::Event<BlockAdded>),
    #[from]
    BlocksAccumulator(#[serde(skip_serializing)] blocks_accumulator::Event),
    #[from]
    CompleteBlockSynchronizer(#[serde(skip_serializing)] complete_block_synchronizer::Event),

    // Requests
    #[from]
    ChainSynchronizerRequest(#[serde(skip_serializing)] NodeStateRequest),
    #[from]
    ContractRuntimeRequest(ContractRuntimeRequest),
    #[from]
    NetworkRequest(#[serde(skip_serializing)] NetworkRequest<Message>),
    #[from]
    NetworkInfoRequest(#[serde(skip_serializing)] NetworkInfoRequest),
    #[from]
    BlockFetcherRequest(#[serde(skip_serializing)] FetcherRequest<Block>),
    #[from]
    BlockHeaderFetcherRequest(#[serde(skip_serializing)] FetcherRequest<BlockHeader>),
    #[from]
    TrieOrChunkFetcherRequest(#[serde(skip_serializing)] FetcherRequest<TrieOrChunk>),
    #[from]
    BlockByHeightFetcherRequest(#[serde(skip_serializing)] FetcherRequest<BlockWithMetadata>),
    #[from]
    BlockHeaderByHeightFetcherRequest(
        #[serde(skip_serializing)] FetcherRequest<BlockHeaderWithMetadata>,
    ),
    #[from]
    BlockAndDeploysFetcherRequest(#[serde(skip_serializing)] FetcherRequest<BlockAndDeploys>),
    #[from]
    DeployFetcherRequest(#[serde(skip_serializing)] FetcherRequest<Deploy>),
    #[from]
    FinalizedApprovalsFetcherRequest(
        #[serde(skip_serializing)] FetcherRequest<FinalizedApprovalsWithId>,
    ),
    #[from]
    FinalitySignatureFetcherRequest(#[serde(skip_serializing)] FetcherRequest<FinalitySignature>),
    #[from]
    BlockHeadersBatchFetcherRequest(#[serde(skip_serializing)] FetcherRequest<BlockHeadersBatch>),
    #[from]
    FinalitySignaturesFetcherRequest(#[serde(skip_serializing)] FetcherRequest<BlockSignatures>),
    #[from]
    SyncLeapFetcherRequest(#[serde(skip_serializing)] FetcherRequest<SyncLeap>),
    #[from]
    BlockAddedFetcherRequest(#[serde(skip_serializing)] FetcherRequest<BlockAdded>),

    #[from]
    BlockProposerRequest(#[serde(skip_serializing)] BlockProposerRequest),
    #[from]
    BlockValidatorRequest(#[serde(skip_serializing)] BlockValidationRequest),
    #[from]
    MetricsRequest(#[serde(skip_serializing)] MetricsRequest),
    #[from]
    UpgradeWatcherRequest(#[serde(skip_serializing)] UpgradeWatcherRequest),
    #[from]
    StorageRequest(#[serde(skip_serializing)] StorageRequest),
    #[from]
    MarkBlockCompletedRequest(MarkBlockCompletedRequest),
    #[from]
    BeginAddressGossipRequest(BeginGossipRequest<GossipedAddress>),
    #[from]
    StateStoreRequest(StateStoreRequest),
    #[from]
    DumpConsensusStateRequest(DumpConsensusStateRequest),
    #[from]
    CompleteBlockSynchronizerRequest(#[serde(skip_serializing)] CompleteBlockSyncRequest),

    // Announcements
    #[from]
    ControlAnnouncement(ControlAnnouncement),
    #[from]
    RpcServerAnnouncement(#[serde(skip_serializing)] RpcServerAnnouncement),
    #[from]
    DeployAcceptorAnnouncement(#[serde(skip_serializing)] DeployAcceptorAnnouncement),
    #[from]
    ConsensusAnnouncement(#[serde(skip_serializing)] ConsensusAnnouncement),
    #[from]
    ContractRuntimeAnnouncement(#[serde(skip_serializing)] ContractRuntimeAnnouncement),
    #[from]
    DeployGossiperAnnouncement(#[serde(skip_serializing)] GossiperAnnouncement<Deploy>),
    #[from]
    BlockAddedGossiperAnnouncement(#[serde(skip_serializing)] GossiperAnnouncement<BlockAdded>),
    #[from]
    FinalitySignatureGossiperAnnouncement(
        #[serde(skip_serializing)] GossiperAnnouncement<FinalitySignature>,
    ),
    #[from]
    AddressGossiperAnnouncement(#[serde(skip_serializing)] GossiperAnnouncement<GossipedAddress>),
    #[from]
    LinearChainAnnouncement(#[serde(skip_serializing)] LinearChainAnnouncement),
    #[from]
    UpgradeWatcherAnnouncement(#[serde(skip_serializing)] UpgradeWatcherAnnouncement),
    #[from]
    ChainSynchronizerAnnouncement(#[serde(skip_serializing)] ChainSynchronizerAnnouncement),
    #[from]
    BlocklistAnnouncement(BlocklistAnnouncement),
    #[from]
    ConsensusMessageIncoming(ConsensusMessageIncoming),
    #[from]
    DeployGossiperIncoming(GossiperIncoming<Deploy>),
    #[from]
    BlockAddedGossiperIncoming(GossiperIncoming<BlockAdded>),
    #[from]
    FinalitySignatureGossiperIncoming(GossiperIncoming<FinalitySignature>),
    #[from]
    AddressGossiperIncoming(GossiperIncoming<GossipedAddress>),
    #[from]
    NetRequestIncoming(NetRequestIncoming),
    #[from]
    NetResponseIncoming(NetResponseIncoming),
    #[from]
    TrieRequestIncoming(TrieRequestIncoming),
    #[from]
    TrieDemand(TrieDemand),
    #[from]
    TrieResponseIncoming(TrieResponseIncoming),
    #[from]
    SyncLeapRequestIncoming(SyncLeapRequestIncoming),
    #[from]
    SyncLeapResponseIncoming(SyncLeapResponseIncoming),
    #[from]
    BlockAddedRequestIncoming(BlockAddedRequestIncoming),
    #[from]
    BlockAddedResponseIncoming(BlockAddedResponseIncoming),
    #[from]
    FinalitySignatureIncoming(FinalitySignatureIncoming),
    #[from]
    BlockProposerAnnouncement(#[serde(skip_serializing)] BlockProposerAnnouncement),
}

impl ReactorEvent for ParticipatingEvent {
    fn as_control(&self) -> Option<&ControlAnnouncement> {
        if let Self::ControlAnnouncement(ref ctrl_ann) = self {
            Some(ctrl_ann)
        } else {
            None
        }
    }

    fn try_into_control(self) -> Option<ControlAnnouncement> {
        if let Self::ControlAnnouncement(ctrl_ann) = self {
            Some(ctrl_ann)
        } else {
            None
        }
    }

    #[inline]
    fn description(&self) -> &'static str {
        match self {
            ParticipatingEvent::Shutdown(_) => "Shutdown",
            ParticipatingEvent::CheckStatus => "CheckStatus",
            ParticipatingEvent::ChainSynchronizer(_) => "ChainSynchronizer",
            ParticipatingEvent::SmallNetwork(_) => "SmallNetwork",
            ParticipatingEvent::BlockProposer(_) => "BlockProposer",
            ParticipatingEvent::Storage(_) => "Storage",
            ParticipatingEvent::RpcServer(_) => "RpcServer",
            ParticipatingEvent::RestServer(_) => "RestServer",
            ParticipatingEvent::EventStreamServer(_) => "EventStreamServer",
            ParticipatingEvent::UpgradeWatcher(_) => "UpgradeWatcher",
            ParticipatingEvent::Consensus(_) => "Consensus",
            ParticipatingEvent::DeployAcceptor(_) => "DeployAcceptor",
            ParticipatingEvent::DeployFetcher(_) => "DeployFetcher",
            ParticipatingEvent::DeployGossiper(_) => "DeployGossiper",
            ParticipatingEvent::BlockAddedGossiper(_) => "BlockGossiper",
            ParticipatingEvent::FinalitySignatureGossiper(_) => "FinalitySignatureGossiper",
            ParticipatingEvent::AddressGossiper(_) => "AddressGossiper",
            ParticipatingEvent::BlockValidator(_) => "BlockValidator",
            ParticipatingEvent::LinearChain(_) => "LinearChain",
            ParticipatingEvent::ContractRuntimeRequest(_) => "ContractRuntimeRequest",
            ParticipatingEvent::ChainSynchronizerRequest(_) => "ChainSynchronizerRequest",
            ParticipatingEvent::BlockFetcher(_) => "BlockFetcher",
            ParticipatingEvent::BlockHeaderFetcher(_) => "BlockHeaderFetcher",
            ParticipatingEvent::TrieOrChunkFetcher(_) => "TrieOrChunkFetcher",
            ParticipatingEvent::BlockByHeightFetcher(_) => "BlockByHeightFetcher",
            ParticipatingEvent::BlockHeaderByHeightFetcher(_) => "BlockHeaderByHeightFetcher",
            ParticipatingEvent::BlockAndDeploysFetcher(_) => "BlockAndDeploysFetcher",
            ParticipatingEvent::FinalizedApprovalsFetcher(_) => "FinalizedApprovalsFetcher",
            ParticipatingEvent::FinalitySignatureFetcher(_) => "FinalitySignatureFetcher",
            ParticipatingEvent::BlockHeadersBatchFetcher(_) => "BlockHeadersBatchFetcher",
            ParticipatingEvent::FinalitySignaturesFetcher(_) => "FinalitySignaturesFetcher",
            ParticipatingEvent::SyncLeapFetcher(_) => "SyncLeapFetcher",
            ParticipatingEvent::BlockAddedFetcher(_) => "BlockAddedFetcher",
            ParticipatingEvent::DiagnosticsPort(_) => "DiagnosticsPort",
            ParticipatingEvent::NetworkRequest(_) => "NetworkRequest",
            ParticipatingEvent::NetworkInfoRequest(_) => "NetworkInfoRequest",
            ParticipatingEvent::BlockFetcherRequest(_) => "BlockFetcherRequest",
            ParticipatingEvent::BlockHeaderFetcherRequest(_) => "BlockHeaderFetcherRequest",
            ParticipatingEvent::TrieOrChunkFetcherRequest(_) => "TrieOrChunkFetcherRequest",
            ParticipatingEvent::BlockByHeightFetcherRequest(_) => "BlockByHeightFetcherRequest",
            ParticipatingEvent::BlockHeaderByHeightFetcherRequest(_) => {
                "BlockHeaderByHeightFetcherRequest"
            }
            ParticipatingEvent::BlockAndDeploysFetcherRequest(_) => "BlockAndDeploysFetcherRequest",
            ParticipatingEvent::DeployFetcherRequest(_) => "DeployFetcherRequest",
            ParticipatingEvent::FinalizedApprovalsFetcherRequest(_) => {
                "FinalizedApprovalsFetcherRequest"
            }
            ParticipatingEvent::FinalitySignatureFetcherRequest(_) => {
                "FinalitySignatureFetcherRequest"
            }
            ParticipatingEvent::BlockHeadersBatchFetcherRequest(_) => {
                "BlockHeadersBatchFetcherRequest"
            }
            ParticipatingEvent::FinalitySignaturesFetcherRequest(_) => {
                "FinalitySignaturesFetcherRequest"
            }
            ParticipatingEvent::SyncLeapFetcherRequest(_) => "SyncLeapFetcherRequest",
            ParticipatingEvent::BlockAddedFetcherRequest(_) => "BlockAddedFetcherRequest",
            ParticipatingEvent::BlockProposerRequest(_) => "BlockProposerRequest",
            ParticipatingEvent::BlockValidatorRequest(_) => "BlockValidatorRequest",
            ParticipatingEvent::MetricsRequest(_) => "MetricsRequest",
            ParticipatingEvent::ChainspecRawBytesRequest(_) => "ChainspecRawBytesRequest",
            ParticipatingEvent::UpgradeWatcherRequest(_) => "UpgradeWatcherRequest",
            ParticipatingEvent::StorageRequest(_) => "StorageRequest",
            ParticipatingEvent::MarkBlockCompletedRequest(_) => "MarkBlockCompletedRequest",
            ParticipatingEvent::StateStoreRequest(_) => "StateStoreRequest",
            ParticipatingEvent::DumpConsensusStateRequest(_) => "DumpConsensusStateRequest",
            ParticipatingEvent::ControlAnnouncement(_) => "ControlAnnouncement",
            ParticipatingEvent::RpcServerAnnouncement(_) => "RpcServerAnnouncement",
            ParticipatingEvent::DeployAcceptorAnnouncement(_) => "DeployAcceptorAnnouncement",
            ParticipatingEvent::ConsensusAnnouncement(_) => "ConsensusAnnouncement",
            ParticipatingEvent::ContractRuntimeAnnouncement(_) => "ContractRuntimeAnnouncement",
            ParticipatingEvent::DeployGossiperAnnouncement(_) => "DeployGossiperAnnouncement",
            ParticipatingEvent::AddressGossiperAnnouncement(_) => "AddressGossiperAnnouncement",
            ParticipatingEvent::LinearChainAnnouncement(_) => "LinearChainAnnouncement",
            ParticipatingEvent::UpgradeWatcherAnnouncement(_) => "UpgradeWatcherAnnouncement",
            ParticipatingEvent::BlocklistAnnouncement(_) => "BlocklistAnnouncement",
            ParticipatingEvent::BlockProposerAnnouncement(_) => "BlockProposerAnnouncement",
            ParticipatingEvent::BeginAddressGossipRequest(_) => "BeginAddressGossipRequest",
            ParticipatingEvent::ConsensusMessageIncoming(_) => "ConsensusMessageIncoming",
            ParticipatingEvent::DeployGossiperIncoming(_) => "DeployGossiperIncoming",
            ParticipatingEvent::BlockAddedGossiperIncoming(_) => "BlockGossiperIncoming",
            ParticipatingEvent::FinalitySignatureGossiperIncoming(_) => {
                "FinalitySignatureGossiperIncoming"
            }
            ParticipatingEvent::AddressGossiperIncoming(_) => "AddressGossiperIncoming",
            ParticipatingEvent::NetRequestIncoming(_) => "NetRequestIncoming",
            ParticipatingEvent::NetResponseIncoming(_) => "NetResponseIncoming",
            ParticipatingEvent::TrieRequestIncoming(_) => "TrieRequestIncoming",
            ParticipatingEvent::TrieDemand(_) => "TrieDemand",
            ParticipatingEvent::TrieResponseIncoming(_) => "TrieResponseIncoming",
            ParticipatingEvent::SyncLeapRequestIncoming(_) => "SyncLeapRequestIncoming",
            ParticipatingEvent::SyncLeapResponseIncoming(_) => "SyncLeapResponseIncoming",
            ParticipatingEvent::BlockAddedRequestIncoming(_) => "BlockAddedRequestIncoming",
            ParticipatingEvent::BlockAddedResponseIncoming(_) => "BlockAddedResponseIncoming",
            ParticipatingEvent::FinalitySignatureIncoming(_) => "FinalitySignatureIncoming",
            ParticipatingEvent::ContractRuntime(_) => "ContractRuntime",
            ParticipatingEvent::ChainSynchronizerAnnouncement(_) => "ChainSynchronizerAnnouncement",
            ParticipatingEvent::BlockAddedGossiperAnnouncement(_) => "BlockGossiperAnnouncement",
            ParticipatingEvent::FinalitySignatureGossiperAnnouncement(_) => {
                "FinalitySignatureGossiperAnnouncement"
            }
            ParticipatingEvent::BlocksAccumulator(_) => "BlocksAccumulator",
            ParticipatingEvent::CompleteBlockSynchronizer(_) => "CompleteBlockSynchronizer",
            ParticipatingEvent::CompleteBlockSynchronizerRequest(_) => {
                "CompleteBlockSynchronizerRequest"
            }
        }
    }
}

impl From<RpcRequest> for ParticipatingEvent {
    fn from(request: RpcRequest) -> Self {
        ParticipatingEvent::RpcServer(rpc_server::Event::RpcRequest(request))
    }
}

impl From<RestRequest> for ParticipatingEvent {
    fn from(request: RestRequest) -> Self {
        ParticipatingEvent::RestServer(rest_server::Event::RestRequest(request))
    }
}

impl From<NetworkRequest<consensus::ConsensusMessage>> for ParticipatingEvent {
    fn from(request: NetworkRequest<consensus::ConsensusMessage>) -> Self {
        ParticipatingEvent::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<NetworkRequest<gossiper::Message<Deploy>>> for ParticipatingEvent {
    fn from(request: NetworkRequest<gossiper::Message<Deploy>>) -> Self {
        ParticipatingEvent::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<NetworkRequest<gossiper::Message<BlockAdded>>> for ParticipatingEvent {
    fn from(request: NetworkRequest<gossiper::Message<BlockAdded>>) -> Self {
        ParticipatingEvent::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<NetworkRequest<gossiper::Message<FinalitySignature>>> for ParticipatingEvent {
    fn from(request: NetworkRequest<gossiper::Message<FinalitySignature>>) -> Self {
        ParticipatingEvent::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<NetworkRequest<gossiper::Message<GossipedAddress>>> for ParticipatingEvent {
    fn from(request: NetworkRequest<gossiper::Message<GossipedAddress>>) -> Self {
        ParticipatingEvent::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<ConsensusRequest> for ParticipatingEvent {
    fn from(request: ConsensusRequest) -> Self {
        ParticipatingEvent::Consensus(consensus::Event::ConsensusRequest(request))
    }
}

impl Display for ParticipatingEvent {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ParticipatingEvent::Shutdown(msg) => write!(f, "shutdown: {}", msg),
            ParticipatingEvent::CheckStatus => write!(f, "check status"),
            ParticipatingEvent::ChainSynchronizer(event) => {
                write!(f, "chain synchronizer: {}", event)
            }
            ParticipatingEvent::Storage(event) => write!(f, "storage: {}", event),
            ParticipatingEvent::SmallNetwork(event) => write!(f, "small network: {}", event),
            ParticipatingEvent::BlockProposer(event) => write!(f, "block proposer: {}", event),
            ParticipatingEvent::RpcServer(event) => write!(f, "rpc server: {}", event),
            ParticipatingEvent::RestServer(event) => write!(f, "rest server: {}", event),
            ParticipatingEvent::EventStreamServer(event) => {
                write!(f, "event stream server: {}", event)
            }
            ParticipatingEvent::UpgradeWatcher(event) => write!(f, "upgrade watcher: {}", event),
            ParticipatingEvent::Consensus(event) => write!(f, "consensus: {}", event),
            ParticipatingEvent::DeployAcceptor(event) => write!(f, "deploy acceptor: {}", event),
            ParticipatingEvent::DeployFetcher(event) => write!(f, "deploy fetcher: {}", event),
            ParticipatingEvent::DeployGossiper(event) => write!(f, "deploy gossiper: {}", event),
            ParticipatingEvent::BlockAddedGossiper(event) => write!(f, "block gossiper: {}", event),
            ParticipatingEvent::FinalitySignatureGossiper(event) => {
                write!(f, "block signature gossiper: {}", event)
            }
            ParticipatingEvent::AddressGossiper(event) => write!(f, "address gossiper: {}", event),
            ParticipatingEvent::ContractRuntimeRequest(event) => {
                write!(f, "contract runtime request: {:?}", event)
            }
            ParticipatingEvent::LinearChain(event) => write!(f, "linear-chain event {}", event),
            ParticipatingEvent::BlockValidator(event) => write!(f, "block validator: {}", event),
            ParticipatingEvent::BlockFetcher(event) => write!(f, "block fetcher: {}", event),
            ParticipatingEvent::BlockHeaderFetcher(event) => {
                write!(f, "block header fetcher: {}", event)
            }
            ParticipatingEvent::TrieOrChunkFetcher(event) => {
                write!(f, "trie or chunk fetcher: {}", event)
            }
            ParticipatingEvent::BlockByHeightFetcher(event) => {
                write!(f, "block by height fetcher: {}", event)
            }
            ParticipatingEvent::BlockHeaderByHeightFetcher(event) => {
                write!(f, "block header by height fetcher: {}", event)
            }
            ParticipatingEvent::BlockAndDeploysFetcher(event) => {
                write!(f, "block and deploys fetcher: {}", event)
            }
            ParticipatingEvent::FinalizedApprovalsFetcher(event) => {
                write!(f, "finalized approvals fetcher: {}", event)
            }
            ParticipatingEvent::FinalitySignatureFetcher(event) => {
                write!(f, "finality signature fetcher: {}", event)
            }
            ParticipatingEvent::BlockHeadersBatchFetcher(event) => {
                write!(f, "block headers batch fetcher: {}", event)
            }
            ParticipatingEvent::FinalitySignaturesFetcher(event) => {
                write!(f, "finality signatures fetcher: {}", event)
            }
            ParticipatingEvent::SyncLeapFetcher(event) => {
                write!(f, "sync leap fetcher: {}", event)
            }
            ParticipatingEvent::BlockAddedFetcher(event) => {
                write!(f, "block added fetcher: {}", event)
            }
            ParticipatingEvent::BlocksAccumulator(event) => {
                write!(f, "blocks accumulator: {}", event)
            }
            ParticipatingEvent::CompleteBlockSynchronizer(event) => {
                write!(f, "complete block synchronizer: {}", event)
            }
            ParticipatingEvent::DiagnosticsPort(event) => write!(f, "diagnostics port: {}", event),
            ParticipatingEvent::ChainSynchronizerRequest(req) => {
                write!(f, "chain synchronizer request: {}", req)
            }
            ParticipatingEvent::NetworkRequest(req) => write!(f, "network request: {}", req),
            ParticipatingEvent::NetworkInfoRequest(req) => {
                write!(f, "network info request: {}", req)
            }
            ParticipatingEvent::ChainspecRawBytesRequest(req) => {
                write!(f, "chainspec loader request: {}", req)
            }
            ParticipatingEvent::UpgradeWatcherRequest(req) => {
                write!(f, "upgrade watcher request: {}", req)
            }
            ParticipatingEvent::StorageRequest(req) => write!(f, "storage request: {}", req),
            ParticipatingEvent::MarkBlockCompletedRequest(req) => {
                write!(f, "mark block completed request: {}", req)
            }
            ParticipatingEvent::StateStoreRequest(req) => write!(f, "state store request: {}", req),
            ParticipatingEvent::BlockFetcherRequest(request) => {
                write!(f, "block fetcher request: {}", request)
            }
            ParticipatingEvent::BlockHeaderFetcherRequest(request) => {
                write!(f, "block header fetcher request: {}", request)
            }
            ParticipatingEvent::TrieOrChunkFetcherRequest(request) => {
                write!(f, "trie or chunk fetcher request: {}", request)
            }
            ParticipatingEvent::BlockByHeightFetcherRequest(request) => {
                write!(f, "block by height fetcher request: {}", request)
            }
            ParticipatingEvent::BlockHeaderByHeightFetcherRequest(request) => {
                write!(f, "block header by height fetcher request: {}", request)
            }
            ParticipatingEvent::BlockAndDeploysFetcherRequest(request) => {
                write!(f, "block and deploys fetcher request: {}", request)
            }
            ParticipatingEvent::DeployFetcherRequest(request) => {
                write!(f, "deploy fetcher request: {}", request)
            }
            ParticipatingEvent::FinalizedApprovalsFetcherRequest(request) => {
                write!(f, "finalized approvals fetcher request: {}", request)
            }
            ParticipatingEvent::FinalitySignatureFetcherRequest(request) => {
                write!(f, "finality signature fetcher request: {}", request)
            }
            ParticipatingEvent::BlockHeadersBatchFetcherRequest(request) => {
                write!(f, "block headers batch fetcher request: {}", request)
            }
            ParticipatingEvent::FinalitySignaturesFetcherRequest(request) => {
                write!(f, "finality signatures fetcher request: {}", request)
            }
            ParticipatingEvent::SyncLeapFetcherRequest(request) => {
                write!(f, "sync leap fetcher request: {}", request)
            }
            ParticipatingEvent::BlockAddedFetcherRequest(request) => {
                write!(f, "block added fetcher request: {}", request)
            }
            ParticipatingEvent::BeginAddressGossipRequest(request) => {
                write!(f, "begin address gossip request: {}", request)
            }
            ParticipatingEvent::BlockProposerRequest(req) => {
                write!(f, "block proposer request: {}", req)
            }
            ParticipatingEvent::BlockValidatorRequest(req) => {
                write!(f, "block validator request: {}", req)
            }
            ParticipatingEvent::MetricsRequest(req) => write!(f, "metrics request: {}", req),
            ParticipatingEvent::CompleteBlockSynchronizerRequest(req) => {
                write!(f, "complete block synchronizer request: {}", req)
            }
            ParticipatingEvent::ControlAnnouncement(ctrl_ann) => write!(f, "control: {}", ctrl_ann),
            ParticipatingEvent::DumpConsensusStateRequest(req) => {
                write!(f, "dump consensus state: {}", req)
            }
            ParticipatingEvent::RpcServerAnnouncement(ann) => {
                write!(f, "api server announcement: {}", ann)
            }
            ParticipatingEvent::DeployAcceptorAnnouncement(ann) => {
                write!(f, "deploy acceptor announcement: {}", ann)
            }
            ParticipatingEvent::ConsensusAnnouncement(ann) => {
                write!(f, "consensus announcement: {}", ann)
            }
            ParticipatingEvent::ContractRuntimeAnnouncement(ann) => {
                write!(f, "block-executor announcement: {}", ann)
            }
            ParticipatingEvent::DeployGossiperAnnouncement(ann) => {
                write!(f, "deploy gossiper announcement: {}", ann)
            }
            ParticipatingEvent::BlockAddedGossiperAnnouncement(ann) => {
                write!(f, "block gossiper announcement: {}", ann)
            }
            ParticipatingEvent::FinalitySignatureGossiperAnnouncement(ann) => {
                write!(f, "block signature gossiper announcement: {}", ann)
            }
            ParticipatingEvent::AddressGossiperAnnouncement(ann) => {
                write!(f, "address gossiper announcement: {}", ann)
            }
            ParticipatingEvent::LinearChainAnnouncement(ann) => {
                write!(f, "linear chain announcement: {}", ann)
            }
            ParticipatingEvent::BlockProposerAnnouncement(ann) => {
                write!(f, "block proposer announcement: {}", ann)
            }
            ParticipatingEvent::UpgradeWatcherAnnouncement(ann) => {
                write!(f, "chainspec loader announcement: {}", ann)
            }
            ParticipatingEvent::BlocklistAnnouncement(ann) => {
                write!(f, "blocklist announcement: {}", ann)
            }
            ParticipatingEvent::ChainSynchronizerAnnouncement(ann) => {
                write!(f, "chain synchronizer announcement: {}", ann)
            }
            ParticipatingEvent::ConsensusMessageIncoming(inner) => Display::fmt(inner, f),
            ParticipatingEvent::DeployGossiperIncoming(inner) => Display::fmt(inner, f),
            ParticipatingEvent::BlockAddedGossiperIncoming(inner) => Display::fmt(inner, f),
            ParticipatingEvent::FinalitySignatureGossiperIncoming(inner) => Display::fmt(inner, f),
            ParticipatingEvent::AddressGossiperIncoming(inner) => Display::fmt(inner, f),
            ParticipatingEvent::NetRequestIncoming(inner) => Display::fmt(inner, f),
            ParticipatingEvent::NetResponseIncoming(inner) => Display::fmt(inner, f),
            ParticipatingEvent::TrieRequestIncoming(inner) => Display::fmt(inner, f),
            ParticipatingEvent::TrieDemand(inner) => Display::fmt(inner, f),
            ParticipatingEvent::TrieResponseIncoming(inner) => Display::fmt(inner, f),
            ParticipatingEvent::SyncLeapRequestIncoming(inner) => Display::fmt(inner, f),
            ParticipatingEvent::SyncLeapResponseIncoming(inner) => Display::fmt(inner, f),
            ParticipatingEvent::BlockAddedRequestIncoming(inner) => Display::fmt(inner, f),
            ParticipatingEvent::BlockAddedResponseIncoming(inner) => Display::fmt(inner, f),
            ParticipatingEvent::FinalitySignatureIncoming(inner) => Display::fmt(inner, f),
            ParticipatingEvent::ContractRuntime(inner) => Display::fmt(inner, f),
        }
    }
}
