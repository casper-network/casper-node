use std::fmt::{self, Display, Formatter};

use derive_more::From;
use rand::Rng;
use serde::Serialize;

use casper_types::{
    binary_port::{BinaryRequest, BinaryResponse, GetRequest, GlobalStateRequest},
    Digest, GlobalStateIdentifier, KeyTag,
};

use crate::{
    components::binary_port::event::Event as BinaryPortEvent,
    effect::{
        announcements::ControlAnnouncement,
        requests::{
            AcceptTransactionRequest, BlockSynchronizerRequest, ChainspecRawBytesRequest,
            ConsensusRequest, ContractRuntimeRequest, NetworkInfoRequest, ReactorInfoRequest,
            StorageRequest, UpgradeWatcherRequest,
        },
    },
    reactor::ReactorEvent,
};
use std::{sync::Arc, time::Duration};

use futures::channel::oneshot::{self, Receiver};
use prometheus::Registry;
use thiserror::Error as ThisError;

use casper_types::{
    binary_port::ErrorCode, testing::TestRng, Chainspec, ChainspecRawBytes, ProtocolVersion,
};

use crate::{
    components::{
        binary_port::config::Config as BinaryPortConfig, network::Identity as NetworkIdentity,
        Component, InitializedComponent,
    },
    effect::{EffectBuilder, EffectExt, Effects, Responder},
    reactor::{self, EventQueueHandle, QueueKind, Reactor, Runner},
    testing::{network::NetworkedReactor, ConditionCheckReactor},
    types::NodeRng,
    utils::Loadable,
};

use super::BinaryPort;

const ENABLED: bool = true;
const DISABLED: bool = false;

struct TestCase {
    allow_request_get_all_values: bool,
    allow_request_get_trie: bool,
    request_generator: fn() -> BinaryRequest,
}

#[tokio::test]
async fn should_execute_enabled_functions() {
    let mut rng = TestRng::new();

    let get_all_values_enabled = TestCase {
        allow_request_get_all_values: ENABLED,
        allow_request_get_trie: rng.gen(),
        request_generator: all_values_request,
    };

    let get_trie_enabled = TestCase {
        allow_request_get_all_values: rng.gen(),
        allow_request_get_trie: ENABLED,
        request_generator: trie_request,
    };

    for test_case in [get_all_values_enabled, get_trie_enabled] {
        let (_, mut runner) = run_test_case(test_case, &mut rng).await;

        runner
            .crank_until(
                &mut rng,
                got_contract_runtime_request,
                Duration::from_secs(10),
            )
            .await;
    }
}

#[tokio::test]
async fn should_return_error_for_disabled_functions() {
    let mut rng = TestRng::new();

    const EXPECTED_ERROR_CODE: ErrorCode = ErrorCode::FunctionDisabled;

    let get_all_values_disabled = TestCase {
        allow_request_get_all_values: DISABLED,
        allow_request_get_trie: rng.gen(),
        request_generator: all_values_request,
    };

    let get_trie_disabled = TestCase {
        allow_request_get_all_values: rng.gen(),
        allow_request_get_trie: DISABLED,
        request_generator: trie_request,
    };

    for test_case in [get_all_values_disabled, get_trie_disabled] {
        let (receiver, mut runner) = run_test_case(test_case, &mut rng).await;

        let result = tokio::select! {
            result = receiver => result.expect("expected successful response"),
            _ = runner.crank_until(
                &mut rng,
                got_contract_runtime_request,
                Duration::from_secs(10),
            ) => {
                panic!("expected receiver to complete first")
            }
        };
        assert_eq!(result.error_code(), EXPECTED_ERROR_CODE as u8)
    }
}

async fn run_test_case(
    TestCase {
        allow_request_get_all_values,
        allow_request_get_trie,
        request_generator,
    }: TestCase,
    rng: &mut TestRng,
) -> (
    Receiver<BinaryResponse>,
    Runner<ConditionCheckReactor<MockReactor>>,
) {
    let config = BinaryPortConfig {
        enable_server: true,
        allow_request_get_all_values,
        allow_request_get_trie,
        max_request_size_bytes: 1024,
        max_response_size_bytes: 1024,
        client_request_limit: 2,
        client_request_buffer_size: 16,
        max_connections: 2,
        ..Default::default()
    };

    let (chainspec, chainspec_raw_bytes) =
        <(Chainspec, ChainspecRawBytes)>::from_resources("local");
    let mut runner: Runner<ConditionCheckReactor<MockReactor>> = Runner::new(
        config.clone(),
        Arc::new(chainspec),
        Arc::new(chainspec_raw_bytes),
        rng,
    )
    .await
    .unwrap();

    // Initialize component.
    runner
        .process_injected_effects(|effect_builder| {
            effect_builder
                .into_inner()
                .schedule(BinaryPortEvent::Initialize, QueueKind::Api)
                .ignore()
        })
        .await;

    let (sender, receiver) = oneshot::channel();
    let event = BinaryPortEvent::HandleRequest {
        request: request_generator(),
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

    (receiver, runner)
}

struct MockReactor {
    binary_port: BinaryPort,
}

impl NetworkedReactor for MockReactor {}

impl Reactor for MockReactor {
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

        let reactor = MockReactor { binary_port };

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
            Event::ReactorInfoRequest(ReactorInfoRequest::ProtocolVersion { responder }) => {
                responder.respond(ProtocolVersion::V1_0_0).ignore()
            }
            Event::ContractRuntimeRequest(_) | Event::ReactorInfoRequest(_) => {
                // We're only interested if the binary port actually created a request to Contract
                // Runtime component, but we're not interested in the result.
                Effects::new()
            }
        }
    }
}

/// Error type returned by the test reactor.
#[derive(Debug, ThisError)]
enum ReactorError {
    #[error("prometheus (metrics) error: {0}")]
    Metrics(#[from] prometheus::Error),
}

/// Top-level event for the test reactors.
#[derive(Debug, From, Serialize)]
#[must_use]
enum Event {
    #[from]
    BinaryPort(#[serde(skip_serializing)] BinaryPortEvent),
    #[from]
    ControlAnnouncement(ControlAnnouncement),
    #[from]
    ContractRuntimeRequest(ContractRuntimeRequest),
    #[from]
    ReactorInfoRequest(ReactorInfoRequest),
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
            Event::ReactorInfoRequest(request) => {
                write!(formatter, "reactor info request: {:?}", request)
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

fn all_values_request() -> BinaryRequest {
    let state_identifier = GlobalStateIdentifier::StateRootHash(Digest::hash([1u8; 32]));
    BinaryRequest::Get(GetRequest::State(GlobalStateRequest::AllItems {
        state_identifier: Some(state_identifier),
        key_tag: KeyTag::Account,
    }))
}

fn trie_request() -> BinaryRequest {
    BinaryRequest::Get(GetRequest::State(GlobalStateRequest::Trie {
        trie_key: Digest::hash([1u8; 32]),
    }))
}

fn got_contract_runtime_request(event: &Event) -> bool {
    matches!(event, Event::ContractRuntimeRequest(_))
}
