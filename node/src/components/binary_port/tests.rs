use std::fmt::{self, Display, Formatter};

use derive_more::From;
use either::Either;
use rand::Rng;
use serde::Serialize;

use casper_binary_port::{
    BinaryResponse, Command, GetRequest, GlobalStateEntityQualifier, GlobalStateRequest, RecordId,
};

use casper_types::{
    BlockHeader, BlockIdentifier, Digest, GlobalStateIdentifier, KeyTag, ProtocolVersion,
    PublicKey, Timestamp, Transaction, TransactionV1,
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

use casper_binary_port::ErrorCode;
use casper_types::{testing::TestRng, Chainspec, ChainspecRawBytes};

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

use super::{BinaryPort, Metrics as BinaryPortMetrics};

const ENABLED: bool = true;
const DISABLED: bool = false;

#[tokio::test]
async fn staged_protocol_versions_returns_a_snapshot_or_binary_error() {
    use crate::{
        reactor::{main_reactor::MainEvent, Scheduler},
        utils,
    };
    use casper_binary_port::{InformationRequest, ResponseType};
    use casper_types::bytesrepr::FromBytes;

    for versions in [
        Ok(Vec::new()),
        Ok(vec![
            ProtocolVersion::from_parts(2, 2, 0),
            ProtocolVersion::from_parts(2, 3, 0),
        ]),
        Err("unreadable installed chainspec".to_string()),
    ] {
        let scheduler = utils::leak(Scheduler::<MainEvent>::new(QueueKind::weights(), None));
        let effect_builder = EffectBuilder::new(EventQueueHandle::without_shutdown(scheduler));
        let expected = versions.clone();
        let request = super::handle_info_request(
            InformationRequest::StagedProtocolVersions,
            effect_builder,
            ProtocolVersion::from_parts(2, 1, 0),
        );
        let respond = async {
            let ((_, event), _) = scheduler.pop().await;
            let MainEvent::UpgradeWatcherRequest(UpgradeWatcherRequest::StagedProtocolVersions(
                responder,
            )) = event
            else {
                panic!("unexpected staged protocol versions request: {event:?}");
            };
            responder.respond(versions).await;
        };
        let (response, ()) = tokio::time::timeout(Duration::from_secs(5), async {
            futures::join!(request, respond)
        })
        .await
        .expect("staged protocol versions request should complete");
        match expected {
            Ok(expected) => {
                assert!(response.is_success());
                assert_eq!(
                    response.returned_data_type_tag(),
                    Some(ResponseType::StagedProtocolVersions as u8)
                );
                let (actual, remainder) =
                    Vec::<ProtocolVersion>::from_bytes(response.payload()).unwrap();
                assert!(remainder.is_empty());
                assert_eq!(actual, expected);
            }
            Err(_) => {
                assert_eq!(response.error_code(), ErrorCode::InternalError as u16);
                assert!(response.payload().is_empty());
            }
        }
    }
}

#[tokio::test]
async fn future_chainspec_returns_the_requested_documents_or_binary_error() {
    use crate::{
        reactor::{main_reactor::MainEvent, Scheduler},
        utils,
    };
    use casper_binary_port::{InformationRequest, ResponseType};
    use casper_types::bytesrepr::FromBytes;

    for version in [
        ProtocolVersion::from_parts(2, 2, 0),
        ProtocolVersion::from_parts(2, 3, 0),
    ] {
        let source = ChainspecRawBytes::new(
            format!("# source documents for {version}")
                .into_bytes()
                .into(),
            Some(b"# accounts".to_vec().into()),
            Some(b"# global state".to_vec().into()),
        );
        for metadata in [
            Ok(None),
            Ok(Some(source)),
            Err("unreadable installed chainspec".to_string()),
        ] {
            let scheduler = utils::leak(Scheduler::<MainEvent>::new(QueueKind::weights(), None));
            let effect_builder = EffectBuilder::new(EventQueueHandle::without_shutdown(scheduler));
            let expected = metadata.clone();
            let metadata = metadata.map(|source| source.map(Arc::new));
            let request = super::handle_info_request(
                InformationRequest::FutureChainspec {
                    protocol_version: version,
                },
                effect_builder,
                ProtocolVersion::from_parts(2, 1, 0),
            );
            let respond = async {
                let ((_, event), _) = scheduler.pop().await;
                match event {
                    MainEvent::UpgradeWatcherRequest(UpgradeWatcherRequest::FutureChainspec {
                        protocol_version,
                        responder,
                    }) => {
                        assert_eq!(protocol_version, version);
                        responder.respond(metadata).await;
                    }
                    other => panic!("unexpected future chainspec request: {other:?}"),
                }
            };
            let (response, ()) = tokio::time::timeout(Duration::from_secs(5), async {
                futures::join!(request, respond)
            })
            .await
            .expect("future chainspec request should complete");
            match expected {
                Ok(Some(expected)) => {
                    assert!(response.is_success());
                    assert_eq!(
                        response.returned_data_type_tag(),
                        Some(ResponseType::ChainspecRawBytes as u8)
                    );
                    let (actual, remainder) =
                        ChainspecRawBytes::from_bytes(response.payload()).unwrap();
                    assert!(remainder.is_empty());
                    assert_eq!(actual, expected);
                }
                Ok(None) => {
                    assert!(response.is_success());
                    assert_eq!(response.returned_data_type_tag(), None);
                    assert!(response.payload().is_empty());
                }
                Err(_) => {
                    assert!(!response.is_success());
                    assert_eq!(response.error_code(), ErrorCode::InternalError as u16);
                    assert!(response.payload().is_empty());
                }
            }
        }
    }
}

#[tokio::test]
async fn oversized_future_chainspec_returns_an_error_and_keeps_connection_open() {
    use casper_binary_port::{
        BinaryMessage, BinaryMessageCodec, BinaryResponseAndRequest, CommandHeader,
        InformationRequest,
    };
    use casper_types::bytesrepr::{self, Bytes, ToBytes};
    use futures::{SinkExt, StreamExt};
    use tokio::{
        net::{TcpListener, TcpStream},
        sync::Mutex,
    };
    use tokio_util::codec::{Encoder, Framed};

    use crate::{
        reactor::{main_reactor::MainEvent, Scheduler},
        utils,
    };

    let version = ProtocolVersion::from_parts(2, 2, 2);
    let source = include_bytes!("../../../../resources/evm/chainspec.toml").to_vec();
    let snapshot = |source: Vec<u8>| ChainspecRawBytes::new(source.into(), None, None);
    let request_bytes = |info: InformationRequest, id| {
        let command = Command::Get(info.try_into().unwrap());
        let mut payload = CommandHeader::new(command.tag(), id).to_bytes().unwrap();
        command.write_bytes(&mut payload).unwrap();
        payload
    };
    let mut echoed_request = bytes::BytesMut::new();
    BinaryMessageCodec::new(u32::MAX)
        .encode(
            BinaryMessage::new(request_bytes(
                InformationRequest::FutureChainspec {
                    protocol_version: version,
                },
                1,
            )),
            &mut echoed_request,
        )
        .unwrap();
    let response = BinaryResponse::from_value(snapshot(source.clone()));
    let max_message_size_bytes =
        BinaryResponseAndRequest::new(response, Bytes::from(echoed_request.to_vec()))
            .serialized_length() as u32;

    // The second response exceeds the limit by one byte, although its payload alone fits.
    let mut larger_source = source.clone();
    larger_source.push(b' ');
    let replies = [
        BinaryResponse::from_value(snapshot(source)),
        BinaryResponse::from_value(snapshot(larger_source)),
        BinaryResponse::from_value(version),
    ];
    assert!(replies[1].payload().len() < max_message_size_bytes as usize);
    let config = Arc::new(BinaryPortConfig {
        max_message_size_bytes,
        ..Default::default()
    });
    let scheduler = utils::leak(Scheduler::<MainEvent>::new(QueueKind::weights(), None));
    let effect_builder = EffectBuilder::new(EventQueueHandle::without_shutdown(scheduler));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (client, server) = futures::join!(
        TcpStream::connect(listener.local_addr().unwrap()),
        listener.accept(),
    );
    let server = super::handle_client_loop(
        server.unwrap().0,
        effect_builder,
        config.clone(),
        Arc::new(Mutex::new(
            super::RateLimiter::new(config.qps_limit, casper_types::TimeDiff::from_seconds(1))
                .unwrap(),
        )),
        super::ConnectionTerminator::new(),
        super::BinaryRequestTerminationDelayValues::from_config(&config),
    );
    let respond = async {
        for reply in replies {
            let ((_, event), _) = scheduler.pop().await;
            let MainEvent::BinaryPort(BinaryPortEvent::HandleRequest { responder, .. }) = event
            else {
                panic!("unexpected binary port event: {event:?}");
            };
            responder.respond(reply).await;
        }
    };
    let client = async {
        let mut framed = Framed::new(
            client.unwrap(),
            BinaryMessageCodec::new(max_message_size_bytes),
        );
        for (id, info) in [
            (
                1,
                InformationRequest::FutureChainspec {
                    protocol_version: version,
                },
            ),
            (
                2,
                InformationRequest::FutureChainspec {
                    protocol_version: version,
                },
            ),
            (3, InformationRequest::ProtocolVersion),
        ] {
            let payload = request_bytes(info, id);
            let mut expected_request = bytes::BytesMut::new();
            BinaryMessageCodec::new(max_message_size_bytes)
                .encode(BinaryMessage::new(payload.clone()), &mut expected_request)
                .unwrap();
            framed.send(BinaryMessage::new(payload)).await.unwrap();
            let message = framed
                .next()
                .await
                .expect("connection must stay open")
                .unwrap();
            let response: BinaryResponseAndRequest =
                bytesrepr::deserialize_from_slice(message.payload()).unwrap();
            assert_eq!(response.request(), &expected_request[..]);
            if id == 2 {
                assert_eq!(response.error_code(), ErrorCode::ResponseTooLarge as u16);
                assert!(!response.is_success());
                assert!(response.response().payload().is_empty());
            } else {
                assert!(response.is_success());
                if id == 3 {
                    assert_eq!(
                        bytesrepr::deserialize_from_slice::<_, ProtocolVersion>(
                            response.response().payload()
                        )
                        .unwrap(),
                        version
                    );
                }
            }
        }
    };
    let (server_result, (), ()) = tokio::time::timeout(Duration::from_secs(5), async {
        futures::join!(server, respond, client)
    })
    .await
    .expect("binary requests should complete");
    server_result.unwrap();
}

/// Runs `handle_client_loop` against `peer`, answering every request with the protocol
/// version. Returns the server's result and how long the connection stayed open.
async fn run_peer_test<F, Fut>(
    config: BinaryPortConfig,
    peer: F,
) -> (Result<(), super::Error>, Duration)
where
    F: FnOnce(tokio::net::TcpStream) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    run_peer_test_with_reply(
        config,
        || BinaryResponse::from_value(ProtocolVersion::V2_0_0),
        peer,
    )
    .await
}

