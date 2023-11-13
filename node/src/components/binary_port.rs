//! The Binary Port
mod error;
mod event;
mod metrics;
#[cfg(test)]
mod tests;

use std::net::SocketAddr;

use bytes::BytesMut;
use casper_types::{bytesrepr::FromBytes, BinaryRequest, BlockHash};
use datasize::DataSize;
use futures::{future::BoxFuture, FutureExt};
use juliet::{
    io::IoCoreBuilder, protocol::ProtocolBuilder, rpc::RpcBuilder, ChannelConfiguration, ChannelId,
};
use prometheus::Registry;
use tokio::net::{TcpListener, TcpStream};
use tracing::{error, info, warn};

use crate::{
    effect::{requests::StorageRequest, EffectBuilder, Effects},
    reactor::{main_reactor::MainEvent, Finalize},
    types::NodeRng,
    utils::ListeningError,
};

use self::metrics::Metrics;

use super::{Component, ComponentState, InitializedComponent, PortBoundComponent};
pub(crate) use event::Event;

const COMPONENT_NAME: &str = "binary_port";

#[derive(Debug, DataSize)]
pub(crate) struct BinaryPort {
    #[data_size(skip)]
    metrics: Metrics,
    state: ComponentState,
}

impl BinaryPort {
    pub(crate) fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        Ok(Self {
            state: ComponentState::Uninitialized,
            metrics: Metrics::new(registry)?,
        })
    }
}

impl<REv> Component<REv> for BinaryPort
where
    REv: From<Event> + From<StorageRequest> + Send,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match &self.state {
            ComponentState::Uninitialized => {
                warn!(
                    ?event,
                    name = <Self as Component<MainEvent>>::name(self),
                    "should not handle this event when component is uninitialized"
                );
                Effects::new()
            }
            ComponentState::Initializing => match event {
                Event::Initialize => {
                    let (effects, state) = self.bind(true, effect_builder); // TODO[RC]: `true` should become `config.enabled`
                    <Self as InitializedComponent<MainEvent>>::set_state(self, state);
                    effects
                }
            },
            ComponentState::Initialized => todo!(),
            ComponentState::Fatal(_) => todo!(),
        }
    }

    fn name(&self) -> &str {
        COMPONENT_NAME
    }
}

impl<REv> InitializedComponent<REv> for BinaryPort
where
    REv: From<Event> + From<StorageRequest> + Send,
{
    fn state(&self) -> &ComponentState {
        &self.state
    }

    fn set_state(&mut self, new_state: ComponentState) {
        info!(
            ?new_state,
            name = <Self as Component<MainEvent>>::name(self),
            "component state changed"
        );

        self.state = new_state;
    }
}

// TODO[RC]: Move to Self::
// TODO[RC]: Handle graceful shutdown
async fn handle_client<REv, const N: usize>(
    addr: SocketAddr,
    mut client: TcpStream,
    rpc_builder: &RpcBuilder<N>,
    effect_builder: EffectBuilder<REv>,
) where
    REv: From<StorageRequest>,
{
    let (reader, writer) = client.split();
    let (client, mut server) = rpc_builder.build(reader, writer);

    loop {
        match server.next_request().await {
            Ok(Some(incoming_request)) => match incoming_request.payload() {
                Some(payload) => {
                    let (req, _): (BinaryRequest, _) = BinaryRequest::from_bytes(payload.as_ref())
                        .expect("TODO: Handle malformed payload");
                    match req {
                        BinaryRequest::Get { key, db } => {
                            let (block_hash, _): (BlockHash, _) =
                                BlockHash::from_bytes(&key).unwrap();
                            error!("XXXXX - getting block hash: {:?}", block_hash);
                            match effect_builder.get_raw_data(db, block_hash).await {
                                Some(block_header_raw) => {
                                    let mut response_payload = BytesMut::new();
                                    response_payload.extend(block_header_raw);
                                    incoming_request.respond(Some(response_payload.freeze()));
                                }
                                None => panic!("error getting block header from storage"),
                            }
                        }
                        BinaryRequest::PutTransaction { tbd } => todo!(),
                        BinaryRequest::SpeculativeExec { tbd } => todo!(),
                        BinaryRequest::Quit => todo!(),
                    }
                }
                None => panic!("Should have payload"),
            },
            Ok(None) => panic!("Handle no request?"),
            Err(err) => {
                println!("client {} error: {}", addr, err);
                break;
            }
        }
    }

    // We are a server, we won't make any requests of our own, but we need to keep the client
    // around, since dropping the client will trigger a server shutdown.
    drop(client);
}

// TODO[RC]: Move to Self::
async fn run_server<REv>(effect_builder: EffectBuilder<REv>)
where
    REv: Send + From<StorageRequest>,
{
    let protocol_builder = ProtocolBuilder::<1>::with_default_channel_config(
        ChannelConfiguration::default()
            .with_request_limit(3)
            .with_max_request_payload_size(4096)
            .with_max_response_payload_size(4096),
    );
    let io_builder = IoCoreBuilder::new(protocol_builder).buffer_size(ChannelId::new(0), 16);
    let rpc_builder = Box::leak(Box::new(RpcBuilder::new(io_builder))); // TODO[RC]: Leak?

    let listener = TcpListener::bind("127.0.0.1:34568") // TODO[RC]: Take from config
        .await;
    match listener {
        Ok(listener) => loop {
            match listener.accept().await {
                Ok((client, addr)) => {
                    println!("new connection from {}", addr);
                    tokio::spawn(handle_client(addr, client, rpc_builder, effect_builder));
                }
                Err(io_err) => {
                    println!("acceptance failure: {:?}", io_err);
                }
            }
        },
        Err(_) => (), // TODO[RC] Silently ignore for now, other node started listening before us,
    };
}

impl<REv> PortBoundComponent<REv> for BinaryPort
where
    REv: From<Event> + From<StorageRequest> + Send,
{
    type Error = ListeningError;
    type ComponentEvent = Event;

    fn listen(
        &mut self,
        effect_builder: EffectBuilder<REv>,
    ) -> Result<Effects<Self::ComponentEvent>, Self::Error> {
        let server_join_handle = tokio::spawn(run_server(effect_builder));
        Ok(Effects::new())
    }
}

impl Finalize for BinaryPort {
    fn finalize(self) -> BoxFuture<'static, ()> {
        // TODO: Shutdown juliet server here
        async move {}.boxed()
    }
}
