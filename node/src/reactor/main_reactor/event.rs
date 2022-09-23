use std::fmt::{self, Debug, Display, Formatter};

use casper_execution_engine::core::{
    engine_state,
    engine_state::{GenesisSuccess, UpgradeSuccess},
};
use derive_more::From;
use serde::Serialize;

use crate::{
    components::{
        block_proposer,
        block_synchronizer::{
            self, BlockSyncRequest, GlobalStateSynchronizerEvent, TrieAccumulatorEvent,
        },
        block_validator, blocks_accumulator, chain_synchronizer, consensus, contract_runtime,
        deploy_acceptor, diagnostics_port, event_stream_server, fetcher, gossiper, linear_chain,
        rest_server, rpc_server,
        small_network::{self, GossipedAddress},
        storage, sync_leaper, upgrade_watcher,
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
            TrieDemand, TrieRequestIncoming, TrieResponseIncoming,
        },
        requests::{
            BeginGossipRequest, BlockProposerRequest, BlockValidationRequest,
            ChainspecRawBytesRequest, ConsensusRequest, ContractRuntimeRequest, FetcherRequest,
            MarkBlockCompletedRequest, MetricsRequest, NetworkInfoRequest, NetworkRequest,
            NodeStateRequest, RestRequest, RpcRequest, StateStoreRequest, StorageRequest,
            SyncGlobalStateRequest, SyncLeapRequest, TrieAccumulatorRequest, UpgradeWatcherRequest,
        },
    },
    protocol::Message,
    reactor::ReactorEvent,
    types::{
        Block, BlockAdded, BlockAndDeploys, BlockDeployApprovals, BlockHeader,
        BlockHeaderWithMetadata, BlockHeadersBatch, BlockSignatures, BlockWithMetadata, Deploy,
        FinalitySignature, SyncLeap, TrieOrChunk,
    },
};

