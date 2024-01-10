use std::{
    fmt::{self, Display, Formatter},
    sync::Arc,
    time::Duration,
};

use derive_more::From;
use futures::channel::oneshot;
use prometheus::Registry;
use serde::Serialize;
use thiserror::Error as ThisError;

use casper_types::{
    binary_port::{binary_request::BinaryRequest, get::GetRequest, ErrorCode},
    testing::TestRng,
    Chainspec, ChainspecRawBytes, Digest, KeyTag,
};

use crate::{
    components::{
        binary_port::{config::Config as BinaryPortConfig, event::Event as BinaryPortEvent},
        network::Identity as NetworkIdentity,
        Component, InitializedComponent,
    },
    effect::{
        announcements::ControlAnnouncement,
        requests::{
            AcceptTransactionRequest, BlockSynchronizerRequest, ChainspecRawBytesRequest,
            ConsensusRequest, ContractRuntimeRequest, NetworkInfoRequest, ReactorInfoRequest,
            StorageRequest, UpgradeWatcherRequest,
        },
        EffectBuilder, EffectExt, Effects, Responder,
    },
    reactor::{self, EventQueueHandle, QueueKind, Reactor, ReactorEvent, Runner, Scheduler},
    testing::{network::NetworkedReactor, ConditionCheckReactor},
    types::NodeRng,
    utils::{self, Loadable},
};

use super::BinaryPort;

/// Top-level event for the reactor.
#[derive(Debug, From, Serialize)]
#[must_use]
enum Event {
    #[from]
    BinaryPort(#[serde(skip_serializing)] BinaryPortEvent),
    #[from]
    ControlAnnouncement(ControlAnnouncement),
    #[from]
    ContractRuntimeRequest(ContractRuntimeRequest),
}

impl From<ChainspecRawBytesRequest> for Event {
    fn from(_request: ChainspecRawBytesRequest) -> Self {
        unreachable!()
    }
}

impl From<UpgradeWatcherRequest> for Event {
    fn from(_request: UpgradeWatcherRequest) -> Self {
        unreachable!()
    }
}

impl From<BlockSynchronizerRequest> for Event {
    fn from(_request: BlockSynchronizerRequest) -> Self {
        unreachable!()
    }
}

impl From<ConsensusRequest> for Event {
    fn from(_request: ConsensusRequest) -> Self {
        unreachable!()
    }
}

impl From<ReactorInfoRequest> for Event {
    fn from(_request: ReactorInfoRequest) -> Self {
        unreachable!()
    }
}

impl From<NetworkInfoRequest> for Event {
    fn from(_request: NetworkInfoRequest) -> Self {
        unreachable!()
    }
}

impl From<AcceptTransactionRequest> for Event {
    fn from(_request: AcceptTransactionRequest) -> Self {
        unreachable!()
    }
}

impl From<StorageRequest> for Event {
    fn from(_request: StorageRequest) -> Self {
        unreachable!()
    }
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::ControlAnnouncement(ctrl_ann) => write!(formatter, "control: {}", ctrl_ann),
            Event::BinaryPort(request) => write!(formatter, "binary port request: {:?}", request),
            Event::ContractRuntimeRequest(request) => {
                write!(formatter, "contract runtime request: {:?}", request)
            }
        }
    }
}

impl ReactorEvent for Event {
    fn is_control(&self) -> bool {
        matches!(self, Event::ControlAnnouncement(_))
    }

    fn try_into_control(self) -> Option<ControlAnnouncement> {
        if let Self::ControlAnnouncement(ctrl_ann) = self {
            Some(ctrl_ann)
        } else {
            None
        }
    }
}

