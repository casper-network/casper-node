use std::{collections::BTreeMap, future::Future, net::SocketAddr, sync::Arc, time::Duration};

use async_trait::async_trait;
use serde::de::DeserializeOwned;

use crate::{config::ExponentialBackoffConfig, NodeClientConfig};
use casper_types::{
    binary_port::{
        binary_request::BinaryRequest, binary_response::BinaryResponse, db_id::DbId,
        get::GetRequest, get_all_values::GetAllValuesResult, global_state::GlobalStateQueryResult,
        non_persistent_data::NonPersistedDataRequest,
        type_wrappers::HighestBlockSequenceCheckResult,
    },
    bytesrepr::{self, FromBytes, ToBytes},
    contract_messages::Message,
    execution::{ExecutionResult, ExecutionResultV1, ExecutionResultV2},
    AvailableBlockRange, BlockBody, BlockBodyV1, BlockHash, BlockHashAndHeight, BlockHeader,
    BlockHeaderV1, BlockSignatures, BlockSynchronizerStatus, Deploy, Digest, EraId,
    FinalizedApprovals, FinalizedDeployApprovals, Key, KeyTag, NextUpgrade, PayloadType, Peers,
    ProtocolVersion, PublicKey, ReactorState, TimeDiff, Timestamp, Transaction, TransactionHash,
    Transfer, ValidatorChange,
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
    async fn read_from_db(&self, db: DbId, key: &[u8]) -> Result<BinaryResponse, Error>;

    async fn read_from_mem(&self, req: NonPersistedDataRequest) -> Result<BinaryResponse, Error>;

    async fn read_trie_bytes(&self, trie_key: Digest) -> Result<Option<Vec<u8>>, Error>;

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
        let resp = self.read_from_db(DbId::Transaction, &key).await?;
        parse_response_versioned::<Deploy, Transaction>(&resp)
    }

    async fn read_finalized_approvals(
        &self,
        hash: TransactionHash,
    ) -> Result<Option<FinalizedApprovals>, Error> {
        let key = hash.to_bytes().expect("should always serialize a digest");
        let resp = self
            .read_from_db(DbId::FinalizedTransactionApprovals, &key)
            .await?;
        parse_response_versioned::<FinalizedDeployApprovals, FinalizedApprovals>(&resp)
    }

    async fn read_block_header(&self, hash: BlockHash) -> Result<Option<BlockHeader>, Error> {
        let key = hash.to_bytes().expect("should always serialize a digest");
        let resp = self.read_from_db(DbId::BlockHeader, &key).await?;
        parse_response_versioned::<BlockHeaderV1, BlockHeader>(&resp)
    }

    async fn read_block_body(&self, hash: Digest) -> Result<Option<BlockBody>, Error> {
        let key = hash.to_bytes().expect("should always serialize a digest");
        let resp = self.read_from_db(DbId::BlockBody, &key).await?;
        parse_response_versioned::<BlockBodyV1, BlockBody>(&resp)
    }

    async fn read_block_signatures(
        &self,
        hash: BlockHash,
    ) -> Result<Option<BlockSignatures>, Error> {
        let key = hash.to_bytes().expect("should always serialize a digest");
        let resp = self.read_from_db(DbId::BlockMetadata, &key).await?;
        parse_response_bincode::<BlockSignatures>(&resp)
    }

    async fn read_block_transfers(&self, hash: BlockHash) -> Result<Option<Vec<Transfer>>, Error> {
        let key = hash.to_bytes().expect("should always serialize a digest");
        let resp = self.read_from_db(DbId::Transfer, &key).await?;
        parse_response::<Vec<Transfer>>(&resp)
    }

    async fn read_execution_result(
        &self,
        hash: TransactionHash,
    ) -> Result<Option<ExecutionResult>, Error> {
        let key = hash.to_bytes().expect("should always serialize a digest");
        let resp = self.read_from_db(DbId::ExecutionResult, &key).await?;
        parse_response_versioned::<ExecutionResultV1, ExecutionResult>(&resp)
    }

    async fn read_transaction_block_info(
        &self,
        transaction_hash: TransactionHash,
    ) -> Result<Option<BlockHashAndHeight>, Error> {
        let req = NonPersistedDataRequest::TransactionHash2BlockHashAndHeight { transaction_hash };
        let resp = self.read_from_mem(req).await?;
        parse_response::<BlockHashAndHeight>(&resp)
    }

    async fn read_highest_completed_block_info(&self) -> Result<Option<BlockHashAndHeight>, Error> {
        let resp = self
            .read_from_mem(NonPersistedDataRequest::HighestCompleteBlock)
            .await?;
        parse_response::<BlockHashAndHeight>(&resp)
    }

    async fn read_block_hash_from_height(&self, height: u64) -> Result<Option<BlockHash>, Error> {
        let req = NonPersistedDataRequest::BlockHeight2Hash { height };
        let resp = self.read_from_mem(req).await?;
        parse_response::<BlockHash>(&resp)
    }

    async fn does_exist_in_completed_blocks(&self, block_hash: BlockHash) -> Result<bool, Error> {
        let req = NonPersistedDataRequest::CompletedBlocksContain { block_hash };
        let resp = self.read_from_mem(req).await?;
        parse_response::<HighestBlockSequenceCheckResult>(&resp)?
            .map(|HighestBlockSequenceCheckResult(result)| result)
            .ok_or(Error::NoResponseBody)
    }

    async fn read_peers(&self) -> Result<Peers, Error> {
        let resp = self.read_from_mem(NonPersistedDataRequest::Peers).await?;
        parse_response::<Peers>(&resp)?.ok_or(Error::NoResponseBody)
    }

    async fn read_uptime(&self) -> Result<Duration, Error> {
        todo!()
        // let resp = self.read_from_mem(NonPersistedDataRequest::Uptime).await?;
        // try_parse::<TimeDiff>(&resp)?
        //     .ok_or(Error::NoResponseBody)
        //     .map(Into::into)
    }

    async fn read_last_progress(&self) -> Result<Timestamp, Error> {
        todo!()
        // let resp = self
        //     .read_from_mem(NonPersistedDataRequest::LastProgress)
        //     .await?;
        // try_parse::<Timestamp>(&resp)?.ok_or(Error::NoResponseBody)
    }

    async fn read_reactor_state(&self) -> Result<ReactorState, Error> {
        todo!()
        // let resp = self
        //     .read_from_mem(NonPersistedDataRequest::ReactorState)
        //     .await?;
        // try_parse::<ReactorState>(&resp)?.ok_or(Error::NoResponseBody)
    }

    async fn read_network_name(&self) -> Result<String, Error> {
        todo!()
        // let resp = self
        //     .read_from_mem(NonPersistedDataRequest::NetworkName)
        //     .await?;
        // try_parse::<String>(&resp)?.ok_or(Error::NoResponseBody)
    }

    async fn read_block_sync_status(&self) -> Result<BlockSynchronizerStatus, Error> {
        todo!()
        // let resp = self
        //     .read_from_mem(NonPersistedDataRequest::BlockSynchronizerStatus)
        //     .await?;
        // try_parse::<BlockSynchronizerStatus>(&resp)?.ok_or(Error::NoResponseBody)
    }

    async fn read_available_block_range(&self) -> Result<AvailableBlockRange, Error> {
        todo!()
        // let resp = self
        //     .read_from_mem(NonPersistedDataRequest::AvailableBlockRange)
        //     .await?;
        // try_parse::<AvailableBlockRange>(&resp)?.ok_or(Error::NoResponseBody)
    }

    async fn read_next_upgrade(&self) -> Result<Option<NextUpgrade>, Error> {
        todo!()
        // let resp = self
        //     .read_from_mem(NonPersistedDataRequest::NextUpgrade)
        //     .await?;
        // try_parse::<NextUpgrade>(&resp)
    }

    async fn read_consensus_status(&self) -> Result<Option<(PublicKey, Option<TimeDiff>)>, Error> {
        todo!()
        // let resp = self
        //     .read_from_mem(NonPersistedDataRequest::ConsensusStatus)
        //     .await?;
        // try_parse(&resp)
    }

    async fn read_chainspec_bytes(&self) -> Result<Vec<u8>, Error> {
        let resp = self
            .read_from_mem(NonPersistedDataRequest::ChainspecRawBytes)
            .await?;
        parse_response::<Vec<u8>>(&resp)?.ok_or(Error::NoResponseBody)
    }

    async fn read_validator_changes(
        &self,
    ) -> Result<BTreeMap<PublicKey, Vec<(EraId, ValidatorChange)>>, Error> {
        todo!()
        // let resp = self
        //     .read_from_mem(NonPersistedDataRequest::ConsensusValidatorChanges)
        //     .await?;
        // <BTreeMap<PublicKey, Vec<(EraId, ValidatorChange)>>>::try_accept(&resp)?
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("request error: {0}")]
    RequestFailed(String),
    #[error("failed to deserialize the envelope of a response: {0}")]
    EnvelopeDeserialization(String),
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
    #[error("unexpected variant received in the response: {0}")]
    UnexpectedVariantReceived(PayloadType),
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

    async fn dispatch(&self, req: BinaryRequest) -> Result<BinaryResponse, Error> {
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
            .map_err(|err| Error::RequestFailed(err.to_string()))?
            .ok_or(Error::NoResponseBody)?;
        bytesrepr::deserialize_from_slice(&response)
            .map_err(|err| Error::EnvelopeDeserialization(err.to_string()))
    }
}

