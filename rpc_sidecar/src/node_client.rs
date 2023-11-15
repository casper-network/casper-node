use async_trait::async_trait;

use casper_types::{
    binary_port::{
        binary_request::BinaryRequest, db_id::DbId, get::GetRequest,
        non_persistent_data::NonPersistedDataRequest,
    },
    bytesrepr::{self, ToBytes},
    FinalizedApprovals, Transaction, TransactionHash,
};
use juliet::{
    io::IoCoreBuilder,
    protocol::ProtocolBuilder,
    rpc::{JulietRpcClient, RpcBuilder},
    ChannelConfiguration, ChannelId,
};
use tokio::{net::TcpStream, task::JoinHandle};

#[async_trait]
pub trait NodeClient: Send + Sync + 'static {
    async fn read_from_db(&self, db: DbId, key: &[u8]) -> Result<Vec<u8>, Error>;
    async fn read_from_mem(&self, req: NonPersistedDataRequest) -> Result<Vec<u8>, Error>;

    async fn read_transaction(&self, hash: TransactionHash) -> Result<Transaction, Error> {
        let key = hash.to_bytes().expect("should always serialize a digest");
        let bytes = self.read_from_db(DbId::Transactions, &key).await?;
        bytesrepr::deserialize_from_slice::<_, Transaction>(&bytes)
            .map_err(|err| Error::Deserialization(err.to_string()))
    }

    async fn read_finalized_approvals(
        &self,
        hash: TransactionHash,
    ) -> Result<FinalizedApprovals, Error> {
        let key = hash.to_bytes().expect("should always serialize a digest");
        let bytes = self
            .read_from_db(DbId::VersionedFinalizedApprovals, &key)
            .await?;
        bytesrepr::deserialize_from_slice::<_, FinalizedApprovals>(&bytes)
            .map_err(|err| Error::Deserialization(err.to_string()))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("request error: {0}")]
    RequestFailed(String),
    #[error("node response had an empty body")]
    EmptyResponseBody,
    #[error("failed to deserialize a response: {0}")]
    Deserialization(String),
    #[error("failed to serialize a request: {0}")]
    Serialization(String),
}

const CHANNEL_COUNT: usize = 1;

pub struct JulietNodeClient {
    client: JulietRpcClient<CHANNEL_COUNT>,
    handle: JoinHandle<()>,
}

impl JulietNodeClient {
    pub async fn new() -> Self {
        let protocol_builder = ProtocolBuilder::<1>::with_default_channel_config(
            ChannelConfiguration::default()
                .with_request_limit(3)
                .with_max_request_payload_size(4 * 1024 * 1024)
                .with_max_response_payload_size(4 * 1024 * 1024),
        );
        let io_builder = IoCoreBuilder::new(protocol_builder).buffer_size(ChannelId::new(0), 16);
        let rpc_builder = RpcBuilder::new(io_builder);

        let remote_server = TcpStream::connect("127.0.0.1:28104")
            .await
            .expect("failed to connect to server");

        let (reader, writer) = remote_server.into_split();
        let (client, mut server) = rpc_builder.build(reader, writer);

        // We are not using the server functionality, but still need to run it for IO reasons.
        let handle = tokio::spawn(async move {
            if let Err(err) = server.next_request().await {
                println!("server read error: {}", err);
            }
        });

        Self { client, handle }
    }

    async fn dispatch(&self, req: BinaryRequest) -> Result<Vec<u8>, Error> {
        let payload = req.to_bytes().expect("should always serialize a request");
        let request_guard = self
            .client
            .create_request(ChannelId::new(0))
            .with_payload(payload.into())
            .queue_for_sending()
            .await;
        let response = request_guard
            .wait_for_response()
            .await
            .map_err(|err| Error::RequestFailed(err.to_string()))?;
        Ok(response.ok_or(Error::EmptyResponseBody)?.to_vec())
    }
}

#[async_trait]
impl NodeClient for JulietNodeClient {
    async fn read_from_db(&self, db: DbId, key: &[u8]) -> Result<Vec<u8>, Error> {
        let get = GetRequest::Db {
            db,
            key: key.to_vec(),
        };
        self.dispatch(BinaryRequest::Get(get)).await
    }

    async fn read_from_mem(&self, req: NonPersistedDataRequest) -> Result<Vec<u8>, Error> {
        let get = GetRequest::NonPersistedData(req);
        self.dispatch(BinaryRequest::Get(get)).await
    }
}

impl Drop for JulietNodeClient {
    fn drop(&mut self) {
        self.handle.abort()
    }
}