/// Top-level event for the reactor.
#[derive(Debug, From, Serialize)]
#[must_use]
// Note: The large enum size must be reigned in eventually. This is a stopgap for now.
#[allow(clippy::large_enum_variant)]
pub(crate) enum MainEvent {
    // Control logic == Reactor self events
    // Shutdown the reactor, should only be raised by the reactor itself
    Shutdown(String),
    // Check the status of the reactor, should only be raised by the reactor itself
    CheckStatus,
    #[from]
    ChainspecRawBytesRequest(#[serde(skip_serializing)] ChainspecRawBytesRequest),
    #[from]
    GenesisResult(#[serde(skip_serializing)] Result<GenesisSuccess, engine_state::Error>),
    #[from]
    UpgradeResult {
        previous_block_header: Box<BlockHeader>,
        #[serde(skip_serializing)]
        result: Result<UpgradeSuccess, engine_state::Error>,
    },

    // SyncLeaper
    #[from]
    SyncLeaper(sync_leaper::Event),

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
    BlockDeployApprovalsFetcher(#[serde(skip_serializing)] fetcher::Event<BlockDeployApprovals>),
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
    BlockSynchronizer(#[serde(skip_serializing)] block_synchronizer::Event),

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
    BlockDeployApprovalsFetcherRequest(
        #[serde(skip_serializing)] FetcherRequest<BlockDeployApprovals>,
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
    BlockSynchronizerRequest(#[serde(skip_serializing)] BlockSyncRequest),

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
    BlockAddedRequestIncoming(BlockAddedRequestIncoming),
    #[from]
    BlockAddedResponseIncoming(BlockAddedResponseIncoming),
    #[from]
    FinalitySignatureIncoming(FinalitySignatureIncoming),
    #[from]
    BlockProposerAnnouncement(#[serde(skip_serializing)] BlockProposerAnnouncement),
}

impl ReactorEvent for MainEvent {
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
            MainEvent::Shutdown(_) => "Shutdown",
            MainEvent::CheckStatus => "CheckStatus",
            MainEvent::ChainSynchronizer(_) => "ChainSynchronizer",
            MainEvent::SmallNetwork(_) => "SmallNetwork",
            MainEvent::GenesisResult(_) => "GenesisResult",
            MainEvent::UpgradeResult { .. } => "UpgradeResult",
            MainEvent::SyncLeaper(_) => "SyncLeaper",
            MainEvent::BlockProposer(_) => "BlockProposer",
            MainEvent::Storage(_) => "Storage",
            MainEvent::RpcServer(_) => "RpcServer",
            MainEvent::RestServer(_) => "RestServer",
            MainEvent::EventStreamServer(_) => "EventStreamServer",
            MainEvent::UpgradeWatcher(_) => "UpgradeWatcher",
            MainEvent::Consensus(_) => "Consensus",
            MainEvent::DeployAcceptor(_) => "DeployAcceptor",
            MainEvent::DeployFetcher(_) => "DeployFetcher",
            MainEvent::DeployGossiper(_) => "DeployGossiper",
            MainEvent::BlockAddedGossiper(_) => "BlockGossiper",
            MainEvent::FinalitySignatureGossiper(_) => "FinalitySignatureGossiper",
            MainEvent::AddressGossiper(_) => "AddressGossiper",
            MainEvent::BlockValidator(_) => "BlockValidator",
            MainEvent::LinearChain(_) => "LinearChain",
            MainEvent::ContractRuntimeRequest(_) => "ContractRuntimeRequest",
            MainEvent::ChainSynchronizerRequest(_) => "ChainSynchronizerRequest",
            MainEvent::BlockFetcher(_) => "BlockFetcher",
            MainEvent::BlockHeaderFetcher(_) => "BlockHeaderFetcher",
            MainEvent::TrieOrChunkFetcher(_) => "TrieOrChunkFetcher",
            MainEvent::BlockByHeightFetcher(_) => "BlockByHeightFetcher",
            MainEvent::BlockHeaderByHeightFetcher(_) => "BlockHeaderByHeightFetcher",
            MainEvent::BlockAndDeploysFetcher(_) => "BlockAndDeploysFetcher",
            MainEvent::BlockDeployApprovalsFetcher(_) => "BlockDeployApprovalsFetcher",
            MainEvent::FinalitySignatureFetcher(_) => "FinalitySignatureFetcher",
            MainEvent::BlockHeadersBatchFetcher(_) => "BlockHeadersBatchFetcher",
            MainEvent::FinalitySignaturesFetcher(_) => "FinalitySignaturesFetcher",
            MainEvent::SyncLeapFetcher(_) => "SyncLeapFetcher",
            MainEvent::BlockAddedFetcher(_) => "BlockAddedFetcher",
            MainEvent::DiagnosticsPort(_) => "DiagnosticsPort",
            MainEvent::NetworkRequest(_) => "NetworkRequest",
            MainEvent::NetworkInfoRequest(_) => "NetworkInfoRequest",
            MainEvent::BlockFetcherRequest(_) => "BlockFetcherRequest",
            MainEvent::BlockHeaderFetcherRequest(_) => "BlockHeaderFetcherRequest",
            MainEvent::TrieOrChunkFetcherRequest(_) => "TrieOrChunkFetcherRequest",
            MainEvent::BlockByHeightFetcherRequest(_) => "BlockByHeightFetcherRequest",
            MainEvent::BlockHeaderByHeightFetcherRequest(_) => "BlockHeaderByHeightFetcherRequest",
            MainEvent::BlockAndDeploysFetcherRequest(_) => "BlockAndDeploysFetcherRequest",
            MainEvent::DeployFetcherRequest(_) => "DeployFetcherRequest",
            MainEvent::BlockDeployApprovalsFetcherRequest(_) => {
                "BlockDeployApprovalsFetcherRequest"
            }
            MainEvent::FinalitySignatureFetcherRequest(_) => "FinalitySignatureFetcherRequest",
            MainEvent::BlockHeadersBatchFetcherRequest(_) => "BlockHeadersBatchFetcherRequest",
            MainEvent::FinalitySignaturesFetcherRequest(_) => "FinalitySignaturesFetcherRequest",
            MainEvent::SyncLeapFetcherRequest(_) => "SyncLeapFetcherRequest",
            MainEvent::BlockAddedFetcherRequest(_) => "BlockAddedFetcherRequest",
            MainEvent::BlockProposerRequest(_) => "BlockProposerRequest",
            MainEvent::BlockValidatorRequest(_) => "BlockValidatorRequest",
            MainEvent::MetricsRequest(_) => "MetricsRequest",
            MainEvent::ChainspecRawBytesRequest(_) => "ChainspecRawBytesRequest",
            MainEvent::UpgradeWatcherRequest(_) => "UpgradeWatcherRequest",
            MainEvent::StorageRequest(_) => "StorageRequest",
            MainEvent::MarkBlockCompletedRequest(_) => "MarkBlockCompletedRequest",
            MainEvent::StateStoreRequest(_) => "StateStoreRequest",
            MainEvent::DumpConsensusStateRequest(_) => "DumpConsensusStateRequest",
            MainEvent::ControlAnnouncement(_) => "ControlAnnouncement",
            MainEvent::RpcServerAnnouncement(_) => "RpcServerAnnouncement",
            MainEvent::DeployAcceptorAnnouncement(_) => "DeployAcceptorAnnouncement",
            MainEvent::ConsensusAnnouncement(_) => "ConsensusAnnouncement",
            MainEvent::ContractRuntimeAnnouncement(_) => "ContractRuntimeAnnouncement",
            MainEvent::DeployGossiperAnnouncement(_) => "DeployGossiperAnnouncement",
            MainEvent::AddressGossiperAnnouncement(_) => "AddressGossiperAnnouncement",
            MainEvent::LinearChainAnnouncement(_) => "LinearChainAnnouncement",
            MainEvent::UpgradeWatcherAnnouncement(_) => "UpgradeWatcherAnnouncement",
            MainEvent::BlocklistAnnouncement(_) => "BlocklistAnnouncement",
            MainEvent::BlockProposerAnnouncement(_) => "BlockProposerAnnouncement",
            MainEvent::BeginAddressGossipRequest(_) => "BeginAddressGossipRequest",
            MainEvent::ConsensusMessageIncoming(_) => "ConsensusMessageIncoming",
            MainEvent::DeployGossiperIncoming(_) => "DeployGossiperIncoming",
            MainEvent::BlockAddedGossiperIncoming(_) => "BlockGossiperIncoming",
            MainEvent::FinalitySignatureGossiperIncoming(_) => "FinalitySignatureGossiperIncoming",
            MainEvent::AddressGossiperIncoming(_) => "AddressGossiperIncoming",
            MainEvent::NetRequestIncoming(_) => "NetRequestIncoming",
            MainEvent::NetResponseIncoming(_) => "NetResponseIncoming",
            MainEvent::TrieRequestIncoming(_) => "TrieRequestIncoming",
            MainEvent::TrieDemand(_) => "TrieDemand",
            MainEvent::TrieResponseIncoming(_) => "TrieResponseIncoming",
            MainEvent::BlockAddedRequestIncoming(_) => "BlockAddedRequestIncoming",
            MainEvent::BlockAddedResponseIncoming(_) => "BlockAddedResponseIncoming",
            MainEvent::FinalitySignatureIncoming(_) => "FinalitySignatureIncoming",
            MainEvent::ContractRuntime(_) => "ContractRuntime",
            MainEvent::ChainSynchronizerAnnouncement(_) => "ChainSynchronizerAnnouncement",
            MainEvent::BlockAddedGossiperAnnouncement(_) => "BlockGossiperAnnouncement",
            MainEvent::FinalitySignatureGossiperAnnouncement(_) => {
                "FinalitySignatureGossiperAnnouncement"
            }
            MainEvent::BlocksAccumulator(_) => "BlocksAccumulator",
            MainEvent::BlockSynchronizer(_) => "BlockSynchronizer",
            MainEvent::BlockSynchronizerRequest(_) => "BlockSynchronizerRequest",
        }
    }
}

impl From<SyncGlobalStateRequest> for MainEvent {
    fn from(request: SyncGlobalStateRequest) -> Self {
        MainEvent::BlockSynchronizer(block_synchronizer::Event::GlobalStateSynchronizer(
            request.into(),
        ))
    }
}

impl From<TrieAccumulatorRequest> for MainEvent {
    fn from(request: TrieAccumulatorRequest) -> Self {
        MainEvent::BlockSynchronizer(block_synchronizer::Event::GlobalStateSynchronizer(
            block_synchronizer::GlobalStateSynchronizerEvent::TrieAccumulatorEvent(request.into()),
        ))
    }
}

impl From<GlobalStateSynchronizerEvent> for MainEvent {
    fn from(event: GlobalStateSynchronizerEvent) -> Self {
        MainEvent::BlockSynchronizer(event.into())
    }
}

impl From<TrieAccumulatorEvent> for MainEvent {
    fn from(event: TrieAccumulatorEvent) -> Self {
        MainEvent::BlockSynchronizer(block_synchronizer::Event::GlobalStateSynchronizer(
            event.into(),
        ))
    }
}

impl From<RpcRequest> for MainEvent {
    fn from(request: RpcRequest) -> Self {
        MainEvent::RpcServer(rpc_server::Event::RpcRequest(request))
    }
}

impl From<RestRequest> for MainEvent {
    fn from(request: RestRequest) -> Self {
        MainEvent::RestServer(rest_server::Event::RestRequest(request))
    }
}

impl From<NetworkRequest<consensus::ConsensusMessage>> for MainEvent {
    fn from(request: NetworkRequest<consensus::ConsensusMessage>) -> Self {
        MainEvent::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<NetworkRequest<gossiper::Message<Deploy>>> for MainEvent {
    fn from(request: NetworkRequest<gossiper::Message<Deploy>>) -> Self {
        MainEvent::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<NetworkRequest<gossiper::Message<BlockAdded>>> for MainEvent {
    fn from(request: NetworkRequest<gossiper::Message<BlockAdded>>) -> Self {
        MainEvent::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<NetworkRequest<gossiper::Message<FinalitySignature>>> for MainEvent {
    fn from(request: NetworkRequest<gossiper::Message<FinalitySignature>>) -> Self {
        MainEvent::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<NetworkRequest<gossiper::Message<GossipedAddress>>> for MainEvent {
    fn from(request: NetworkRequest<gossiper::Message<GossipedAddress>>) -> Self {
        MainEvent::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<ConsensusRequest> for MainEvent {
    fn from(request: ConsensusRequest) -> Self {
        MainEvent::Consensus(consensus::Event::ConsensusRequest(request))
    }
}

impl Display for MainEvent {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            MainEvent::Shutdown(msg) => write!(f, "shutdown: {}", msg),
            MainEvent::CheckStatus => write!(f, "check status"),
            MainEvent::ChainSynchronizer(event) => {
                write!(f, "chain synchronizer: {}", event)
            }
            MainEvent::Storage(event) => write!(f, "storage: {}", event),
            MainEvent::SmallNetwork(event) => write!(f, "small network: {}", event),
            MainEvent::GenesisResult(result) => write!(f, "genesis result: {:?}", result),
            MainEvent::UpgradeResult { result, .. } => write!(f, "upgrade result: {:?}", result),
            MainEvent::SyncLeaper(event) => write!(f, "sync leaper: {}", event),
            MainEvent::BlockProposer(event) => write!(f, "block proposer: {}", event),
            MainEvent::RpcServer(event) => write!(f, "rpc server: {}", event),
            MainEvent::RestServer(event) => write!(f, "rest server: {}", event),
            MainEvent::EventStreamServer(event) => {
                write!(f, "event stream server: {}", event)
            }
            MainEvent::UpgradeWatcher(event) => write!(f, "upgrade watcher: {}", event),
            MainEvent::Consensus(event) => write!(f, "consensus: {}", event),
            MainEvent::DeployAcceptor(event) => write!(f, "deploy acceptor: {}", event),
            MainEvent::DeployFetcher(event) => write!(f, "deploy fetcher: {}", event),
            MainEvent::DeployGossiper(event) => write!(f, "deploy gossiper: {}", event),
            MainEvent::BlockAddedGossiper(event) => write!(f, "block gossiper: {}", event),
            MainEvent::FinalitySignatureGossiper(event) => {
                write!(f, "block signature gossiper: {}", event)
            }
            MainEvent::AddressGossiper(event) => write!(f, "address gossiper: {}", event),
            MainEvent::ContractRuntimeRequest(event) => {
                write!(f, "contract runtime request: {:?}", event)
            }
            MainEvent::LinearChain(event) => write!(f, "linear-chain event {}", event),
            MainEvent::BlockValidator(event) => write!(f, "block validator: {}", event),
            MainEvent::BlockFetcher(event) => write!(f, "block fetcher: {}", event),
            MainEvent::BlockHeaderFetcher(event) => {
                write!(f, "block header fetcher: {}", event)
            }
            MainEvent::TrieOrChunkFetcher(event) => {
                write!(f, "trie or chunk fetcher: {}", event)
            }
            MainEvent::BlockByHeightFetcher(event) => {
                write!(f, "block by height fetcher: {}", event)
            }
            MainEvent::BlockHeaderByHeightFetcher(event) => {
                write!(f, "block header by height fetcher: {}", event)
            }
            MainEvent::BlockAndDeploysFetcher(event) => {
                write!(f, "block and deploys fetcher: {}", event)
            }
            MainEvent::BlockDeployApprovalsFetcher(event) => {
                write!(f, "finalized approvals fetcher: {}", event)
            }
            MainEvent::FinalitySignatureFetcher(event) => {
                write!(f, "finality signature fetcher: {}", event)
            }
            MainEvent::BlockHeadersBatchFetcher(event) => {
                write!(f, "block headers batch fetcher: {}", event)
            }
            MainEvent::FinalitySignaturesFetcher(event) => {
                write!(f, "finality signatures fetcher: {}", event)
            }
            MainEvent::SyncLeapFetcher(event) => {
                write!(f, "sync leap fetcher: {}", event)
            }
            MainEvent::BlockAddedFetcher(event) => {
                write!(f, "block added fetcher: {}", event)
            }
            MainEvent::BlocksAccumulator(event) => {
                write!(f, "blocks accumulator: {}", event)
            }
            MainEvent::BlockSynchronizer(event) => {
                write!(f, "block synchronizer: {}", event)
            }
            MainEvent::DiagnosticsPort(event) => write!(f, "diagnostics port: {}", event),
            MainEvent::ChainSynchronizerRequest(req) => {
                write!(f, "chain synchronizer request: {}", req)
            }
            MainEvent::NetworkRequest(req) => write!(f, "network request: {}", req),
            MainEvent::NetworkInfoRequest(req) => {
                write!(f, "network info request: {}", req)
            }
            MainEvent::ChainspecRawBytesRequest(req) => {
                write!(f, "chainspec loader request: {}", req)
            }
            MainEvent::UpgradeWatcherRequest(req) => {
                write!(f, "upgrade watcher request: {}", req)
            }
            MainEvent::StorageRequest(req) => write!(f, "storage request: {}", req),
            MainEvent::MarkBlockCompletedRequest(req) => {
                write!(f, "mark block completed request: {}", req)
            }
            MainEvent::StateStoreRequest(req) => write!(f, "state store request: {}", req),
            MainEvent::BlockFetcherRequest(request) => {
                write!(f, "block fetcher request: {}", request)
            }
            MainEvent::BlockHeaderFetcherRequest(request) => {
                write!(f, "block header fetcher request: {}", request)
            }
            MainEvent::TrieOrChunkFetcherRequest(request) => {
                write!(f, "trie or chunk fetcher request: {}", request)
            }
            MainEvent::BlockByHeightFetcherRequest(request) => {
                write!(f, "block by height fetcher request: {}", request)
            }
            MainEvent::BlockHeaderByHeightFetcherRequest(request) => {
                write!(f, "block header by height fetcher request: {}", request)
            }
            MainEvent::BlockAndDeploysFetcherRequest(request) => {
                write!(f, "block and deploys fetcher request: {}", request)
            }
            MainEvent::DeployFetcherRequest(request) => {
                write!(f, "deploy fetcher request: {}", request)
            }
            MainEvent::BlockDeployApprovalsFetcherRequest(request) => {
                write!(f, "finalized approvals fetcher request: {}", request)
            }
            MainEvent::FinalitySignatureFetcherRequest(request) => {
                write!(f, "finality signature fetcher request: {}", request)
            }
            MainEvent::BlockHeadersBatchFetcherRequest(request) => {
                write!(f, "block headers batch fetcher request: {}", request)
            }
            MainEvent::FinalitySignaturesFetcherRequest(request) => {
                write!(f, "finality signatures fetcher request: {}", request)
            }
            MainEvent::SyncLeapFetcherRequest(request) => {
                write!(f, "sync leap fetcher request: {}", request)
            }
            MainEvent::BlockAddedFetcherRequest(request) => {
                write!(f, "block added fetcher request: {}", request)
            }
            MainEvent::BeginAddressGossipRequest(request) => {
                write!(f, "begin address gossip request: {}", request)
            }
            MainEvent::BlockProposerRequest(req) => {
                write!(f, "block proposer request: {}", req)
            }
            MainEvent::BlockValidatorRequest(req) => {
                write!(f, "block validator request: {}", req)
            }
            MainEvent::MetricsRequest(req) => write!(f, "metrics request: {}", req),
            MainEvent::BlockSynchronizerRequest(req) => {
                write!(f, "block synchronizer request: {}", req)
            }
            MainEvent::ControlAnnouncement(ctrl_ann) => write!(f, "control: {}", ctrl_ann),
            MainEvent::DumpConsensusStateRequest(req) => {
                write!(f, "dump consensus state: {}", req)
            }
            MainEvent::RpcServerAnnouncement(ann) => {
                write!(f, "api server announcement: {}", ann)
            }
            MainEvent::DeployAcceptorAnnouncement(ann) => {
                write!(f, "deploy acceptor announcement: {}", ann)
            }
            MainEvent::ConsensusAnnouncement(ann) => {
                write!(f, "consensus announcement: {}", ann)
            }
            MainEvent::ContractRuntimeAnnouncement(ann) => {
                write!(f, "block-executor announcement: {}", ann)
            }
            MainEvent::DeployGossiperAnnouncement(ann) => {
                write!(f, "deploy gossiper announcement: {}", ann)
            }
            MainEvent::BlockAddedGossiperAnnouncement(ann) => {
                write!(f, "block gossiper announcement: {}", ann)
            }
            MainEvent::FinalitySignatureGossiperAnnouncement(ann) => {
                write!(f, "block signature gossiper announcement: {}", ann)
            }
            MainEvent::AddressGossiperAnnouncement(ann) => {
                write!(f, "address gossiper announcement: {}", ann)
            }
            MainEvent::LinearChainAnnouncement(ann) => {
                write!(f, "linear chain announcement: {}", ann)
            }
            MainEvent::BlockProposerAnnouncement(ann) => {
                write!(f, "block proposer announcement: {}", ann)
            }
            MainEvent::UpgradeWatcherAnnouncement(ann) => {
                write!(f, "chainspec loader announcement: {}", ann)
            }
            MainEvent::BlocklistAnnouncement(ann) => {
                write!(f, "blocklist announcement: {}", ann)
            }
            MainEvent::ChainSynchronizerAnnouncement(ann) => {
                write!(f, "chain synchronizer announcement: {}", ann)
            }
            MainEvent::ConsensusMessageIncoming(inner) => Display::fmt(inner, f),
            MainEvent::DeployGossiperIncoming(inner) => Display::fmt(inner, f),
            MainEvent::BlockAddedGossiperIncoming(inner) => Display::fmt(inner, f),
            MainEvent::FinalitySignatureGossiperIncoming(inner) => Display::fmt(inner, f),
            MainEvent::AddressGossiperIncoming(inner) => Display::fmt(inner, f),
            MainEvent::NetRequestIncoming(inner) => Display::fmt(inner, f),
            MainEvent::NetResponseIncoming(inner) => Display::fmt(inner, f),
            MainEvent::TrieRequestIncoming(inner) => Display::fmt(inner, f),
            MainEvent::TrieDemand(inner) => Display::fmt(inner, f),
            MainEvent::TrieResponseIncoming(inner) => Display::fmt(inner, f),
            MainEvent::BlockAddedRequestIncoming(inner) => Display::fmt(inner, f),
            MainEvent::BlockAddedResponseIncoming(inner) => Display::fmt(inner, f),
            MainEvent::FinalitySignatureIncoming(inner) => Display::fmt(inner, f),
            MainEvent::ContractRuntime(inner) => Display::fmt(inner, f),
        }
    }
}
