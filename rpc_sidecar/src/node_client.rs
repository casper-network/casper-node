use std::{future::Future, net::SocketAddr, sync::Arc, time::Duration};

use async_trait::async_trait;

use casper_types::{
    binary_port::{
        binary_request::BinaryRequest, db_id::DbId, get::GetRequest,
        non_persistent_data::NonPersistedDataRequest,
    },
    bytesrepr::{self, ToBytes},
    BlockBody, BlockHash, BlockHeader, BlockSignatures, Digest,
    FinalizedApprovals, Transaction, TransactionHash,
};
use juliet::{
    io::IoCoreBuilder,
    protocol::ProtocolBuilder,
    rpc::{JulietRpcClient, JulietRpcServer, RpcBuilder},
    ChannelConfiguration, ChannelId,
};
use tokio::{
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpStream,
    },
    sync::RwLock,
};
use tracing::{error, info, warn};

#[async_trait]
pub trait NodeClient: Send + Sync + 'static {
    async fn read_from_db(&self, db: DbId, key: &[u8]) -> Result<Option<Vec<u8>>, Error>;
    async fn read_from_mem(&self, req: NonPersistedDataRequest) -> Result<Option<Vec<u8>>, Error>;

    async fn read_transaction(&self, hash: TransactionHash) -> Result<Option<Transaction>, Error> {
        let key = hash.to_bytes().expect("should always serialize a digest");
        self.read_from_db(DbId::Transactions, &key)
            .await?
            .map(|bytes| bytesrepr::deserialize_from_slice(&bytes))
            .transpose()
            .map_err(|err| Error::Deserialization(err.to_string()))
    }

    async fn read_finalized_approvals(
        &self,
        hash: TransactionHash,
    ) -> Result<Option<FinalizedApprovals>, Error> {
        let key = hash.to_bytes().expect("should always serialize a digest");
        self.read_from_db(DbId::VersionedFinalizedApprovals, &key)
            .await?
            .map(|bytes| bytesrepr::deserialize_from_slice(&bytes))
            .transpose()
            .map_err(|err| Error::Deserialization(err.to_string()))
    }

    async fn read_block_header(&self, hash: BlockHash) -> Result<Option<BlockHeader>, Error> {
        let key = hash.to_bytes().expect("should always serialize a digest");
        self.read_from_db(DbId::BlockHeaderV2, &key)
            .await?
            .map(|bytes| bytesrepr::deserialize_from_slice(&bytes))
            .transpose()
            .map_err(|err| Error::Deserialization(err.to_string()))
    }

    async fn read_block_body(&self, hash: Digest) -> Result<Option<BlockBody>, Error> {
        let key = hash.to_bytes().expect("should always serialize a digest");
        self.read_from_db(DbId::BlockBodyV2, &key)
            .await?
            .map(|bytes| bytesrepr::deserialize_from_slice(&bytes))
            .transpose()
            .map_err(|err| Error::Deserialization(err.to_string()))
    }

    async fn read_block_signatures(
        &self,
        hash: BlockHash,
    ) -> Result<Option<BlockSignatures>, Error> {
        let key = hash.to_bytes().expect("should always serialize a digest");
        self.read_from_db(DbId::BlockMetadata, &key)
            .await?
            .map(|bytes| bincode::deserialize(&bytes))
            .transpose()
            .map_err(|err| Error::Deserialization(err.to_string()))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("request error: {0}")]
    RequestFailed(String),
    #[error("failed to deserialize a response: {0}")]
    Deserialization(String),
    #[error("failed to serialize a request: {0}")]
    Serialization(String),
}

const CHANNEL_COUNT: usize = 1;

pub struct JulietNodeClient {
    client: Arc<RwLock<JulietRpcClient<CHANNEL_COUNT>>>,
}

impl JulietNodeClient {
    pub async fn new(addr: impl Into<SocketAddr>) -> (Self, impl Future<Output = ()>) {
        let addr = addr.into();
        let protocol_builder = ProtocolBuilder::<1>::with_default_channel_config(
            ChannelConfiguration::default()
                .with_request_limit(3)
                .with_max_request_payload_size(4 * 1024 * 1024)
                .with_max_response_payload_size(4 * 1024 * 1024),
        );
        let io_builder = IoCoreBuilder::new(protocol_builder).buffer_size(ChannelId::new(0), 16);
        let rpc_builder = RpcBuilder::new(io_builder);

        let stream = Self::connect_with_retries(addr).await;
        let (reader, writer) = stream.into_split();
        let (client, server) = rpc_builder.build(reader, writer);
        let client = Arc::new(RwLock::new(client));
        let server_loop = Self::server_loop(addr, rpc_builder, Arc::clone(&client), server);

        (Self { client }, server_loop)
    }

    async fn server_loop(
        addr: SocketAddr,
        rpc_builder: RpcBuilder<CHANNEL_COUNT>,
        client: Arc<RwLock<JulietRpcClient<CHANNEL_COUNT>>>,
        mut server: JulietRpcServer<CHANNEL_COUNT, OwnedReadHalf, OwnedWriteHalf>,
    ) {
        loop {
            match server.next_request().await {
                Ok(None) | Err(_) => {
                    info!("node connection closed, will attempt to reconnect");
                    let (reader, writer) = Self::connect_with_retries(addr).await.into_split();
                    let (new_client, new_server) = rpc_builder.build(reader, writer);

                    info!("connection with the node has been re-established");
                    *client.write().await = new_client;
                    server = new_server;
                }
                Ok(Some(_)) => {
                    warn!("node client received a request from the node, it's going to be ignored")
                }
            }
        }
    }

    async fn connect_with_retries(addr: SocketAddr) -> TcpStream {
        const BACKOFF_MULT: u64 = 2;
        const MIN_WAIT: u64 = 1000;
        const MAX_WAIT: u64 = 60_000;

        let mut wait = MIN_WAIT;
        loop {
            match TcpStream::connect(addr).await {
                Ok(server) => break server,
                Err(err) => {
                    wait = (wait * BACKOFF_MULT).min(MAX_WAIT);
                    error!(%err, "failed to connect to the node, waiting {wait}ms before retrying");
                    tokio::time::sleep(Duration::from_millis(wait)).await;
                }
            }
        }
    }

    async fn dispatch(&self, req: BinaryRequest) -> Result<Option<Vec<u8>>, Error> {
        let payload = req.to_bytes().expect("should always serialize a request");
        let request_guard = self
            .client
            .read()
            .await
            .create_request(ChannelId::new(0))
            .with_payload(payload.into())
            .queue_for_sending()
            .await;
        let response = request_guard
            .wait_for_response()
            .await
            .map_err(|err| Error::RequestFailed(err.to_string()))?;
        Ok(response.map(|bytes| bytes.to_vec()))
    }
}

#[async_trait]
impl NodeClient for JulietNodeClient {
    async fn read_from_db(&self, db: DbId, key: &[u8]) -> Result<Option<Vec<u8>>, Error> {
        let get = GetRequest::Db {
            db,
            key: key.to_vec(),
        };
        self.dispatch(BinaryRequest::Get(get)).await
    }

    async fn read_from_mem(&self, req: NonPersistedDataRequest) -> Result<Option<Vec<u8>>, Error> {
        let get = GetRequest::NonPersistedData(req);
        self.dispatch(BinaryRequest::Get(get)).await
    }
}