#[async_trait]
impl NodeClient for JulietNodeClient {
    async fn read_from_db(&self, db: DbId, key: &[u8]) -> Result<BinaryResponse, Error> {
        let get = GetRequest::Db {
            db,
            key: key.to_vec(),
        };
        self.dispatch(BinaryRequest::Get(get)).await
    }

    async fn read_from_mem(&self, req: NonPersistedDataRequest) -> Result<BinaryResponse, Error> {
        let get = GetRequest::NonPersistedData(req);
        self.dispatch(BinaryRequest::Get(get)).await
    }

    async fn read_trie_bytes(&self, trie_key: Digest) -> Result<Option<Vec<u8>>, Error> {
        let get = GetRequest::Trie { trie_key };
        let resp = self.dispatch(BinaryRequest::Get(get)).await?;
        parse_response::<Vec<u8>>(&resp)
    }

    async fn query_global_state(
        &self,
        _state_root_hash: Digest,
        _base_key: Key,
        _path: Vec<String>,
    ) -> Result<GlobalStateQueryResult, Error> {
        todo!()
        // let get = GetRequest::State {
        //     state_root_hash,
        //     base_key,
        //     path,
        // };
        // let resp = self.dispatch(BinaryRequest::Get(get)).await?;
        // GlobalStateQueryResult::try_accept(&resp)?.ok_or(Error::NoResponseBody)
    }

