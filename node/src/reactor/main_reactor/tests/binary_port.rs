use std::{collections::HashMap, time::Duration};

use casper_types::{
    binary_port::{
        binary_request::BinaryRequest, get::GetRequest,
        non_persistent_data_request::NonPersistedDataRequest,
        type_wrappers::HighestBlockSequenceCheckResult,
    },
    bytesrepr::{FromBytes, ToBytes},
    testing::TestRng,
    BinaryResponse, BinaryResponseAndRequest, BlockHash, BlockHashAndHeight, BlockIdentifier,
    PayloadType, TransactionHash,
};
use juliet::{
    io::IoCoreBuilder,
    protocol::ProtocolBuilder,
    rpc::{JulietRpcClient, RpcBuilder},
    ChannelConfiguration, ChannelId,
};
use tokio::net::TcpStream;
use tracing::error;

use crate::{
    reactor::{main_reactor::MainReactor, Runner},
    testing::{
        self, filter_reactor::FilterReactor, network::TestingNetwork, ConditionCheckReactor,
    },
    types::NodeId,
};

use super::{InitialStakes, TestFixture};

const GUARANTEED_BLOCK_HEIGHT: u64 = 2;

fn network_produced_blocks(
    nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<FilterReactor<MainReactor>>>>,
    block_count: u64,
) -> bool {
    nodes.values().all(|node| {
        node.reactor()
            .inner()
            .inner()
            .storage()
            .get_available_block_range()
            .high()
            >= block_count
    })
}

async fn setup() -> (
    JulietRpcClient<1>,
    (
        impl futures::Future<Output = (TestingNetwork<FilterReactor<MainReactor>>, TestRng)>,
        TestRng,
    ),
) {
    let mut fixture = TestFixture::new(
        InitialStakes::AllEqual {
            count: 4,
            stake: 100,
        },
        None,
    )
    .await;
    let mut rng = fixture.rng_mut().create_child();
    let net = fixture.network_mut();

    net.settle_on(
        &mut rng,
        |nodes| network_produced_blocks(nodes, GUARANTEED_BLOCK_HEIGHT),
        Duration::from_secs(59),
    )
    .await;

    // Get the binary port address..
    let binary_port_addr = net.nodes()[net.nodes().keys().next().unwrap()]
        .main_reactor()
        .binary_port
        .bind_address()
        .expect("should be bound");

    // We let the entire network run in the background, until our request completes.
    let finish_cranking = fixture.run_until_stopped(rng.create_child());

    // Set-up juliet client.
    let protocol_builder = ProtocolBuilder::<1>::with_default_channel_config(
        ChannelConfiguration::default()
            .with_request_limit(10)
            .with_max_request_payload_size(8192)
            .with_max_response_payload_size(8192),
    );
    let io_builder = IoCoreBuilder::new(protocol_builder).buffer_size(ChannelId::new(0), 4096);
    let rpc_builder = RpcBuilder::new(io_builder);
    let address = format!("localhost:{}", binary_port_addr.port());
    let stream = TcpStream::connect(address.clone())
        .await
        .expect("should create stream");
    let (reader, writer) = stream.into_split();
    let (client, mut server) = rpc_builder.build(reader, writer);

    // We are not using the server functionality, but still need to run it for IO reasons.
    tokio::spawn(async move {
        if let Err(err) = server.next_request().await {
            error!(%err, "server read error");
        }
    });

    (client, (finish_cranking, rng))
}

struct TestCase {
    request: BinaryRequest,
    asserter: Box<dyn Fn(&BinaryResponse) -> bool>,
}

fn validate_metadata(
    response: &BinaryResponse,
    expected_payload_type: Option<PayloadType>,
) -> bool {
    response.is_success()
        && response.returned_data_type() == expected_payload_type
        && expected_payload_type.map_or(true, |_| !response.payload().is_empty())
}

fn validate_deserialization<T>(response: &BinaryResponse) -> Option<T>
where
    T: FromBytes,
{
    FromBytes::from_bytes(response.payload())
        .ok()
        .map(|(data, remainder)| {
            assert!(remainder.is_empty());
            data
        })
}