/// Like [`run_peer_test`], answering every request with `reply()`.
async fn run_peer_test_with_reply<F, Fut>(
    config: BinaryPortConfig,
    reply: impl Fn() -> BinaryResponse,
    peer: F,
) -> (Result<(), super::Error>, Duration)
where
    F: FnOnce(tokio::net::TcpStream) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    use tokio::{
        net::{TcpListener, TcpStream},
        sync::Mutex,
    };

    use crate::{
        reactor::{main_reactor::MainEvent, Scheduler},
        utils,
    };

    let config = Arc::new(config);
    let scheduler = utils::leak(Scheduler::<MainEvent>::new(QueueKind::weights(), None));
    let effect_builder = EffectBuilder::new(EventQueueHandle::without_shutdown(scheduler));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (client, server) = futures::join!(
        TcpStream::connect(listener.local_addr().unwrap()),
        listener.accept(),
    );
    let started = std::time::Instant::now();
    let server = super::handle_client_loop(
        server.unwrap().0,
        effect_builder,
        config.clone(),
        Arc::new(Mutex::new(
            super::RateLimiter::new(config.qps_limit, casper_types::TimeDiff::from_seconds(1))
                .unwrap(),
        )),
        super::ConnectionTerminator::new(),
        super::BinaryRequestTerminationDelayValues::from_config(&config),
    );
    let respond = async {
        loop {
            let ((_, event), _) = scheduler.pop().await;
            let MainEvent::BinaryPort(BinaryPortEvent::HandleRequest { responder, .. }) = event
            else {
                panic!("unexpected binary port event: {event:?}");
            };
            responder.respond(reply()).await;
        }
    };
    let server = async {
        let result = server.await;
        (result, started.elapsed())
    };
    // Peers are expected to run until the server closes the connection.
    let peer = peer(client.unwrap());
    tokio::time::timeout(Duration::from_secs(15), async {
        tokio::select! {
            ((result, elapsed), ()) = async { futures::join!(server, peer) } => (result, elapsed),
            _ = respond => unreachable!(),
        }
    })
    .await
    .expect("server should close the connection")
}

