use std::{future::Future, net::SocketAddr, sync::Arc, time::Duration};

use async_trait::async_trait;

use crate::{config::ExponentialBackoffConfig, NodeClientConfig};
use casper_types::{
    binary_port::{
        binary_request::BinaryRequest, db_id::DbId, get::GetRequest,
        get_all_values::GetAllValuesResult, global_state::GlobalStateQueryResult,
        non_persistent_data::NonPersistedDataRequest,
    },
    bytesrepr::{self, ToBytes},
    contract_messages::Message,
    execution::{ExecutionResult, ExecutionResultV2},
    AvailableBlockRange, BlockBody, BlockHash, BlockHashAndHeight, BlockHeader, BlockSignatures,
    BlockSynchronizerStatus, Digest, FinalizedApprovals, Key, KeyTag, NextUpgrade, PeersMap,
    ProtocolVersion, PublicKey, ReactorState, TimeDiff, Timestamp, Transaction, TransactionHash,
    Transfer,
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
pub trait NodeClient: Send + Sync {
    async fn read_from_db(&self, db: DbId, key: &[u8]) -> Result<Option<Vec<u8>>, Error>;

    async fn read_from_mem(&self, req: NonPersistedDataRequest) -> Result<Option<Vec<u8>>, Error>;

    async fn query_global_state(
        &self,
        state_root_hash: Digest,
        base_key: Key,
        path: Vec<String>,
    ) -> Result<GlobalStateQueryResult, Error>;

    async fn query_global_state_by_tag(
        &self,
        state_root_hash: Digest,
        tag: KeyTag,
    ) -> Result<GetAllValuesResult, Error>;

    async fn try_accept_transaction(
        &self,
        transaction: Transaction,
        speculative_exec_block: Option<BlockHeader>,
    ) -> Result<(), Error>;

    async fn exec_speculatively(
        &self,
        state_root_hash: Digest,
        block_time: Timestamp,
        protocol_version: ProtocolVersion,
        transaction: Transaction,
    ) -> Result<Option<(ExecutionResultV2, Vec<Message>)>, Error>;

    async fn read_transaction(&self, hash: TransactionHash) -> Result<Option<Transaction>, Error> {
        let key = hash.to_bytes().expect("should always serialize a digest");
        self.read_from_db(DbId::Transactions, &key)
            .await?
            .map(bytesrepr::deserialize_from_slice)
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
            .map(bytesrepr::deserialize_from_slice)
            .transpose()
            .map_err(|err| Error::Deserialization(err.to_string()))
    }

    async fn read_block_header(&self, hash: BlockHash) -> Result<Option<BlockHeader>, Error> {
        let key = hash.to_bytes().expect("should always serialize a digest");
        self.read_from_db(DbId::BlockHeaderV2, &key)
            .await?
            .map(bytesrepr::deserialize_from_slice)
            .transpose()
            .map_err(|err| Error::Deserialization(err.to_string()))
    }

    async fn read_block_body(&self, hash: Digest) -> Result<Option<BlockBody>, Error> {
        let key = hash.to_bytes().expect("should always serialize a digest");
        self.read_from_db(DbId::BlockBodyV2, &key)
            .await?
            .map(bytesrepr::deserialize_from_slice)
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

    async fn read_block_transfers(&self, hash: BlockHash) -> Result<Option<Vec<Transfer>>, Error> {
        let key = hash.to_bytes().expect("should always serialize a digest");
        self.read_from_db(DbId::Transfer, &key)
            .await?
            .map(|bytes| bincode::deserialize(&bytes))
            .transpose()
            .map_err(|err| Error::Deserialization(err.to_string()))
    }

    async fn read_execution_result(
        &self,
        hash: TransactionHash,
    ) -> Result<Option<ExecutionResult>, Error> {
        let key = hash.to_bytes().expect("should always serialize a digest");
        self.read_from_db(DbId::ExecutionResults, &key)
            .await?
            .map(bytesrepr::deserialize_from_slice)
            .transpose()
            .map_err(|err| Error::Deserialization(err.to_string()))
    }

    async fn read_transaction_block_info(
        &self,
        transaction_hash: TransactionHash,
    ) -> Result<Option<BlockHashAndHeight>, Error> {
        self.read_from_mem(
            NonPersistedDataRequest::TransactionHash2BlockHashAndHeight { transaction_hash },
        )
        .await?
        .map(bytesrepr::deserialize_from_slice)
        .transpose()
        .map_err(|err| Error::Deserialization(err.to_string()))
    }

    async fn read_highest_completed_block_info(&self) -> Result<Option<BlockHashAndHeight>, Error> {
        self.read_from_mem(NonPersistedDataRequest::HighestCompleteBlock {})
            .await?
            .map(bytesrepr::deserialize_from_slice)
            .transpose()
            .map_err(|err| Error::Deserialization(err.to_string()))
    }

    async fn read_block_hash_from_height(&self, height: u64) -> Result<Option<BlockHash>, Error> {
        self.read_from_mem(NonPersistedDataRequest::BlockHeight2Hash { height })
            .await?
            .map(bytesrepr::deserialize_from_slice)
            .transpose()
            .map_err(|err| Error::Deserialization(err.to_string()))
    }

    async fn does_exist_in_completed_blocks(&self, block_hash: BlockHash) -> Result<bool, Error> {
        let resp = self
            .read_from_mem(NonPersistedDataRequest::CompletedBlockContains { block_hash })
            .await?
            .ok_or(Error::NoResponseBody)?;
        bytesrepr::deserialize_from_slice(resp)
            .map_err(|err| Error::Deserialization(err.to_string()))
    }

    async fn read_peers(&self) -> Result<PeersMap, Error> {
        let resp = self
            .read_from_mem(NonPersistedDataRequest::Peers)
            .await?
            .ok_or(Error::NoResponseBody)?;
        bytesrepr::deserialize_from_slice(resp)
            .map_err(|err| Error::Deserialization(err.to_string()))
    }

    async fn read_uptime(&self) -> Result<Duration, Error> {
        let resp = self
            .read_from_mem(NonPersistedDataRequest::Uptime)
            .await?
            .ok_or(Error::NoResponseBody)?;
        let secs: u64 = bytesrepr::deserialize_from_slice(resp)
            .map_err(|err| Error::Deserialization(err.to_string()))?;
        Ok(Duration::from_secs(secs))
    }

    async fn read_last_progress(&self) -> Result<Timestamp, Error> {
        let resp = self
            .read_from_mem(NonPersistedDataRequest::LastProgress)
            .await?
            .ok_or(Error::NoResponseBody)?;
        bytesrepr::deserialize_from_slice(resp)
            .map_err(|err| Error::Deserialization(err.to_string()))
    }

    async fn read_reactor_state(&self) -> Result<ReactorState, Error> {
        let resp = self
            .read_from_mem(NonPersistedDataRequest::ReactorState)
            .await?
            .ok_or(Error::NoResponseBody)?;
        bytesrepr::deserialize_from_slice(resp)
            .map_err(|err| Error::Deserialization(err.to_string()))
    }

    async fn read_network_name(&self) -> Result<String, Error> {
        let resp = self
            .read_from_mem(NonPersistedDataRequest::NetworkName)
            .await?
            .ok_or(Error::NoResponseBody)?;
        bytesrepr::deserialize_from_slice(resp)
            .map_err(|err| Error::Deserialization(err.to_string()))
    }

    async fn read_block_sync_status(&self) -> Result<BlockSynchronizerStatus, Error> {
        let resp = self
            .read_from_mem(NonPersistedDataRequest::BlockSynchronizerStatus)
            .await?
            .ok_or(Error::NoResponseBody)?;
        bytesrepr::deserialize_from_slice(resp)
            .map_err(|err| Error::Deserialization(err.to_string()))
    }

    async fn read_available_block_range(&self) -> Result<AvailableBlockRange, Error> {
        let resp = self
            .read_from_mem(NonPersistedDataRequest::AvailableBlockRange)
            .await?
            .ok_or(Error::NoResponseBody)?;
        bytesrepr::deserialize_from_slice(resp)
            .map_err(|err| Error::Deserialization(err.to_string()))
    }

    async fn read_next_upgrade(&self) -> Result<Option<NextUpgrade>, Error> {
        self.read_from_mem(NonPersistedDataRequest::NextUpgrade)
            .await?
            .map(bytesrepr::deserialize_from_slice)
            .transpose()
            .map_err(|err| Error::Deserialization(err.to_string()))
    }

    async fn read_consensus_status(&self) -> Result<Option<(PublicKey, Option<TimeDiff>)>, Error> {
        self.read_from_mem(NonPersistedDataRequest::ConsensusStatus)
            .await?
            .map(bytesrepr::deserialize_from_slice)
            .transpose()
            .map_err(|err| Error::Deserialization(err.to_string()))
    }

    async fn read_chainspec_bytes(&self) -> Result<Vec<u8>, Error> {
        self.read_from_mem(NonPersistedDataRequest::ChainspecRawBytes)
            .await?
            .ok_or(Error::NoResponseBody)
    }

    async fn read_genesis_account_bytes(&self) -> Result<Option<Vec<u8>>, Error> {
        self.read_from_mem(NonPersistedDataRequest::GenesisAccountsBytes)
            .await
    }

    async fn read_global_state_bytes(&self) -> Result<Option<Vec<u8>>, Error> {
        self.read_from_mem(NonPersistedDataRequest::GlobalStateBytes)
            .await
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
    #[error("unexpectedly received no response body")]
    NoResponseBody,
    #[error("transaction failed: {0}")]
    TransactionFailed(String),
    #[error("speculative execution failed: {0}")]
    SpeculativeExecFailed(String),
}

const CHANNEL_COUNT: usize = 1;

#[derive(Debug)]
pub struct JulietNodeClient {
    client: Arc<RwLock<JulietRpcClient<CHANNEL_COUNT>>>,
}

impl JulietNodeClient {
    pub async fn new(config: &NodeClientConfig) -> (Self, impl Future<Output = ()> + '_) {
        let protocol_builder = ProtocolBuilder::<1>::with_default_channel_config(
            ChannelConfiguration::default()
                .with_request_limit(config.request_limit)
                .with_max_request_payload_size(config.max_request_size_bytes)
                .with_max_response_payload_size(config.max_response_size_bytes),
        );
        let io_builder = IoCoreBuilder::new(protocol_builder)
            .buffer_size(ChannelId::new(0), config.queue_buffer_size);
        let rpc_builder = RpcBuilder::new(io_builder);

        let stream = Self::connect_with_retries(config.address, &config.exponential_backoff).await;
        let (reader, writer) = stream.into_split();
        let (client, server) = rpc_builder.build(reader, writer);
        let client = Arc::new(RwLock::new(client));
        let server_loop = Self::server_loop(
            config.address,
            &config.exponential_backoff,
            rpc_builder,
            Arc::clone(&client),
            server,
        );

        (Self { client }, server_loop)
    }

    async fn server_loop(
        addr: SocketAddr,
        config: &ExponentialBackoffConfig,
        rpc_builder: RpcBuilder<CHANNEL_COUNT>,
        client: Arc<RwLock<JulietRpcClient<CHANNEL_COUNT>>>,
        mut server: JulietRpcServer<CHANNEL_COUNT, OwnedReadHalf, OwnedWriteHalf>,
    ) {
        loop {
            match server.next_request().await {
                Ok(None) | Err(_) => {
                    error!("node connection closed, will attempt to reconnect");
                    let (reader, writer) =
                        Self::connect_with_retries(addr, config).await.into_split();
                    let (new_client, new_server) = rpc_builder.build(reader, writer);

                    info!("connection with the node has been re-established");
                    *client.write().await = new_client;
                    server = new_server;
                }
                Ok(Some(_)) => {
                    error!("node client received a request from the node, it's going to be ignored")
                }
            }
        }
    }

    async fn connect_with_retries(
        addr: SocketAddr,
        config: &ExponentialBackoffConfig,
    ) -> TcpStream {
        let mut wait = config.initial_delay_ms;
        loop {
            match TcpStream::connect(addr).await {
                Ok(server) => break server,
                Err(err) => {
                    warn!(%err, "failed to connect to the node, waiting {wait}ms before retrying");
                    tokio::time::sleep(Duration::from_millis(wait)).await;
                    wait = (wait * config.coefficient).min(config.max_delay_ms);
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
        Ok(response.map(Into::into))
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

    async fn query_global_state(
        &self,
        state_root_hash: Digest,
        base_key: Key,
        path: Vec<String>,
    ) -> Result<GlobalStateQueryResult, Error> {
        let get = GetRequest::State {
            state_root_hash,
            base_key,
            path,
        };
        let resp = self
            .dispatch(BinaryRequest::Get(get))
            .await?
            .ok_or(Error::NoResponseBody)?;
        bytesrepr::deserialize_from_slice(resp)
            .map_err(|err| Error::Deserialization(err.to_string()))
    }

    async fn query_global_state_by_tag(
        &self,
        state_root_hash: Digest,
        key_tag: KeyTag,
    ) -> Result<GetAllValuesResult, Error> {
        let get = GetRequest::AllValues {
            state_root_hash,
            key_tag,
        };
        let resp = self
            .dispatch(BinaryRequest::Get(get))
            .await?
            .ok_or(Error::NoResponseBody)?;
        bytesrepr::deserialize_from_slice(resp)
            .map_err(|err| Error::Deserialization(err.to_string()))
    }

    async fn try_accept_transaction(
        &self,
        transaction: Transaction,
        speculative_exec_block: Option<BlockHeader>,
    ) -> Result<(), Error> {
        let request = BinaryRequest::TryAcceptTransaction {
            transaction,
            speculative_exec_at_block: speculative_exec_block,
        };
        let resp = self.dispatch(request).await?.ok_or(Error::NoResponseBody)?;
        let result: Result<(), String> = bytesrepr::deserialize_from_slice(resp)
            .map_err(|err| Error::Deserialization(err.to_string()))?;
        result.map_err(Error::TransactionFailed)
    }

    async fn exec_speculatively(
        &self,
        state_root_hash: Digest,
        block_time: Timestamp,
        protocol_version: ProtocolVersion,
        transaction: Transaction,
    ) -> Result<Option<(ExecutionResultV2, Vec<Message>)>, Error> {
        let request = BinaryRequest::TrySpeculativeExec {
            transaction,
            state_root_hash,
            block_time,
            protocol_version,
        };
        let resp = self.dispatch(request).await?.ok_or(Error::NoResponseBody)?;
        let result: Result<Option<(ExecutionResultV2, Vec<Message>)>, String> =
            bytesrepr::deserialize_from_slice(resp)
                .map_err(|err| Error::Deserialization(err.to_string()))?;
        result.map_err(Error::SpeculativeExecFailed)
    }
}