fn assert_response<T, F>(
    response: &BinaryResponse,
    payload_type: Option<PayloadType>,
    validator: F,
) -> bool
where
    T: FromBytes,
    F: FnOnce(T) -> bool,
{
    validate_metadata(response, payload_type)
        && payload_type.map_or(true, |_| {
            validate_deserialization::<T>(response).map_or(false, validator)
        })
}

#[tokio::test]
async fn binary_port_component() {
    testing::init_logging();

    // Peers,
    // Uptime,
    // LastProgress,
    // ReactorState,
    // NetworkName,
    // ConsensusValidatorChanges,
    // BlockSynchronizerStatus,
    // AvailableBlockRange,
    // NextUpgrade,
    // ConsensusStatus,
    // ChainspecRawBytes,
    // NodeStatus,

    let (client, (finish_cranking, mut rng)) = setup().await;

    let test_cases = &[
        TestCase {
            request: BinaryRequest::Get(GetRequest::NonPersistedData(
                NonPersistedDataRequest::BlockHeight2Hash {
                    height: GUARANTEED_BLOCK_HEIGHT - 1,
                },
            )),
            asserter: Box::new(|response| {
                assert_response::<BlockHash, _>(response, Some(PayloadType::BlockHash), |_| true)
            }),
        },
        TestCase {
            request: BinaryRequest::Get(GetRequest::NonPersistedData(
                NonPersistedDataRequest::HighestCompleteBlock,
            )),
            asserter: Box::new(|response| {
                assert_response::<BlockHashAndHeight, _>(
                    response,
                    Some(PayloadType::BlockHashAndHeight),
                    |data| data.block_height() >= GUARANTEED_BLOCK_HEIGHT,
                )
            }),
        },
        TestCase {
            request: BinaryRequest::Get(GetRequest::NonPersistedData(
                NonPersistedDataRequest::CompletedBlocksContain {
                    block_identifier: BlockIdentifier::Height(GUARANTEED_BLOCK_HEIGHT - 1),
                },
            )),
            asserter: Box::new(|response| {
                assert_response::<HighestBlockSequenceCheckResult, _>(
                    response,
                    Some(PayloadType::HighestBlockSequenceCheckResult),
                    |HighestBlockSequenceCheckResult(result)| result,
                )
            }),
        },
        TestCase {
            request: BinaryRequest::Get(GetRequest::NonPersistedData(
                NonPersistedDataRequest::CompletedBlocksContain {
                    block_identifier: BlockIdentifier::Height(GUARANTEED_BLOCK_HEIGHT + 1000),
                },
            )),
            asserter: Box::new(|response| {
                assert_response::<HighestBlockSequenceCheckResult, _>(
                    response,
                    Some(PayloadType::HighestBlockSequenceCheckResult),
                    |HighestBlockSequenceCheckResult(result)| !result,
                )
            }),
        },
        TestCase {
            request: BinaryRequest::Get(GetRequest::NonPersistedData(
                NonPersistedDataRequest::TransactionHash2BlockHashAndHeight {
                    // TODO: Add similar test case but with an existing transaction
                    transaction_hash: TransactionHash::random(&mut rng),
                },
            )),
            asserter: Box::new(|response| {
                assert_response::<Option<BlockHashAndHeight>, _>(response, None, |_| true)
            }),
        },
    ];

    for TestCase { request, asserter } in test_cases {
        let original_request_bytes = ToBytes::to_bytes(&request).expect("should serialize");

        let request_guard = client
            .create_request(ChannelId::new(0))
            .with_payload(original_request_bytes.clone().into())
            .queue_for_sending()
            .await;

        let response = request_guard
            .wait_for_response()
            .await
            .expect("should have ok response")
            .expect("should have bytes");
        let (binary_response_and_request, _): (BinaryResponseAndRequest, _) =
            FromBytes::from_bytes(&response).expect("should deserialize response");

        let mirrored_request_bytes = binary_response_and_request.original_request();
        assert_eq!(mirrored_request_bytes, original_request_bytes.as_slice());
        assert!(asserter(binary_response_and_request.response()));
    }

    let (_net, _rng) = finish_cranking.await;
}