    async fn query_global_state_by_tag(
        &self,
        _state_root_hash: Digest,
        _key_tag: KeyTag,
    ) -> Result<GetAllValuesResult, Error> {
        todo!()
        // let get = GetRequest::AllValues {
        //     state_root_hash,
        //     key_tag,
        // };
        // let resp = self.dispatch(BinaryRequest::Get(get)).await?;
        // GetAllValuesResult::try_accept(&resp)?.ok_or(Error::NoResponseBody)
    }

    async fn try_accept_transaction(
        &self,
        _transaction: Transaction,
        _speculative_exec_block: Option<BlockHeader>,
    ) -> Result<(), Error> {
        todo!()
        // let request = BinaryRequest::TryAcceptTransaction {
        //     transaction,
        //     speculative_exec_at_block: speculative_exec_block,
        // };
        // let resp = self.dispatch(request).await?;
        // <()>::try_accept(&resp)?.ok_or(Error::NoResponseBody)
    }

    async fn exec_speculatively(
        &self,
        _state_root_hash: Digest,
        _block_time: Timestamp,
        _protocol_version: ProtocolVersion,
        _transaction: Transaction,
    ) -> Result<Option<(ExecutionResultV2, Vec<Message>)>, Error> {
        todo!()
        // let request = BinaryRequest::TrySpeculativeExec {
        //     transaction,
        //     state_root_hash,
        //     block_time,
        //     protocol_version,
        // };
        // let resp = self.dispatch(request).await?;
        // let result: Result<Option<(ExecutionResultV2, Vec<Message>)>, String> =
        //     bytesrepr::deserialize_from_slice(resp)
        //         .map_err(|err| Error::Deserialization(err.to_string()))?;
        // result.map_err(Error::SpeculativeExecFailed)
    }
}

