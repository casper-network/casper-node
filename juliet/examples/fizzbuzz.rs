//! A juliet-based fizzbuzz server and client.
//!
//! To run this example, in one terminal, launch the server:
//!
//! ```
//! cargo run --example fizzbuzz --features tracing -- server
//! ```
//!
//! Then, in a second terminal launch the client:
//!
//! ```
//! cargo run --example fizzbuzz --features tracing
//! ```
//!
//! You should [Fizz buzz](https://en.wikipedia.org/wiki/Fizz_buzz) solutions being calculated on
//! the server side and sent back.

use std::{fmt::Write, net::SocketAddr, time::Duration};

use bytes::BytesMut;
use juliet::{
    io::IoCoreBuilder,
    protocol::ProtocolBuilder,
    rpc::{IncomingRequest, RpcBuilder},
    ChannelConfiguration, ChannelId,
};
use rand::Rng;
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info, warn};

const SERVER_ADDR: &str = "127.0.0.1:12345";

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("juliet=trace".parse().unwrap())
                .add_directive("fizzbuzz=trace".parse().unwrap()),
        )
        .init();

    // Create a new protocol instance with two channels, allowing three requests in flight each.
    let protocol_builder = ProtocolBuilder::<2>::with_default_channel_config(
        ChannelConfiguration::default()
            .with_request_limit(3)
            .with_max_request_payload_size(4)
            .with_max_response_payload_size(512),
    );

    // Create the IO layer, buffering at most two messages on the wait queue per channel.
    let io_builder = IoCoreBuilder::new(protocol_builder)
        .buffer_size(ChannelId::new(0), 2)
        .buffer_size(ChannelId::new(1), 2);

    // Create the final RPC builder - we will use this on every connection.
    let rpc_builder = Box::leak(Box::new(RpcBuilder::new(io_builder)));

    let mut args = std::env::args();
    args.next().expect("did not expect missing argv0");
    let is_server = args.next().map(|a| a == "server").unwrap_or_default();

    if is_server {
        let listener = TcpListener::bind(SERVER_ADDR)
            .await
            .expect("failed to listen");
        info!("listening on {}", SERVER_ADDR);
        loop {
            match listener.accept().await {
                Ok((client, addr)) => {
                    info!("new connection from {}", addr);
                    tokio::spawn(handle_client(addr, client, rpc_builder));
                }
                Err(io_err) => {
                    warn!("acceptance failure: {:?}", io_err);
                }
            }
        }
    } else {
        let remote_server = TcpStream::connect(SERVER_ADDR)
            .await
            .expect("failed to connect to server");
        info!("connected to server {}", SERVER_ADDR);

        let (reader, writer) = remote_server.into_split();
        let (client, mut server) = rpc_builder.build(reader, writer);

        // We are not using the server functionality, but still need to run it for IO reasons.
        tokio::spawn(async move {
            if let Err(err) = server.next_request().await {
                error!(%err, "server read error");
            }
        });

        for num in 0..u32::MAX {
            let request_guard = client
                .create_request(ChannelId::new(0))
                .with_payload(num.to_be_bytes().to_vec().into())
                .queue_for_sending()
                .await;

            debug!("sent request {}", num);
            match request_guard.wait_for_response().await {
                Ok(response) => {
                    let decoded =
                        String::from_utf8(response.expect("should have payload").to_vec())
                            .expect("did not expect invalid UTF8");
                    info!("{} -> {}", num, decoded);
                }
                Err(err) => {
                    error!("server error: {}", err);
                    break;
                }
            }
        }
    }
}

/// Handles a incoming client connection.
async fn handle_client<const N: usize>(
    addr: SocketAddr,
    mut client: TcpStream,
    rpc_builder: &RpcBuilder<N>,
) {
    let (reader, writer) = client.split();
    let (client, mut server) = rpc_builder.build(reader, writer);

    loop {
        match server.next_request().await {
            Ok(opt_incoming_request) => {
                if let Some(incoming_request) = opt_incoming_request {
                    tokio::spawn(handle_request(incoming_request));
                } else {
                    // Client exited.
                    info!("client {} disconnected", addr);
                    break;
                }
            }
            Err(err) => {
                warn!("client {} error: {}", addr, err);
                break;
            }
        }
    }

    // We are a server, we won't make any requests of our own, but we need to keep the client
    // around, since dropping the client will trigger a server shutdown.
    drop(client);
}

/// Handles a single request made by a client (on the server).
async fn handle_request(incoming_request: IncomingRequest) {
    let processing_time = rand::thread_rng().gen_range(5..20) * Duration::from_millis(100);
    tokio::time::sleep(processing_time).await;

    let payload = incoming_request
        .payload()
        .as_ref()
        .expect("should have payload");
    let num =
        u32::from_be_bytes(<[u8; 4]>::try_from(payload.as_ref()).expect("could not decode u32"));

    // Construct the response.
    let mut response_payload = BytesMut::new();
    if num % 3 == 0 {
        response_payload.write_str("Fizz ").unwrap();
    }
    if num % 5 == 0 {
        response_payload.write_str("Buzz ").unwrap();
    }
    if response_payload.is_empty() {
        write!(response_payload, "{}", num).unwrap();
    }

    // Send it back.
    incoming_request.respond(Some(response_payload.freeze()));
}
