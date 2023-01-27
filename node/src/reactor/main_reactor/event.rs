use std::fmt::{self, Debug, Display, Formatter};

use derive_more::From;
use serde::Serialize;

use crate::{
    components::{
        block_accumulator,
        block_synchronizer::{self, GlobalStateSynchronizerEvent, TrieAccumulatorEvent},
        block_validator, consensus, contract_runtime, deploy_acceptor, deploy_buffer,
        diagnostics_port, event_stream_server, fetcher, gossiper,
        network::{self, GossipedAddress},
        rest_server, rpc_server, shutdown_trigger, storage, sync_leaper, upgrade_watcher,
    },
    effect::{
        announcements::{
            BlockAccumulatorAnnouncement, ConsensusAnnouncement, ContractRuntimeAnnouncement,
            ControlAnnouncement, DeployAcceptorAnnouncement, DeployBufferAnnouncement,
            FatalAnnouncement, GossiperAnnouncement, MetaBlockAnnouncement,
            PeerBehaviorAnnouncement, RpcServerAnnouncement, UpgradeWatcherAnnouncement,
        },
        diagnostics_port::DumpConsensusStateRequest,
        incoming::{
            ConsensusDemand, ConsensusMessageIncoming, FinalitySignatureIncoming, GossiperIncoming,
            NetRequestIncoming, NetResponseIncoming, TrieDemand, TrieRequestIncoming,
            TrieResponseIncoming,
        },
        requests::{
            BeginGossipRequest, BlockAccumulatorRequest, BlockCompleteConfirmationRequest,
            BlockSynchronizerRequest, BlockValidationRequest, ChainspecRawBytesRequest,
            ConsensusRequest, ContractRuntimeRequest, DeployBufferRequest, FetcherRequest,
            MakeBlockExecutableRequest, MetricsRequest, NetworkInfoRequest, NetworkRequest,
            ReactorStatusRequest, RestRequest, RpcRequest, SetNodeStopRequest, StorageRequest,
            SyncGlobalStateRequest, TrieAccumulatorRequest, UpgradeWatcherRequest,
        },
    },
    protocol::Message,
    reactor::ReactorEvent,
    types::{
        ApprovalsHashes, Block, BlockExecutionResultsOrChunk, BlockHeader, Deploy,
        FinalitySignature, LegacyDeploy, SyncLeap, TrieOrChunk,
    },
};

/// Top-level event for the reactor.
#[derive(Debug, From, Serialize)]
#[must_use]
// Note: The large enum size must be reigned in eventually. This is a stopgap for now.
#[allow(clippy::large_enum_variant)]
pub(crate) enum MainEvent {
    #[from]
    ControlAnnouncement(ControlAnnouncement),
    #[from]
    FatalAnnouncement(FatalAnnouncement),

    /// Check the status of the reactor, should only be raised by the reactor itself
    ReactorCrank,