fn parse_response<A>(resp: &BinaryResponse) -> Result<Option<A>, Error>
where
    A: FromBytes + PayloadEntity,
{
    match resp.header.returned_data_type() {
        Some(&found) if found == A::PAYLOAD_TYPE => {
            bytesrepr::deserialize_from_slice(&resp.payload)
                .map(Some)
                .map_err(|err| Error::Deserialization(err.to_string()))
        }
        Some(&other) => Err(Error::UnexpectedVariantReceived(other)),
        _ => Ok(None),
    }
}

fn parse_response_versioned<V1, V2>(resp: &BinaryResponse) -> Result<Option<V2>, Error>
where
    V1: DeserializeOwned + PayloadEntity,
    V2: FromBytes + PayloadEntity + From<V1>,
{
    match resp.header.returned_data_type() {
        Some(&found) if found == V1::PAYLOAD_TYPE => bincode::deserialize(&resp.payload)
            .map(|val| Some(V2::from(val)))
            .map_err(|err| Error::Deserialization(err.to_string())),
        Some(&found) if found == V2::PAYLOAD_TYPE => {
            bytesrepr::deserialize_from_slice(&resp.payload)
                .map(Some)
                .map_err(|err| Error::Deserialization(err.to_string()))
        }
        Some(&other) => Err(Error::UnexpectedVariantReceived(other)),
        _ => Err(Error::NoResponseBody),
    }
}

fn parse_response_bincode<A>(resp: &BinaryResponse) -> Result<Option<A>, Error>
where
    A: DeserializeOwned + PayloadEntity,
{
    match resp.header.returned_data_type() {
        Some(&found) if found == A::PAYLOAD_TYPE => bincode::deserialize(&resp.payload)
            .map(Some)
            .map_err(|err| Error::Deserialization(err.to_string())),
        Some(&other) => Err(Error::UnexpectedVariantReceived(other)),
        _ => Err(Error::NoResponseBody),
    }
}

trait PayloadEntity {
    const PAYLOAD_TYPE: PayloadType;
}

impl PayloadEntity for Transaction {
    const PAYLOAD_TYPE: PayloadType = PayloadType::Transaction;
}

impl PayloadEntity for Deploy {
    const PAYLOAD_TYPE: PayloadType = PayloadType::Deploy;
}

impl PayloadEntity for BlockHeader {
    const PAYLOAD_TYPE: PayloadType = PayloadType::BlockHeader;
}

impl PayloadEntity for BlockHeaderV1 {
    const PAYLOAD_TYPE: PayloadType = PayloadType::BlockHeaderV1;
}

impl PayloadEntity for BlockBody {
    const PAYLOAD_TYPE: PayloadType = PayloadType::BlockBody;
}

impl PayloadEntity for BlockBodyV1 {
    const PAYLOAD_TYPE: PayloadType = PayloadType::BlockBodyV1;
}

impl PayloadEntity for ExecutionResult {
    const PAYLOAD_TYPE: PayloadType = PayloadType::ExecutionResult;
}

impl PayloadEntity for FinalizedApprovals {
    const PAYLOAD_TYPE: PayloadType = PayloadType::FinalizedApprovals;
}

impl PayloadEntity for FinalizedDeployApprovals {
    const PAYLOAD_TYPE: PayloadType = PayloadType::FinalizedDeployApprovals;
}

impl PayloadEntity for BlockHashAndHeight {
    const PAYLOAD_TYPE: PayloadType = PayloadType::BlockHashAndHeight;
}

impl PayloadEntity for ExecutionResultV1 {
    const PAYLOAD_TYPE: PayloadType = PayloadType::ExecutionResultV1;
}

impl PayloadEntity for Peers {
    const PAYLOAD_TYPE: PayloadType = PayloadType::Peers;
}

impl PayloadEntity for BlockSignatures {
    const PAYLOAD_TYPE: PayloadType = PayloadType::BlockSignatures;
}

impl PayloadEntity for Vec<Transfer> {
    const PAYLOAD_TYPE: PayloadType = PayloadType::VecTransfers;
}

impl PayloadEntity for BlockHash {
    const PAYLOAD_TYPE: PayloadType = PayloadType::BlockHash;
}

impl PayloadEntity for HighestBlockSequenceCheckResult {
    const PAYLOAD_TYPE: PayloadType = PayloadType::HighestBlockSequenceCheckResult;
}

impl PayloadEntity for Vec<u8> {
    const PAYLOAD_TYPE: PayloadType = PayloadType::VecU8;
}
