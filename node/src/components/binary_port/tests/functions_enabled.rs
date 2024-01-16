use std::{sync::Arc, time::Duration};

use futures::channel::oneshot;
use prometheus::Registry;
use thiserror::Error as ThisError;

use casper_types::{binary_port::ErrorCode, testing::TestRng, Chainspec, ChainspecRawBytes};

use crate::{
    components::{
        binary_port::{
            config::Config as BinaryPortConfig,
            event::Event as BinaryPortEvent,
            tests::{all_values_request, trie_request},
        },
        network::Identity as NetworkIdentity,
        Component, InitializedComponent,
    },
    effect::{EffectBuilder, EffectExt, Effects, Responder},
    reactor::{self, EventQueueHandle, QueueKind, Reactor, Runner},
    testing::{network::NetworkedReactor, ConditionCheckReactor},
    types::NodeRng,
    utils::Loadable,
};

use super::{BinaryPort, Event};

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
    let mut runner: Runner<ConditionCheckReactor<MockReactor>> = Runner::new(
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
                .schedule(BinaryPortEvent::Initialize, QueueKind::Api)
                .ignore()
        })
        .await;

    let (sender, _receiver) = oneshot::channel();
    let event = BinaryPortEvent::HandleRequest {
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
    let mut runner: Runner<ConditionCheckReactor<MockReactor>> = Runner::new(
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
                .schedule(BinaryPortEvent::Initialize, QueueKind::Api)
                .ignore()
        })
        .await;

    let (sender, _receiver) = oneshot::channel();
    let event = BinaryPortEvent::HandleRequest {
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

/// Error type returned by the test reactor.
#[derive(Debug, ThisError)]
enum ReactorError {
    #[error("prometheus (metrics) error: {0}")]
    Metrics(#[from] prometheus::Error),
}