/// Reads until the server closes the connection. Returns `true` if it was closed gracefully.
async fn wait_until_closed(client: &mut tokio::net::TcpStream) -> bool {
    use tokio::io::AsyncReadExt;

    let mut buf = [0; 1024];
    loop {
        match client.read(&mut buf).await {
            Ok(0) => return true,
            Ok(_) => continue,
            Err(_) => return false,
        }
    }
}

async fn read_response(client: &mut tokio::net::TcpStream) -> Vec<u8> {
    use tokio::io::AsyncReadExt;

    let mut response_len = [0; 4];
    client.read_exact(&mut response_len).await.unwrap();
    let mut response = vec![0; u32::from_le_bytes(response_len) as usize];
    client.read_exact(&mut response).await.unwrap();
    response
}

fn encoded_protocol_version_request() -> Vec<u8> {
    use casper_binary_port::{
        BinaryMessage, BinaryMessageCodec, CommandHeader, InformationRequest,
    };
    use casper_types::bytesrepr::ToBytes;
    use tokio_util::codec::Encoder;

    let command = Command::Get(InformationRequest::ProtocolVersion.try_into().unwrap());
    let mut payload = CommandHeader::new(command.tag(), 1).to_bytes().unwrap();
    command.write_bytes(&mut payload).unwrap();
    let mut encoded = bytes::BytesMut::new();
    BinaryMessageCodec::new(u32::MAX)
        .encode(BinaryMessage::new(payload), &mut encoded)
        .unwrap();
    encoded.to_vec()
}

