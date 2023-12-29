use std::{collections::HashMap, time::Duration};

use casper_types::{
    binary_port::{
        binary_request::BinaryRequest, get::GetRequest,
        non_persistent_data_request::NonPersistedDataRequest,
    },
    bytesrepr::{FromBytes, ToBytes},
    testing::TestRng,
    BinaryResponse, BinaryResponseAndRequest, BlockHash, BlockHashAndHeight, PayloadType,
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

const MINIMUM_BLOCK_HEIGHT: u64 = 2;

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
    impl futures::Future<Output = (TestingNetwork<FilterReactor<MainReactor>>, TestRng)>,
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
        |nodes| network_produced_blocks(nodes, MINIMUM_BLOCK_HEIGHT),
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
    let finish_cranking = fixture.run_until_stopped(rng);

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

    (client, finish_cranking)
}

struct TestCase {
    request: BinaryRequest,
    validator: Box<dyn Fn(&BinaryResponse) -> bool>,
}

fn validate_metadata(response: &BinaryResponse, expected_payload_type: PayloadType) -> bool {
    response.is_success()
        && !response.payload().is_empty()
        && response.returned_data_type() == Some(expected_payload_type)
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

#[tokio::test]
async fn binary_port_component_success() {
    testing::init_logging();

    // CompletedBlocksContain,
    // TransactionHash2BlockHashAndHeight
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

    let test_cases = &[
        TestCase {
            request: BinaryRequest::Get(GetRequest::NonPersistedData(
                NonPersistedDataRequest::BlockHeight2Hash {
                    height: MINIMUM_BLOCK_HEIGHT - 1,
                },
            )),
            validator: Box::new(|response| {
                validate_metadata(response, PayloadType::BlockHash)
                    && validate_deserialization::<BlockHash>(response).is_some()
            }),
        },
        TestCase {
            request: BinaryRequest::Get(GetRequest::NonPersistedData(
                NonPersistedDataRequest::HighestCompleteBlock,
            )),
            validator: Box::new(|response| {
                validate_metadata(response, PayloadType::BlockHashAndHeight)
                    && validate_deserialization::<BlockHashAndHeight>(response)
                        .map_or(false, |data| data.block_height() >= MINIMUM_BLOCK_HEIGHT)
            }),
        },
    ];

    let (client, finish_cranking) = setup().await;

    for TestCase { request, validator } in test_cases {
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
        assert!(validator(binary_response_and_request.response()));
    }

    let (_net, _rng) = finish_cranking.await;
}

#[tokio::test]
async fn binary_port_component_errors() {
    todo!()
}