    #[from]
    UpgradeWatcher(#[serde(skip_serializing)] upgrade_watcher::Event),
    #[from]
    UpgradeWatcherRequest(#[serde(skip_serializing)] UpgradeWatcherRequest),
    #[from]
    UpgradeWatcherAnnouncement(#[serde(skip_serializing)] UpgradeWatcherAnnouncement),
    #[from]
    RpcServer(#[serde(skip_serializing)] rpc_server::Event),
    #[from]
    RpcServerAnnouncement(#[serde(skip_serializing)] RpcServerAnnouncement),
    #[from]
    RestServer(#[serde(skip_serializing)] rest_server::Event),
    #[from]
    MetricsRequest(#[serde(skip_serializing)] MetricsRequest),
    #[from]
    ChainspecRawBytesRequest(#[serde(skip_serializing)] ChainspecRawBytesRequest),
    #[from]
    EventStreamServer(#[serde(skip_serializing)] event_stream_server::Event),
    #[from]
    ShutdownTrigger(shutdown_trigger::Event),
    #[from]
    DiagnosticsPort(diagnostics_port::Event),
    #[from]
    DumpConsensusStateRequest(DumpConsensusStateRequest),
    #[from]
    Network(network::Event<Message>),
    #[from]
    NetworkRequest(#[serde(skip_serializing)] NetworkRequest<Message>),
    #[from]
    NetworkInfoRequest(#[serde(skip_serializing)] NetworkInfoRequest),
    #[from]
    NetworkPeerBehaviorAnnouncement(PeerBehaviorAnnouncement),
    #[from]
    NetworkPeerRequestingData(NetRequestIncoming),
    #[from]
    NetworkPeerProvidingData(NetResponseIncoming),
    #[from]
    AddressGossiper(gossiper::Event<GossipedAddress>),
    #[from]
    AddressGossiperCrank(BeginGossipRequest<GossipedAddress>),
    #[from]
    AddressGossiperIncoming(GossiperIncoming<GossipedAddress>),
    #[from]
    AddressGossiperAnnouncement(#[serde(skip_serializing)] GossiperAnnouncement<GossipedAddress>),
    #[from]
    SyncLeaper(sync_leaper::Event),
    #[from]
    SyncLeapFetcher(#[serde(skip_serializing)] fetcher::Event<SyncLeap>),
    #[from]
    SyncLeapFetcherRequest(#[serde(skip_serializing)] FetcherRequest<SyncLeap>),
    #[from]
    Consensus(#[serde(skip_serializing)] consensus::Event),
    #[from]
    ConsensusMessageIncoming(ConsensusMessageIncoming),
    #[from]
    ConsensusDemand(ConsensusDemand),
    #[from]
    ConsensusAnnouncement(#[serde(skip_serializing)] ConsensusAnnouncement),
    #[from]
    BlockHeaderFetcher(#[serde(skip_serializing)] fetcher::Event<BlockHeader>),
    #[from]
    BlockHeaderFetcherRequest(#[serde(skip_serializing)] FetcherRequest<BlockHeader>),
    #[from]
    BlockValidator(#[serde(skip_serializing)] block_validator::Event),
    #[from]
    BlockValidatorRequest(#[serde(skip_serializing)] BlockValidationRequest),
    #[from]
    BlockAccumulator(#[serde(skip_serializing)] block_accumulator::Event),
    #[from]
    BlockAccumulatorRequest(#[serde(skip_serializing)] BlockAccumulatorRequest),
    #[from]
    BlockAccumulatorAnnouncement(#[serde(skip_serializing)] BlockAccumulatorAnnouncement),
    #[from]
    BlockSynchronizer(#[serde(skip_serializing)] block_synchronizer::Event),
    #[from]
    BlockSynchronizerRequest(#[serde(skip_serializing)] BlockSynchronizerRequest),

    #[from]
    ApprovalsHashesFetcher(#[serde(skip_serializing)] fetcher::Event<ApprovalsHashes>),
    #[from]
    ApprovalsHashesFetcherRequest(#[serde(skip_serializing)] FetcherRequest<ApprovalsHashes>),

    #[from]
    BlockGossiper(#[serde(skip_serializing)] gossiper::Event<Block>),
    #[from]
    BlockGossiperIncoming(GossiperIncoming<Block>),
    #[from]
    BlockGossiperAnnouncement(#[serde(skip_serializing)] GossiperAnnouncement<Block>),
    #[from]
    BlockFetcher(#[serde(skip_serializing)] fetcher::Event<Block>),
    #[from]
    BlockFetcherRequest(#[serde(skip_serializing)] FetcherRequest<Block>),
    #[from]
    MakeBlockExecutableRequest(MakeBlockExecutableRequest),
    #[from]
    BlockCompleteConfirmationRequest(BlockCompleteConfirmationRequest),
    #[from]
    FinalitySignatureIncoming(FinalitySignatureIncoming),
    #[from]
    FinalitySignatureGossiper(#[serde(skip_serializing)] gossiper::Event<FinalitySignature>),
    #[from]
    FinalitySignatureGossiperIncoming(GossiperIncoming<FinalitySignature>),
    #[from]
    FinalitySignatureGossiperAnnouncement(
        #[serde(skip_serializing)] GossiperAnnouncement<FinalitySignature>,
    ),
    #[from]
    FinalitySignatureFetcher(#[serde(skip_serializing)] fetcher::Event<FinalitySignature>),
    #[from]
    FinalitySignatureFetcherRequest(#[serde(skip_serializing)] FetcherRequest<FinalitySignature>),
    #[from]
    DeployAcceptor(#[serde(skip_serializing)] deploy_acceptor::Event),
    #[from]
    DeployAcceptorAnnouncement(#[serde(skip_serializing)] DeployAcceptorAnnouncement),
    #[from]
    DeployGossiper(#[serde(skip_serializing)] gossiper::Event<Deploy>),
    #[from]
    DeployGossiperIncoming(GossiperIncoming<Deploy>),
    #[from]
    DeployGossiperAnnouncement(#[serde(skip_serializing)] GossiperAnnouncement<Deploy>),
    #[from]
    DeployBuffer(#[serde(skip_serializing)] deploy_buffer::Event),
    #[from]
    DeployBufferAnnouncement(#[serde(skip_serializing)] DeployBufferAnnouncement),
    #[from]
    LegacyDeployFetcher(#[serde(skip_serializing)] fetcher::Event<LegacyDeploy>),
    #[from]
    LegacyDeployFetcherRequest(#[serde(skip_serializing)] FetcherRequest<LegacyDeploy>),
    #[from]
    DeployFetcher(#[serde(skip_serializing)] fetcher::Event<Deploy>),
    #[from]
    DeployFetcherRequest(#[serde(skip_serializing)] FetcherRequest<Deploy>),
    #[from]
    DeployBufferRequest(DeployBufferRequest),
    #[from]
    ContractRuntime(contract_runtime::Event),
    #[from]
    ContractRuntimeRequest(ContractRuntimeRequest),
    #[from]
    ContractRuntimeAnnouncement(#[serde(skip_serializing)] ContractRuntimeAnnouncement),
    #[from]
    TrieOrChunkFetcher(#[serde(skip_serializing)] fetcher::Event<TrieOrChunk>),
    #[from]
    TrieOrChunkFetcherRequest(#[serde(skip_serializing)] FetcherRequest<TrieOrChunk>),
    #[from]
    BlockExecutionResultsOrChunkFetcher(
        #[serde(skip_serializing)] fetcher::Event<BlockExecutionResultsOrChunk>,
    ),
    #[from]
    BlockExecutionResultsOrChunkFetcherRequest(
        #[serde(skip_serializing)] FetcherRequest<BlockExecutionResultsOrChunk>,
    ),
    #[from]
    TrieRequestIncoming(TrieRequestIncoming),
    #[from]
    TrieDemand(TrieDemand),
    #[from]
    TrieResponseIncoming(TrieResponseIncoming),
    #[from]
    Storage(storage::Event),
    #[from]
    StorageRequest(StorageRequest),
    #[from]
    SetNodeStopRequest(SetNodeStopRequest),
    #[from]
    MainReactorRequest(ReactorStatusRequest),
    #[from]
    MetaBlockAnnouncement(MetaBlockAnnouncement),
}

impl ReactorEvent for MainEvent {
    fn is_control(&self) -> bool {
        matches!(self, MainEvent::ControlAnnouncement(_))
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
            MainEvent::ReactorCrank => "ReactorCrank",
            MainEvent::Network(_) => "Network",
            MainEvent::SyncLeaper(_) => "SyncLeaper",
            MainEvent::DeployBuffer(_) => "DeployBuffer",
            MainEvent::Storage(_) => "Storage",
            MainEvent::RpcServer(_) => "RpcServer",
            MainEvent::RestServer(_) => "RestServer",
            MainEvent::EventStreamServer(_) => "EventStreamServer",
            MainEvent::UpgradeWatcher(_) => "UpgradeWatcher",
            MainEvent::Consensus(_) => "Consensus",
            MainEvent::DeployAcceptor(_) => "DeployAcceptor",
            MainEvent::LegacyDeployFetcher(_) => "LegacyDeployFetcher",
            MainEvent::DeployFetcher(_) => "DeployFetcher",
            MainEvent::DeployGossiper(_) => "DeployGossiper",
            MainEvent::FinalitySignatureGossiper(_) => "FinalitySignatureGossiper",
            MainEvent::AddressGossiper(_) => "AddressGossiper",
            MainEvent::BlockValidator(_) => "BlockValidator",
            MainEvent::ContractRuntimeRequest(_) => "ContractRuntimeRequest",
            MainEvent::BlockHeaderFetcher(_) => "BlockHeaderFetcher",
            MainEvent::TrieOrChunkFetcher(_) => "TrieOrChunkFetcher",
            MainEvent::BlockExecutionResultsOrChunkFetcher(_) => {
                "BlockExecutionResultsOrChunkFetcher"
            }
            MainEvent::FinalitySignatureFetcher(_) => "FinalitySignatureFetcher",
            MainEvent::SyncLeapFetcher(_) => "SyncLeapFetcher",
            MainEvent::ApprovalsHashesFetcher(_) => "ApprovalsHashesFetcher",
            MainEvent::ShutdownTrigger(_) => "ShutdownTrigger",
            MainEvent::DiagnosticsPort(_) => "DiagnosticsPort",
            MainEvent::NetworkRequest(_) => "NetworkRequest",
            MainEvent::NetworkInfoRequest(_) => "NetworkInfoRequest",
            MainEvent::BlockHeaderFetcherRequest(_) => "BlockHeaderFetcherRequest",
            MainEvent::TrieOrChunkFetcherRequest(_) => "TrieOrChunkFetcherRequest",
            MainEvent::BlockExecutionResultsOrChunkFetcherRequest(_) => {
                "BlockExecutionResultsOrChunkFetcherRequest"
            }
            MainEvent::LegacyDeployFetcherRequest(_) => "LegacyDeployFetcherRequest",
            MainEvent::DeployFetcherRequest(_) => "DeployFetcherRequest",
            MainEvent::FinalitySignatureFetcherRequest(_) => "FinalitySignatureFetcherRequest",
            MainEvent::SyncLeapFetcherRequest(_) => "SyncLeapFetcherRequest",
            MainEvent::ApprovalsHashesFetcherRequest(_) => "ApprovalsHashesFetcherRequest",
            MainEvent::DeployBufferRequest(_) => "DeployBufferRequest",
            MainEvent::BlockValidatorRequest(_) => "BlockValidatorRequest",
            MainEvent::MetricsRequest(_) => "MetricsRequest",
            MainEvent::ChainspecRawBytesRequest(_) => "ChainspecRawBytesRequest",
            MainEvent::UpgradeWatcherRequest(_) => "UpgradeWatcherRequest",
            MainEvent::StorageRequest(_) => "StorageRequest",
            MainEvent::BlockCompleteConfirmationRequest(_) => "MarkBlockCompletedRequest",
            MainEvent::DumpConsensusStateRequest(_) => "DumpConsensusStateRequest",
            MainEvent::ControlAnnouncement(_) => "ControlAnnouncement",
            MainEvent::FatalAnnouncement(_) => "FatalAnnouncement",
            MainEvent::RpcServerAnnouncement(_) => "RpcServerAnnouncement",
            MainEvent::DeployAcceptorAnnouncement(_) => "DeployAcceptorAnnouncement",
            MainEvent::ConsensusAnnouncement(_) => "ConsensusAnnouncement",
            MainEvent::ContractRuntimeAnnouncement(_) => "ContractRuntimeAnnouncement",
            MainEvent::DeployGossiperAnnouncement(_) => "DeployGossiperAnnouncement",
            MainEvent::AddressGossiperAnnouncement(_) => "AddressGossiperAnnouncement",
            MainEvent::UpgradeWatcherAnnouncement(_) => "UpgradeWatcherAnnouncement",
            MainEvent::NetworkPeerBehaviorAnnouncement(_) => "BlocklistAnnouncement",
            MainEvent::DeployBufferAnnouncement(_) => "DeployBufferAnnouncement",
            MainEvent::AddressGossiperCrank(_) => "BeginAddressGossipRequest",
            MainEvent::ConsensusMessageIncoming(_) => "ConsensusMessageIncoming",
            MainEvent::ConsensusDemand(_) => "ConsensusDemand",
            MainEvent::DeployGossiperIncoming(_) => "DeployGossiperIncoming",
            MainEvent::FinalitySignatureGossiperIncoming(_) => "FinalitySignatureGossiperIncoming",
            MainEvent::AddressGossiperIncoming(_) => "AddressGossiperIncoming",
            MainEvent::NetworkPeerRequestingData(_) => "NetRequestIncoming",
            MainEvent::NetworkPeerProvidingData(_) => "NetResponseIncoming",
            MainEvent::TrieRequestIncoming(_) => "TrieRequestIncoming",
            MainEvent::TrieDemand(_) => "TrieDemand",
            MainEvent::TrieResponseIncoming(_) => "TrieResponseIncoming",
            MainEvent::FinalitySignatureIncoming(_) => "FinalitySignatureIncoming",
            MainEvent::ContractRuntime(_) => "ContractRuntime",
            MainEvent::FinalitySignatureGossiperAnnouncement(_) => {
                "FinalitySignatureGossiperAnnouncement"
            }
            MainEvent::BlockAccumulator(_) => "BlockAccumulator",
            MainEvent::BlockAccumulatorRequest(_) => "BlockAccumulatorRequest",
            MainEvent::BlockAccumulatorAnnouncement(_) => "BlockAccumulatorAnnouncement",
            MainEvent::BlockSynchronizer(_) => "BlockSynchronizer",
            MainEvent::BlockSynchronizerRequest(_) => "BlockSynchronizerRequest",
            MainEvent::BlockGossiper(_) => "BlockGossiper",
            MainEvent::BlockGossiperIncoming(_) => "BlockGossiperIncoming",
            MainEvent::BlockGossiperAnnouncement(_) => "BlockGossiperAnnouncement",
            MainEvent::BlockFetcher(_) => "BlockFetcher",
            MainEvent::BlockFetcherRequest(_) => "BlockFetcherRequest",
            MainEvent::SetNodeStopRequest(_) => "SetNodeStopRequest",
            MainEvent::MainReactorRequest(_) => "MainReactorRequest",
            MainEvent::MakeBlockExecutableRequest(_) => "MakeBlockExecutableRequest",
            MainEvent::MetaBlockAnnouncement(_) => "MetaBlockAnnouncement",
        }
    }
}

impl Display for MainEvent {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            MainEvent::ReactorCrank => write!(f, "reactor crank"),
            MainEvent::Storage(event) => write!(f, "storage: {}", event),
            MainEvent::Network(event) => write!(f, "network: {}", event),
            MainEvent::SyncLeaper(event) => write!(f, "sync leaper: {}", event),
            MainEvent::DeployBuffer(event) => write!(f, "deploy buffer: {}", event),
            MainEvent::RpcServer(event) => write!(f, "rpc server: {}", event),
            MainEvent::RestServer(event) => write!(f, "rest server: {}", event),
            MainEvent::EventStreamServer(event) => {
                write!(f, "event stream server: {}", event)
            }
            MainEvent::UpgradeWatcher(event) => write!(f, "upgrade watcher: {}", event),
            MainEvent::Consensus(event) => write!(f, "consensus: {}", event),
            MainEvent::DeployAcceptor(event) => write!(f, "deploy acceptor: {}", event),
            MainEvent::LegacyDeployFetcher(event) => write!(f, "legacy deploy fetcher: {}", event),
            MainEvent::DeployFetcher(event) => write!(f, "deploy fetcher: {}", event),
            MainEvent::DeployGossiper(event) => write!(f, "deploy gossiper: {}", event),
            MainEvent::FinalitySignatureGossiper(event) => {
                write!(f, "block signature gossiper: {}", event)
            }
            MainEvent::AddressGossiper(event) => write!(f, "address gossiper: {}", event),
            MainEvent::ContractRuntimeRequest(event) => {
                write!(f, "contract runtime request: {:?}", event)
            }
            MainEvent::BlockValidator(event) => write!(f, "block validator: {}", event),
            MainEvent::BlockHeaderFetcher(event) => {
                write!(f, "block header fetcher: {}", event)
            }
            MainEvent::TrieOrChunkFetcher(event) => {
                write!(f, "trie or chunk fetcher: {}", event)
            }
            MainEvent::BlockExecutionResultsOrChunkFetcher(event) => {
                write!(f, "block execution results or chunk fetcher: {}", event)
            }
            MainEvent::FinalitySignatureFetcher(event) => {
                write!(f, "finality signature fetcher: {}", event)
            }
            MainEvent::SyncLeapFetcher(event) => {
                write!(f, "sync leap fetcher: {}", event)
            }
            MainEvent::ApprovalsHashesFetcher(event) => {
                write!(f, "approvals hashes fetcher: {}", event)
            }
            MainEvent::BlockAccumulator(event) => {
                write!(f, "block accumulator: {}", event)
            }
            MainEvent::BlockAccumulatorRequest(req) => {
                write!(f, "block accumulator request: {}", req)
            }
            MainEvent::BlockAccumulatorAnnouncement(ann) => {
                write!(f, "block accumulator announcement: {}", ann)
            }
            MainEvent::BlockSynchronizer(event) => {
                write!(f, "block synchronizer: {}", event)
            }
            MainEvent::BlockSynchronizerRequest(req) => {
                write!(f, "block synchronizer request: {}", req)
            }
            MainEvent::ShutdownTrigger(event) => write!(f, "shutdown trigger: {}", event),
            MainEvent::DiagnosticsPort(event) => write!(f, "diagnostics port: {}", event),
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
            MainEvent::BlockCompleteConfirmationRequest(req) => {
                write!(f, "mark block completed request: {}", req)
            }
            MainEvent::BlockHeaderFetcherRequest(request) => {
                write!(f, "block header fetcher request: {}", request)
            }
            MainEvent::TrieOrChunkFetcherRequest(request) => {
                write!(f, "trie or chunk fetcher request: {}", request)
            }
            MainEvent::BlockExecutionResultsOrChunkFetcherRequest(request) => {
                write!(
                    f,
                    "block execution results or chunk fetcher request: {}",
                    request
                )
            }
            MainEvent::LegacyDeployFetcherRequest(request) => {
                write!(f, "legacy deploy fetcher request: {}", request)
            }
            MainEvent::DeployFetcherRequest(request) => {
                write!(f, "deploy fetcher request: {}", request)
            }
            MainEvent::FinalitySignatureFetcherRequest(request) => {
                write!(f, "finality signature fetcher request: {}", request)
            }
            MainEvent::SyncLeapFetcherRequest(request) => {
                write!(f, "sync leap fetcher request: {}", request)
            }
            MainEvent::ApprovalsHashesFetcherRequest(request) => {
                write!(f, "approvals hashes fetcher request: {}", request)
            }
            MainEvent::AddressGossiperCrank(request) => {
                write!(f, "begin address gossip request: {}", request)
            }
            MainEvent::DeployBufferRequest(req) => {
                write!(f, "deploy buffer request: {}", req)
            }
            MainEvent::BlockValidatorRequest(req) => {
                write!(f, "block validator request: {}", req)
            }
            MainEvent::MetricsRequest(req) => write!(f, "metrics request: {}", req),
            MainEvent::ControlAnnouncement(ctrl_ann) => write!(f, "control: {}", ctrl_ann),
            MainEvent::FatalAnnouncement(fatal_ann) => write!(f, "fatal: {}", fatal_ann),
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
            MainEvent::FinalitySignatureGossiperAnnouncement(ann) => {
                write!(f, "block signature gossiper announcement: {}", ann)
            }
            MainEvent::AddressGossiperAnnouncement(ann) => {
                write!(f, "address gossiper announcement: {}", ann)
            }
            MainEvent::DeployBufferAnnouncement(ann) => {
                write!(f, "deploy buffer announcement: {}", ann)
            }
            MainEvent::UpgradeWatcherAnnouncement(ann) => {
                write!(f, "chainspec loader announcement: {}", ann)
            }
            MainEvent::NetworkPeerBehaviorAnnouncement(ann) => {
                write!(f, "blocklist announcement: {}", ann)
            }
            MainEvent::ConsensusMessageIncoming(inner) => Display::fmt(inner, f),
            MainEvent::ConsensusDemand(inner) => Display::fmt(inner, f),
            MainEvent::DeployGossiperIncoming(inner) => Display::fmt(inner, f),
            MainEvent::FinalitySignatureGossiperIncoming(inner) => Display::fmt(inner, f),
            MainEvent::AddressGossiperIncoming(inner) => Display::fmt(inner, f),
            MainEvent::NetworkPeerRequestingData(inner) => Display::fmt(inner, f),
            MainEvent::NetworkPeerProvidingData(inner) => Display::fmt(inner, f),
            MainEvent::TrieRequestIncoming(inner) => Display::fmt(inner, f),
            MainEvent::TrieDemand(inner) => Display::fmt(inner, f),
            MainEvent::TrieResponseIncoming(inner) => Display::fmt(inner, f),
            MainEvent::FinalitySignatureIncoming(inner) => Display::fmt(inner, f),
            MainEvent::ContractRuntime(inner) => Display::fmt(inner, f),
            MainEvent::BlockGossiper(inner) => Display::fmt(inner, f),
            MainEvent::BlockGossiperIncoming(inner) => Display::fmt(inner, f),
            MainEvent::BlockGossiperAnnouncement(inner) => Display::fmt(inner, f),
            MainEvent::BlockFetcher(inner) => Display::fmt(inner, f),
            MainEvent::BlockFetcherRequest(inner) => Display::fmt(inner, f),
            MainEvent::SetNodeStopRequest(inner) => Display::fmt(inner, f),
            MainEvent::MainReactorRequest(inner) => Display::fmt(inner, f),
            MainEvent::MakeBlockExecutableRequest(inner) => Display::fmt(inner, f),
            MainEvent::MetaBlockAnnouncement(inner) => Display::fmt(inner, f),
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

impl From<NetworkRequest<gossiper::Message<Block>>> for MainEvent {
    fn from(request: NetworkRequest<gossiper::Message<Block>>) -> Self {
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