/// Error type returned by the test reactor.
#[derive(Debug, ThisError)]
enum ReactorError {
    #[error("prometheus (metrics) error: {0}")]
    Metrics(#[from] prometheus::Error),
}

struct MockReactor {
    binary_port: BinaryPort,
    scheduler: &'static Scheduler<Event>,
}

impl MockReactor {
    fn new(config: BinaryPortConfig) -> Self {
        let registry = Registry::new();
        let mut binary_port = BinaryPort::new(config, &registry).unwrap();
        <BinaryPort as InitializedComponent<Event>>::start_initialization(&mut binary_port);
        Self {
            binary_port,
            scheduler: utils::leak(Scheduler::new(QueueKind::weights(), None)),
        }
    }
}

#[tokio::test]
async fn should_respect_function_disabled_flag_for_get_all_values() {
    let mut rng = TestRng::new();

    const ENABLED: bool = true;
    const DISABLED: bool = false;
    const EXPECTED_ERROR_CODE: ErrorCode = ErrorCode::FunctionDisabled;

    let config = BinaryPortConfig {
        enable_server: true,
        allow_request_get_all_values: DISABLED,
        allow_request_get_trie: ENABLED,
        max_request_size_bytes: 1024,
        max_response_size_bytes: 1024,
        client_request_limit: 2,
        client_request_buffer_size: 16,
        max_connections: 2,
        ..Default::default()
    };

    let (sender, receiver) = oneshot::channel();
    let event = super::Event::HandleRequest {
        request: all_values_request(),
        responder: Responder::without_shutdown(sender),
    };

    let mut reactor = MockReactor::new(config.clone());
    let effect_builder = EffectBuilder::new(EventQueueHandle::without_shutdown(reactor.scheduler));

    let initialize_event = super::Event::Initialize;
    let effects = reactor
        .binary_port
        .handle_event(effect_builder, &mut rng, initialize_event);
    assert_eq!(0, effects.len());

    let mut effects = reactor
        .binary_port
        .handle_event(effect_builder, &mut rng, event);
    assert_eq!(1, effects.len());
    tokio::spawn(effects.remove(0));
    let result = receiver.await.unwrap();
    assert_eq!(result.error_code(), EXPECTED_ERROR_CODE as u8);
}

#[tokio::test]
async fn should_respect_function_disabled_flag_for_get_trie() {
    let mut rng = TestRng::new();

    const ENABLED: bool = true;
    const DISABLED: bool = false;
    const EXPECTED_ERROR_CODE: ErrorCode = ErrorCode::FunctionDisabled;

    let config = BinaryPortConfig {
        enable_server: true,
        allow_request_get_all_values: ENABLED,
        allow_request_get_trie: DISABLED,
        max_request_size_bytes: 1024,
        max_response_size_bytes: 1024,
        client_request_limit: 2,
        client_request_buffer_size: 16,
        max_connections: 2,
        ..Default::default()
    };

    let (sender, receiver) = oneshot::channel();
    let event = super::Event::HandleRequest {
        request: trie_request(),
        responder: Responder::without_shutdown(sender),
    };

    let mut reactor = MockReactor::new(config.clone());
    let effect_builder = EffectBuilder::new(EventQueueHandle::without_shutdown(reactor.scheduler));

    let initialize_event = super::Event::Initialize;
    let effects = reactor
        .binary_port
        .handle_event(effect_builder, &mut rng, initialize_event);
    assert_eq!(0, effects.len());

    let mut effects = reactor
        .binary_port
        .handle_event(effect_builder, &mut rng, event);
    assert_eq!(1, effects.len());
    tokio::spawn(effects.remove(0));
    let result = receiver.await.unwrap();
    assert_eq!(result.error_code(), EXPECTED_ERROR_CODE as u8);
}

struct MockReactor1 {
    binary_port: BinaryPort,
    //scheduler: &'static Scheduler<Event>,
}

impl NetworkedReactor for MockReactor1 {}

impl Reactor for MockReactor1 {
    type Event = Event;
    type Config = BinaryPortConfig;
    type Error = ReactorError;