fn assert_dropped_as_incomplete(result: Result<(), super::Error>, elapsed: Duration) {
    assert!(
        matches!(result, Err(super::Error::TransferViolation(_))),
        "{result:?}"
    );
    assert!(elapsed >= Duration::from_secs(1), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
}

#[tokio::test]
async fn should_drop_peer_that_sends_only_frame_header() {
    use tokio::io::AsyncWriteExt;

    let config = BinaryPortConfig {
        initial_connection_lifetime: casper_types::TimeDiff::from_seconds(60),
        throughput_grace_period: casper_types::TimeDiff::from_seconds(1),
        ..Default::default()
    };
    let (result, elapsed) = run_peer_test(config, |mut client| async move {
        client.write_all(&1024u32.to_le_bytes()).await.unwrap();
        wait_until_closed(&mut client).await;
    })
    .await;
    // Throughput check kicks in right after the grace period.
    assert_dropped_as_incomplete(result, elapsed);
}

#[tokio::test]
async fn should_leave_stalled_message_to_connection_terminator_when_rate_check_disabled() {
    use tokio::io::AsyncWriteExt;

    let config = BinaryPortConfig {
        initial_connection_lifetime: casper_types::TimeDiff::from_seconds(3),
        throughput_grace_period: casper_types::TimeDiff::from_seconds(1),
        min_transfer_throughput_bytes_per_second: 0,
        ..Default::default()
    };
    let (result, elapsed) = run_peer_test(config, |mut client| async move {
        client.write_all(&1024u32.to_le_bytes()).await.unwrap();
        client.write_all(&[0; 16]).await.unwrap();
        wait_until_closed(&mut client).await;
    })
    .await;
    // No transfer guard; the connection lives out its lifetime.
    assert!(
        !matches!(result, Err(super::Error::TransferViolation(_))),
        "{result:?}"
    );
    assert!(elapsed >= Duration::from_secs(3), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
}

#[tokio::test]
async fn should_drop_peer_trickling_frame_body() {
    use tokio::io::AsyncWriteExt;

    let config = BinaryPortConfig {
        initial_connection_lifetime: casper_types::TimeDiff::from_seconds(60),
        throughput_grace_period: casper_types::TimeDiff::from_seconds(1),
        ..Default::default()
    };
    let (result, elapsed) = run_peer_test(config, |mut client| async move {
        client.write_all(&1024u32.to_le_bytes()).await.unwrap();
        // One byte every 200ms is always some progress, but far below the minimum rate.
        loop {
            tokio::time::sleep(Duration::from_millis(200)).await;
            if client.write_all(&[0]).await.is_err() {
                break;
            }
        }
        wait_until_closed(&mut client).await;
    })
    .await;
    assert_dropped_as_incomplete(result, elapsed);
}

#[tokio::test]
async fn should_leave_silent_connection_to_connection_terminator() {
    let config = BinaryPortConfig {
        initial_connection_lifetime: casper_types::TimeDiff::from_seconds(3),
        throughput_grace_period: casper_types::TimeDiff::from_seconds(1),
        ..Default::default()
    };
    let (result, elapsed) = run_peer_test(config, |mut client| async move {
        // Keep the socket open without sending anything.
        assert!(wait_until_closed(&mut client).await);
    })
    .await;
    // Closed gracefully by the terminator after the initial lifetime, not by the receive guard.
    result.unwrap();
    assert!(elapsed >= Duration::from_secs(3), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
}

#[tokio::test]
async fn should_close_connection_flooded_with_frames_at_end_of_lifetime() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let config = BinaryPortConfig {
        initial_connection_lifetime: casper_types::TimeDiff::from_seconds(2),
        ..Default::default()
    };
    let (result, elapsed) = run_peer_test(config, |client| async move {
        let (mut reader, mut writer) = client.into_split();
        // Minimal malformed frames (a 1-byte payload is too short for a header), sent back to
        // back so a complete frame is always waiting. They never extend the connection lifetime.
        let frames = [1u8, 0, 0, 0, 0].repeat(1024);
        let flood = async move { while writer.write_all(&frames).await.is_ok() {} };
        // Keep reading the error responses, so sending them never blocks the server.
        let drain = async move {
            let mut buf = vec![0; 64 * 1024];
            while matches!(reader.read(&mut buf).await, Ok(read) if read > 0) {}
        };
        tokio::select! {
            _ = flood => {}
            _ = drain => {}
        }
    })
    .await;
    // Closed by the connection terminator, not kept alive by the steady stream of frames.
    result.unwrap();
    assert!(elapsed >= Duration::from_secs(2), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(4), "{elapsed:?}");
}

#[tokio::test]
async fn should_honour_termination_delay_after_completed_request() {
    use tokio::io::AsyncWriteExt;

    let config = BinaryPortConfig {
        initial_connection_lifetime: casper_types::TimeDiff::from_seconds(1),
        get_information_request_termination_delay: casper_types::TimeDiff::from_seconds(4),
        throughput_grace_period: casper_types::TimeDiff::from_seconds(1),
        ..Default::default()
    };
    let (result, elapsed) = run_peer_test(config, |mut client| async move {
        client
            .write_all(&encoded_protocol_version_request())
            .await
            .unwrap();
        read_response(&mut client).await;
        // Stay silent, far longer than the incomplete message timeout.
        assert!(wait_until_closed(&mut client).await);
    })
    .await;
    // The extension granted by the request is honoured; the receive guard doesn't cut it short.
    result.unwrap();
    assert!(elapsed >= Duration::from_secs(4), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(6), "{elapsed:?}");
}

#[tokio::test]
async fn should_serve_multiple_requests_on_long_lived_connection_by_default() {
    use tokio::io::AsyncWriteExt;

    let config = BinaryPortConfig {
        initial_connection_lifetime: casper_types::TimeDiff::from_seconds(2),
        ..Default::default()
    };
    assert!(config.keep_long_lived_tcp_connections);
    let (result, elapsed) = run_peer_test(config, |mut client| async move {
        for _ in 0..3 {
            client
                .write_all(&encoded_protocol_version_request())
                .await
                .unwrap();
            read_response(&mut client).await;
        }
        assert!(wait_until_closed(&mut client).await);
    })
    .await;
    // Closed by the connection terminator, not after the first response.
    result.unwrap();
    assert!(elapsed >= Duration::from_secs(2), "{elapsed:?}");
}

/// Large enough to fill the kernel socket buffers on both ends.
const LARGE_RESPONSE_SIZE: usize = 16 * 1024 * 1024;

fn large_response_config() -> BinaryPortConfig {
    BinaryPortConfig {
        initial_connection_lifetime: casper_types::TimeDiff::from_seconds(60),
        max_message_size_bytes: 2 * LARGE_RESPONSE_SIZE as u32,
        throughput_grace_period: casper_types::TimeDiff::from_seconds(1),
        ..Default::default()
    }
}

fn large_response() -> BinaryResponse {
    BinaryResponse::from_raw_bytes(
        casper_binary_port::ResponseType::ProtocolVersion,
        vec![0; LARGE_RESPONSE_SIZE],
    )
}

#[tokio::test]
async fn should_drop_peer_that_does_not_read_response() {
    use tokio::io::AsyncWriteExt;

    let (result, elapsed) = run_peer_test_with_reply(
        large_response_config(),
        large_response,
        |mut client| async move {
            client
                .write_all(&encoded_protocol_version_request())
                .await
                .unwrap();
            // Never read the response. Keep poking the server until it drops the connection
            // (writes fail once the server has reset it).
            loop {
                tokio::time::sleep(Duration::from_millis(250)).await;
                if client.write_all(&[0]).await.is_err() {
                    break;
                }
            }
        },
    )
    .await;
    assert!(
        matches!(
            result,
            Err(super::Error::TransferViolation(
                super::transfer_guard::TransferViolation {
                    direction: super::transfer_guard::Direction::Send,
                    ..
                }
            ))
        ),
        "{result:?}"
    );
    // Dropped after the grace period - the connection lifetime (60s) didn't save the peer.
    assert!(elapsed >= Duration::from_secs(1), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(4), "{elapsed:?}");
}

#[tokio::test]
async fn should_leave_unread_response_to_connection_terminator_when_rate_check_disabled() {
    use tokio::io::AsyncWriteExt;

    let config = BinaryPortConfig {
        initial_connection_lifetime: casper_types::TimeDiff::from_seconds(3),
        get_information_request_termination_delay: casper_types::TimeDiff::from_seconds(0),
        min_transfer_throughput_bytes_per_second: 0,
        ..large_response_config()
    };
    let (result, elapsed) =
        run_peer_test_with_reply(config, large_response, |mut client| async move {
            client
                .write_all(&encoded_protocol_version_request())
                .await
                .unwrap();
            // Never read the response; poke the server until it drops the connection.
            loop {
                tokio::time::sleep(Duration::from_millis(250)).await;
                if client.write_all(&[0]).await.is_err() {
                    break;
                }
            }
        })
        .await;
    // No transfer guard; the connection lives out its lifetime.
    assert!(
        !matches!(result, Err(super::Error::TransferViolation(_))),
        "{result:?}"
    );
    assert!(elapsed >= Duration::from_secs(3), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
}

#[tokio::test]
async fn should_send_large_response_to_reading_peer() {
    use tokio::io::AsyncWriteExt;

    let (result, _) = run_peer_test_with_reply(
        large_response_config(),
        large_response,
        |mut client| async move {
            client
                .write_all(&encoded_protocol_version_request())
                .await
                .unwrap();
            let response = read_response(&mut client).await;
            assert!(response.len() > LARGE_RESPONSE_SIZE);
            // Close from the client side.
        },
    )
    .await;
    // The server observes the client closing the connection.
    result.unwrap();
}

#[tokio::test]
async fn should_close_connection_after_first_response_when_long_lived_disabled() {
    use tokio::io::AsyncWriteExt;

    let config = BinaryPortConfig {
        initial_connection_lifetime: casper_types::TimeDiff::from_seconds(60),
        keep_long_lived_tcp_connections: false,
        ..Default::default()
    };
    let (result, elapsed) = run_peer_test(config, |mut client| async move {
        client
            .write_all(&encoded_protocol_version_request())
            .await
            .unwrap();
        let response = read_response(&mut client).await;
        let response: casper_binary_port::BinaryResponseAndRequest =
            casper_types::bytesrepr::deserialize_from_slice(response).unwrap();
        assert!(response.is_success());
        // The server closes the connection gracefully right after the response.
        assert!(wait_until_closed(&mut client).await);
    })
    .await;
    result.unwrap();
    assert!(elapsed < Duration::from_secs(1), "{elapsed:?}");
}

struct TestCase {
    allow_request_get_all_values: bool,
    allow_request_get_trie: bool,
    allow_request_speculative_exec: bool,
    request_generator: Either<fn(&mut TestRng) -> Command, Command>,
}

#[tokio::test]
async fn should_enqueue_requests_for_enabled_functions() {
    let mut rng = TestRng::new();

    let get_all_values_enabled = TestCase {
        allow_request_get_all_values: ENABLED,
        allow_request_get_trie: rng.gen(),
        allow_request_speculative_exec: rng.gen(),
        request_generator: Either::Left(|_| all_values_request()),
    };

    let get_trie_enabled = TestCase {
        allow_request_get_all_values: rng.gen(),
        allow_request_get_trie: ENABLED,
        allow_request_speculative_exec: rng.gen(),
        request_generator: Either::Left(|_| trie_request()),
    };

    let try_speculative_exec_enabled = TestCase {
        allow_request_get_all_values: rng.gen(),
        allow_request_get_trie: rng.gen(),
        allow_request_speculative_exec: ENABLED,
        request_generator: Either::Left(try_speculative_exec_request),
    };

    for test_case in [
        get_all_values_enabled,
        get_trie_enabled,
        try_speculative_exec_enabled,
    ] {
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
async fn should_use_requested_block_for_speculative_execution() {
    let mut rng = TestRng::new();
    let request = Command::TrySpeculativeExec {
        transaction: Transaction::V1(TransactionV1::random(&mut rng)),
        block_identifier: Some(BlockIdentifier::Height(1)),
    };
    let test_case = TestCase {
        allow_request_get_all_values: DISABLED,
        allow_request_get_trie: DISABLED,
        allow_request_speculative_exec: ENABLED,
        request_generator: Either::Right(request),
    };
    let (_, mut runner) = run_test_case(test_case, &mut rng).await;

    runner
        .crank_until(
            &mut rng,
            got_requested_contract_runtime_request,
            Duration::from_secs(10),
        )
        .await;
}

#[tokio::test]
async fn should_reject_unavailable_or_incompatible_speculative_execution_block() {
    let mut rng = TestRng::new();

    for (height, expected_error) in [
        (u64::MAX, ErrorCode::NotFound),
        (2, ErrorCode::UnsupportedRequest),
    ] {
        let request = Command::TrySpeculativeExec {
            transaction: Transaction::V1(TransactionV1::random(&mut rng)),
            block_identifier: Some(BlockIdentifier::Height(height)),
        };
        let test_case = TestCase {
            allow_request_get_all_values: DISABLED,
            allow_request_get_trie: DISABLED,
            allow_request_speculative_exec: ENABLED,
            request_generator: Either::Right(request),
        };
        let (receiver, mut runner) = run_test_case(test_case, &mut rng).await;

        let response = tokio::select! {
            result = receiver => result.expect("expected successful response"),
            _ = runner.crank_until(
                &mut rng,
                got_contract_runtime_request,
                Duration::from_secs(10),
            ) => {
                panic!("request should fail before reaching contract runtime")
            }
        };
        assert_eq!(response.error_code(), expected_error as u16);
    }
}

#[tokio::test]
async fn should_return_error_for_disabled_functions() {
    let mut rng = TestRng::new();

    const EXPECTED_ERROR_CODE: ErrorCode = ErrorCode::FunctionDisabled;

    let get_all_values_disabled = TestCase {
        allow_request_get_all_values: DISABLED,
        allow_request_get_trie: rng.gen(),
        allow_request_speculative_exec: rng.gen(),
        request_generator: Either::Left(|_| all_values_request()),
    };

    let get_trie_disabled = TestCase {
        allow_request_get_all_values: rng.gen(),
        allow_request_get_trie: DISABLED,
        allow_request_speculative_exec: rng.gen(),
        request_generator: Either::Left(|_| trie_request()),
    };

    let try_speculative_exec_disabled = TestCase {
        allow_request_get_all_values: rng.gen(),
        allow_request_get_trie: rng.gen(),
        allow_request_speculative_exec: DISABLED,
        request_generator: Either::Left(try_speculative_exec_request),
    };

    for test_case in [
        get_all_values_disabled,
        get_trie_disabled,
        try_speculative_exec_disabled,
    ] {
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
        assert_eq!(result.error_code(), EXPECTED_ERROR_CODE as u16)
    }
}

#[tokio::test]
async fn should_return_empty_response_when_fetching_empty_key() {
    let mut rng = TestRng::new();

    let test_cases: Vec<TestCase> = record_requests_with_empty_keys()
        .into_iter()
        .map(|request| TestCase {
            allow_request_get_all_values: DISABLED,
            allow_request_get_trie: DISABLED,
            allow_request_speculative_exec: DISABLED,
            request_generator: Either::Right(request),
        })
        .collect();

    for test_case in test_cases {
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
        assert_eq!(result.error_code(), 0);
        assert!(result.payload().is_empty());
    }
}

async fn run_test_case(
    TestCase {
        allow_request_get_all_values,
        allow_request_get_trie,
        allow_request_speculative_exec,
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
        allow_request_speculative_exec,
        max_message_size_bytes: 1024,
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
    let request = match request_generator {
        Either::Left(f) => f(rng),
        Either::Right(v) => v,
    };
    let event = BinaryPortEvent::HandleRequest {
        request,
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
    protocol_version: ProtocolVersion,
}

impl NetworkedReactor for MockReactor {}

impl Reactor for MockReactor {
    type Event = Event;
    type Config = BinaryPortConfig;
    type Error = ReactorError;

    fn new(
        config: Self::Config,
        chainspec: Arc<Chainspec>,
        _chainspec_raw_bytes: Arc<ChainspecRawBytes>,
        _network_identity: NetworkIdentity,
        registry: &Registry,
        _event_queue: EventQueueHandle<Self::Event>,
        _rng: &mut NodeRng,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error> {
        let binary_port_metrics = BinaryPortMetrics::new(registry).unwrap();
        let protocol_version = chainspec.protocol_version();
        let mut binary_port = BinaryPort::new(config, chainspec, binary_port_metrics);
        <BinaryPort as InitializedComponent<Event>>::start_initialization(&mut binary_port);

        let reactor = MockReactor {
            binary_port,
            protocol_version,
        };

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
            Event::ContractRuntimeRequest(_) | Event::ReactorInfoRequest(_) => {
                // We're only interested if the binary port actually created a request to Contract
                // Runtime component, but we're not interested in the result.
                Effects::new()
            }
            Event::AcceptTransactionRequest(req) => req.responder.respond(Ok(())).ignore(),
            Event::StorageRequest(StorageRequest::GetHighestCompleteBlockHeader { responder }) => {
                responder
                    .respond(Some(block_header(rng, 0, self.protocol_version)))
                    .ignore()
            }
            Event::StorageRequest(StorageRequest::GetBlockHeaderByHeight {
                block_height,
                responder,
                ..
            }) => {
                let maybe_header = if block_height == u64::MAX {
                    None
                } else {
                    let protocol_version = if block_height == 2 {
                        ProtocolVersion::from_parts(999, 0, 0)
                    } else {
                        self.protocol_version
                    };
                    Some(block_header(rng, block_height, protocol_version))
                };
                responder.respond(maybe_header).ignore()
            }
            Event::StorageRequest(req) => panic!("unexpected storage req {}", req),
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
    #[from]
    AcceptTransactionRequest(AcceptTransactionRequest),
    StorageRequest(StorageRequest),
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

impl From<StorageRequest> for Event {
    fn from(request: StorageRequest) -> Self {
        Event::StorageRequest(request)
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
            Event::AcceptTransactionRequest(request) => {
                write!(formatter, "accept transaction request: {:?}", request)
            }
            Event::StorageRequest(request) => {
                write!(formatter, "storage request: {:?}", request)
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

fn all_values_request() -> Command {
    let state_identifier = GlobalStateIdentifier::StateRootHash(Digest::hash([1u8; 32]));
    Command::Get(GetRequest::State(Box::new(GlobalStateRequest::new(
        Some(state_identifier),
        GlobalStateEntityQualifier::AllItems {
            key_tag: KeyTag::Account,
        },
    ))))
}

#[cfg(test)]
fn record_requests_with_empty_keys() -> Vec<Command> {
    let mut data = Vec::new();
    for record_id in RecordId::all() {
        data.push(Command::Get(GetRequest::Record {
            record_type_tag: record_id.into(),
            key: vec![],
        }))
    }
    data
}

fn trie_request() -> Command {
    Command::Get(GetRequest::Trie {
        trie_key: Digest::hash([1u8; 32]),
    })
}

fn try_speculative_exec_request(rng: &mut TestRng) -> Command {
    Command::TrySpeculativeExec {
        transaction: Transaction::V1(TransactionV1::random(rng)),
        block_identifier: None,
    }
}

fn got_contract_runtime_request(event: &Event) -> bool {
    matches!(event, Event::ContractRuntimeRequest(_))
}

fn got_requested_contract_runtime_request(event: &Event) -> bool {
    matches!(
        event,
        Event::ContractRuntimeRequest(ContractRuntimeRequest::SpeculativelyExecute {
            block_header,
            ..
        }) if block_header.height() == 1
    )
}

fn block_header(rng: &mut TestRng, height: u64, protocol_version: ProtocolVersion) -> BlockHeader {
    BlockHeader::V2(casper_types::BlockHeaderV2::new(
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        Default::default(),
        Timestamp::now(),
        Default::default(),
        height,
        protocol_version,
        PublicKey::random(rng),
        Default::default(),
        Default::default(),
        Default::default(),
    ))
}
