use futures::channel::oneshot;
use prometheus::Registry;

use casper_types::{binary_port::ErrorCode, testing::TestRng};

use crate::{
    components::{
        binary_port::{
            config::Config as BinaryPortConfig,
            event::Event as BinaryPortEvent,
            tests::{all_values_request, trie_request},
        },
        Component, InitializedComponent,
    },
    effect::{EffectBuilder, Responder},
    reactor::{EventQueueHandle, QueueKind, Scheduler},
    utils,
};

use super::{BinaryPort, Event};

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
    let event = BinaryPortEvent::HandleRequest {
        request: all_values_request(),
        responder: Responder::without_shutdown(sender),
    };

    let mut reactor = MockReactor::new(config.clone());
    let effect_builder = EffectBuilder::new(EventQueueHandle::without_shutdown(reactor.scheduler));

    let initialize_event = BinaryPortEvent::Initialize;
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
    let event = BinaryPortEvent::HandleRequest {
        request: trie_request(),
        responder: Responder::without_shutdown(sender),
    };

    let mut reactor = MockReactor::new(config.clone());
    let effect_builder = EffectBuilder::new(EventQueueHandle::without_shutdown(reactor.scheduler));

    let initialize_event = BinaryPortEvent::Initialize;
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