    fn new(
        config: Self::Config,
        _chainspec: Arc<Chainspec>,
        _chainspec_raw_bytes: Arc<ChainspecRawBytes>,
        _network_identity: NetworkIdentity,
        registry: &Registry,
        _event_queue: EventQueueHandle<Self::Event>,
        _rng: &mut NodeRng,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error> {
        let mut binary_port = BinaryPort::new(config, registry).unwrap();
        <BinaryPort as InitializedComponent<Event>>::start_initialization(&mut binary_port);

        let reactor = MockReactor1 { binary_port };

        let effects = Effects::new();

        Ok((reactor, effects))
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut NodeRng,
        event: Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::BinaryPort(event) => reactor::wrap_effects(
                Event::BinaryPort,
                self.binary_port.handle_event(effect_builder, rng, event),
            ),
            Event::ControlAnnouncement(_) => panic!("unexpected control announcement"),
            Event::ContractRuntimeRequest(_) => {
                // We're only interested if the binary port actually created a request to Contract
                // Runtime component, but we're not interested in the result.
                Effects::new()
            }
        }
    }
}

fn got_contract_runtime_request(event: &Event) -> bool {
    match event {
        Event::BinaryPort(_) => false,
        Event::ControlAnnouncement(_) => false,
        Event::ContractRuntimeRequest(_) => true,
    }
}

#[tokio::test]
async fn should_respect_function_enabled_flag_for_get_all_values() {
    let mut rng = TestRng::new();

    const ENABLED: bool = true;
    const DISABLED: bool = false;
    const EXPECTED_ERROR_CODE: ErrorCode = ErrorCode::FunctionDisabled;

    let config = BinaryPortConfig {
        enable_server: true,
        allow_request_get_all_values: ENABLED,
        allow_request_get_trie: DISABLED,
        max_request_size_bytes: 1024,
        max_response_size_bytes: 1024,
        client_request_limit: 2,
        client_request_buffer_size: 16,
        max_connections: 2,
        ..Default::default()
    };

    let (chainspec, chainspec_raw_bytes) =
        <(Chainspec, ChainspecRawBytes)>::from_resources("local");
    let mut runner: Runner<ConditionCheckReactor<MockReactor1>> = Runner::new(
        config.clone(),
        Arc::new(chainspec),
        Arc::new(chainspec_raw_bytes),
        &mut rng,
    )
    .await
    .unwrap();

    // Initialize component.
    runner
        .process_injected_effects(|effect_builder| {
            effect_builder
                .into_inner()
                .schedule(super::Event::Initialize, QueueKind::Api)
                .ignore()
        })
        .await;

    let (sender, _receiver) = oneshot::channel();
    let event = super::Event::HandleRequest {
        request: all_values_request(),
        responder: Responder::without_shutdown(sender),
    };

    runner
        .process_injected_effects(|effect_builder| {
            effect_builder
                .into_inner()
                .schedule(event, QueueKind::Api)
                .ignore()
        })
        .await;

    runner
        .crank_until(
            &mut rng,
            got_contract_runtime_request,
            Duration::from_secs(10),
        )
        .await;
}

#[tokio::test]
async fn should_respect_function_enabled_flag_for_get_trie() {
    let mut rng = TestRng::new();

    const ENABLED: bool = true;
    const DISABLED: bool = false;
    const EXPECTED_ERROR_CODE: ErrorCode = ErrorCode::FunctionDisabled;

    let config = BinaryPortConfig {
        enable_server: true,
        allow_request_get_all_values: DISABLED,
        allow_request_get_trie: ENABLED,
        max_request_size_bytes: 1024,
        max_response_size_bytes: 1024,
        client_request_limit: 2,
        client_request_buffer_size: 16,
        max_connections: 2,
        ..Default::default()
    };

    let (chainspec, chainspec_raw_bytes) =
        <(Chainspec, ChainspecRawBytes)>::from_resources("local");
    let mut runner: Runner<ConditionCheckReactor<MockReactor1>> = Runner::new(
        config.clone(),
        Arc::new(chainspec),
        Arc::new(chainspec_raw_bytes),
        &mut rng,
    )
    .await
    .unwrap();

    // Initialize component.
    runner
        .process_injected_effects(|effect_builder| {
            effect_builder
                .into_inner()
                .schedule(super::Event::Initialize, QueueKind::Api)
                .ignore()
        })
        .await;

    let (sender, _receiver) = oneshot::channel();
    let event = super::Event::HandleRequest {
        request: trie_request(),
        responder: Responder::without_shutdown(sender),
    };

    runner
        .process_injected_effects(|effect_builder| {
            effect_builder
                .into_inner()
                .schedule(event, QueueKind::Api)
                .ignore()
        })
        .await;

    runner
        .crank_until(
            &mut rng,
            got_contract_runtime_request,
            Duration::from_secs(10),
        )
        .await;
}

fn all_values_request() -> BinaryRequest {
    BinaryRequest::Get(GetRequest::AllValues {
        state_root_hash: Digest::hash([1u8; 32]),
        key_tag: KeyTag::Account,
    })
}

fn trie_request() -> BinaryRequest {
    BinaryRequest::Get(GetRequest::Trie {
        trie_key: Digest::hash([1u8; 32]),
    })
}
