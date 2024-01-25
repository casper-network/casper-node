use std::{collections::HashMap, sync::Arc, time::Duration};

use casper_types::{
    binary_port::{
        BinaryRequest, BinaryRequestHeader, BinaryResponse, BinaryResponseAndRequest,
        ConsensusStatus, ConsensusValidatorChanges, DbId, ErrorCode, GetRequest, GetTrieFullResult,
        GlobalStateQueryResult, HighestBlockSequenceCheckResult, LastProgress, NetworkName,
        NodeStatus, NonPersistedDataRequest, PayloadType, Uptime,
    },
    bytesrepr::{FromBytes, ToBytes},
    testing::TestRng,
    AvailableBlockRange, BlockHash, BlockHashAndHeight, BlockHeader, BlockIdentifier,
    BlockSynchronizerStatus, ChainspecRawBytes, Digest, Key, KeyTag, NextUpgrade, Peers,
    ProtocolVersion, ReactorState, SecretKey, SignedBlock, StoredValue, Transaction,
    TransactionHash, TransactionV1Builder, Transfer,
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
        ChainspecRawBytes,
        SignedBlock,
        Arc<SecretKey>,
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
    let chainspec_raw_bytes = ChainspecRawBytes::clone(&fixture.chainspec_raw_bytes);
    let mut rng = fixture.rng_mut().create_child();
    let net = fixture.network_mut();
    net.settle_on(
        &mut rng,
        |nodes| network_produced_blocks(nodes, GUARANTEED_BLOCK_HEIGHT),
        Duration::from_secs(59),
    )
    .await;
    let (_, first_node) = net
        .nodes()
        .iter()
        .next()
        .expect("should have at least one node");
    let secret_signing_key = first_node
        .reactor()
        .inner()
        .inner()
        .validator_matrix
        .secret_signing_key()
        .clone();
    let highest_block = net
        .nodes()
        .iter()
        .find_map(|(_, runner)| {
            runner
                .reactor()
                .inner()
                .inner()
                .storage()
                .get_highest_signed_block(true)
        })
        .expect("should have highest block");

    // Get the binary port address.
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
            .with_max_request_payload_size(1024 * 1024 * 8)
            .with_max_response_payload_size(1024 * 1024 * 8),
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

    (
        client,
        (
            finish_cranking,
            rng,
            chainspec_raw_bytes,
            highest_block,
            secret_signing_key,
        ),
    )
}

struct TestCase {
    name: &'static str,
    request: BinaryRequest,
    asserter: Box<dyn Fn(&BinaryResponse) -> bool>,
}

fn validate_metadata(
    response: &BinaryResponse,
    expected_payload_type: Option<PayloadType>,
) -> bool {
    response.is_success()
        && response.returned_data_type_tag()
            == expected_payload_type.map(|payload_type| payload_type as u8)
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

    let (
        client,
        (finish_cranking, mut rng, network_chainspec_raw_bytes, highest_block, secret_key),
    ) = setup().await;

    let test_cases = &[
        block_hash_2_height(),
        highest_complete_block(),
        completed_blocks_contain_true(),
        completed_blocks_contain_false(),
        transaction_hash_2_block_hash_and_height(&mut rng),
        peers(),
        uptime(),
        last_progress(),
        reactor_state(),
        network_name(),
        consensus_validator_changes(),
        block_synchronizer_status(),
        available_block_range(),
        next_upgrade(),
        consensus_status(),
        chainspec_raw_bytes(network_chainspec_raw_bytes),
        node_status(),
        get_block_header(highest_block.block().clone_header()),
        get_block_transfers(highest_block.block().clone_header()),
        get_era_summary(*highest_block.block().state_root_hash()),
        get_all_bids(*highest_block.block().state_root_hash()),
        get_trie(*highest_block.block().state_root_hash()),
        try_accept_transaction(&secret_key),
        try_accept_transaction_invalid(&mut rng),
        try_spec_exec_invalid(&mut rng, highest_block.block().clone_header()),
    ];

    let header = BinaryRequestHeader::new(ProtocolVersion::V1_0_0);
    let header_bytes = ToBytes::to_bytes(&header).expect("should serialize");

    for TestCase {
        name,
        request,
        asserter,
    } in test_cases
    {
        let original_request_bytes = header_bytes
            .iter()
            .chain(
                ToBytes::to_bytes(&request)
                    .expect("should serialize")
                    .iter(),
            )
            .cloned()
            .collect::<Vec<_>>();

        let request_guard = client
            .create_request(ChannelId::new(0))
            .with_payload(original_request_bytes.clone().into())
            .queue_for_sending()
            .await;

        let response = request_guard
            .wait_for_response()
            .await
            .unwrap_or_else(|_| panic!("{}: should have ok response", name))
            .unwrap_or_else(|| panic!("{}: should have bytes", name));
        let (binary_response_and_request, _): (BinaryResponseAndRequest, _) =
            FromBytes::from_bytes(&response).expect("should deserialize response");

        let mirrored_request_bytes = binary_response_and_request.original_request();
        assert_eq!(
            mirrored_request_bytes,
            original_request_bytes.as_slice(),
            "{}",
            name
        );
        assert!(asserter(binary_response_and_request.response()), "{}", name);
    }

    let (_net, _rng) = finish_cranking.await;
}

fn block_hash_2_height() -> TestCase {
    TestCase {
        name: "block_hash_2_height",
        request: BinaryRequest::Get(GetRequest::NonPersistedData(
            NonPersistedDataRequest::BlockHeight2Hash {
                height: GUARANTEED_BLOCK_HEIGHT - 1,
            },
        )),
        asserter: Box::new(|response| {
            assert_response::<BlockHash, _>(response, Some(PayloadType::BlockHash), |_| true)
        }),
    }
}

fn highest_complete_block() -> TestCase {
    TestCase {
        name: "highest_complete_block",
        request: BinaryRequest::Get(GetRequest::NonPersistedData(
            NonPersistedDataRequest::HighestCompleteBlock,
        )),
        asserter: Box::new(|response| {
            assert_response::<BlockHashAndHeight, _>(
                response,
                Some(PayloadType::BlockHashAndHeight),
                |block_data| block_data.block_height() >= GUARANTEED_BLOCK_HEIGHT,
            )
        }),
    }
}

fn completed_blocks_contain_true() -> TestCase {
    TestCase {
        name: "completed_blocks_contain_true",
        request: BinaryRequest::Get(GetRequest::NonPersistedData(
            NonPersistedDataRequest::CompletedBlocksContain {
                block_identifier: BlockIdentifier::Height(GUARANTEED_BLOCK_HEIGHT - 1),
            },
        )),
        asserter: Box::new(|response| {
            assert_response::<HighestBlockSequenceCheckResult, _>(
                response,
                Some(PayloadType::HighestBlockSequenceCheckResult),
                HighestBlockSequenceCheckResult::into_inner,
            )
        }),
    }
}

fn completed_blocks_contain_false() -> TestCase {
    TestCase {
        name: "completed_blocks_contain_false",
        request: BinaryRequest::Get(GetRequest::NonPersistedData(
            NonPersistedDataRequest::CompletedBlocksContain {
                block_identifier: BlockIdentifier::Height(GUARANTEED_BLOCK_HEIGHT + 1000),
            },
        )),
        asserter: Box::new(|response| {
            assert_response::<HighestBlockSequenceCheckResult, _>(
                response,
                Some(PayloadType::HighestBlockSequenceCheckResult),
                |res| !res.into_inner(),
            )
        }),
    }
}

fn transaction_hash_2_block_hash_and_height(rng: &mut TestRng) -> TestCase {
    TestCase {
        name: "transaction_hash_2_block_hash_and_height",
        request: BinaryRequest::Get(GetRequest::NonPersistedData(
            NonPersistedDataRequest::TransactionHash2BlockHashAndHeight {
                // TODO: Add similar test case but with an existing transaction
                transaction_hash: TransactionHash::random(rng),
            },
        )),
        asserter: Box::new(|response| {
            assert_response::<Option<BlockHashAndHeight>, _>(response, None, |_| true)
        }),
    }
}

fn peers() -> TestCase {
    TestCase {
        name: "peers",
        request: BinaryRequest::Get(GetRequest::NonPersistedData(NonPersistedDataRequest::Peers)),
        asserter: Box::new(|response| {
            assert_response::<Peers, _>(response, Some(PayloadType::Peers), |peers| {
                !peers.into_inner().is_empty()
            })
        }),
    }
}

fn uptime() -> TestCase {
    TestCase {
        name: "uptime",
        request: BinaryRequest::Get(GetRequest::NonPersistedData(
            NonPersistedDataRequest::Uptime,
        )),
        asserter: Box::new(|response| {
            assert_response::<Uptime, _>(response, Some(PayloadType::Uptime), |uptime| {
                uptime.into_inner() > 0
            })
        }),
    }
}

fn last_progress() -> TestCase {
    TestCase {
        name: "last_progress",
        request: BinaryRequest::Get(GetRequest::NonPersistedData(
            NonPersistedDataRequest::LastProgress,
        )),
        asserter: Box::new(|response| {
            assert_response::<LastProgress, _>(
                response,
                Some(PayloadType::LastProgress),
                |last_progress| last_progress.into_inner().millis() > 0,
            )
        }),
    }
}

fn reactor_state() -> TestCase {
    TestCase {
        name: "reactor_state",
        request: BinaryRequest::Get(GetRequest::NonPersistedData(
            NonPersistedDataRequest::ReactorState,
        )),
        asserter: Box::new(|response| {
            assert_response::<ReactorState, _>(
                response,
                Some(PayloadType::ReactorState),
                |reactor_state| matches!(reactor_state, ReactorState::Validate),
            )
        }),
    }
}

fn network_name() -> TestCase {
    TestCase {
        name: "network_name",
        request: BinaryRequest::Get(GetRequest::NonPersistedData(
            NonPersistedDataRequest::NetworkName,
        )),
        asserter: Box::new(|response| {
            assert_response::<NetworkName, _>(
                response,
                Some(PayloadType::NetworkName),
                |network_name| &network_name.into_inner() == "casper-example",
            )
        }),
    }
}

fn consensus_validator_changes() -> TestCase {
    TestCase {
        name: "consensus_validator_changes",
        request: BinaryRequest::Get(GetRequest::NonPersistedData(
            NonPersistedDataRequest::ConsensusValidatorChanges,
        )),
        asserter: Box::new(|response| {
            assert_response::<ConsensusValidatorChanges, _>(
                response,
                Some(PayloadType::ConsensusValidatorChanges),
                |cvc| cvc.into_inner().is_empty(),
            )
        }),
    }
}

fn block_synchronizer_status() -> TestCase {
    TestCase {
        name: "block_synchronizer_status",
        request: BinaryRequest::Get(GetRequest::NonPersistedData(
            NonPersistedDataRequest::BlockSynchronizerStatus,
        )),
        asserter: Box::new(|response| {
            assert_response::<BlockSynchronizerStatus, _>(
                response,
                Some(PayloadType::BlockSynchronizerStatus),
                |bss| bss.historical().is_none() && bss.forward().is_none(),
            )
        }),
    }
}

fn available_block_range() -> TestCase {
    TestCase {
        name: "available_block_range",
        request: BinaryRequest::Get(GetRequest::NonPersistedData(
            NonPersistedDataRequest::AvailableBlockRange,
        )),
        asserter: Box::new(|response| {
            assert_response::<AvailableBlockRange, _>(
                response,
                Some(PayloadType::AvailableBlockRange),
                |abr| abr.low() == 0 && abr.high() >= GUARANTEED_BLOCK_HEIGHT,
            )
        }),
    }
}

fn next_upgrade() -> TestCase {
    TestCase {
        name: "next_upgrade",
        request: BinaryRequest::Get(GetRequest::NonPersistedData(
            NonPersistedDataRequest::NextUpgrade,
        )),
        asserter: Box::new(|response| assert_response::<NextUpgrade, _>(response, None, |_| true)),
    }
}

fn consensus_status() -> TestCase {
    TestCase {
        name: "consensus_status",
        request: BinaryRequest::Get(GetRequest::NonPersistedData(
            NonPersistedDataRequest::ConsensusStatus,
        )),
        asserter: Box::new(|response| {
            assert_response::<ConsensusStatus, _>(
                response,
                Some(PayloadType::ConsensusStatus),
                |_| true,
            )
        }),
    }
}

fn chainspec_raw_bytes(network_chainspec_raw_bytes: ChainspecRawBytes) -> TestCase {
    TestCase {
        name: "chainspec_raw_bytes",
        request: BinaryRequest::Get(GetRequest::NonPersistedData(
            NonPersistedDataRequest::ChainspecRawBytes,
        )),
        asserter: Box::new(move |response| {
            assert_response::<ChainspecRawBytes, _>(
                response,
                Some(PayloadType::ChainspecRawBytes),
                |crb| crb == network_chainspec_raw_bytes,
            )
        }),
    }
}

fn node_status() -> TestCase {
    TestCase {
        name: "node_status",
        request: BinaryRequest::Get(GetRequest::NonPersistedData(
            NonPersistedDataRequest::NodeStatus,
        )),
        asserter: Box::new(move |response| {
            assert_response::<NodeStatus, _>(
                response,
                Some(PayloadType::NodeStatus),
                |node_status| {
                    !node_status.peers.into_inner().is_empty()
                        && node_status.chainspec_name == "casper-example"
                        && node_status.last_added_block_info.is_some()
                        && node_status.our_public_signing_key.is_some()
                        && node_status.block_sync.historical().is_none()
                        && node_status.block_sync.forward().is_none()
                        && matches!(node_status.reactor_state, ReactorState::Validate)
                },
            )
        }),
    }
}

fn get_block_header(expected: BlockHeader) -> TestCase {
    TestCase {
        name: "get_block_header",
        request: BinaryRequest::Get(GetRequest::Db {
            db_tag: DbId::BlockHeader.into(),
            key: expected.block_hash().to_bytes().unwrap(),
        }),
        asserter: Box::new(move |response| {
            assert_response::<BlockHeader, _>(response, Some(PayloadType::BlockHeader), |header| {
                header == expected
            })
        }),
    }
}

fn get_block_transfers(expected: BlockHeader) -> TestCase {
    TestCase {
        name: "get_block_transfers",
        request: BinaryRequest::Get(GetRequest::Db {
            db_tag: DbId::Transfer.into(),
            key: expected.block_hash().to_bytes().unwrap(),
        }),
        asserter: Box::new(move |response| {
            validate_metadata(response, Some(PayloadType::Transfers))
                && bincode::deserialize::<Vec<Transfer>>(response.payload()).is_ok()
        }),
    }
}

fn get_era_summary(state_root_hash: Digest) -> TestCase {
    TestCase {
        name: "get_era_summary",
        request: BinaryRequest::Get(GetRequest::State {
            state_root_hash,
            base_key: Key::EraSummary,
            path: vec![],
        }),
        asserter: Box::new(|response| {
            assert_response::<GlobalStateQueryResult, _>(
                response,
                Some(PayloadType::GlobalStateQueryResult),
                |res| {
                    let (value, _) = res.into_inner();
                    matches!(value, StoredValue::EraInfo(_))
                },
            )
        }),
    }
}

fn get_all_bids(state_root_hash: Digest) -> TestCase {
    TestCase {
        name: "get_all_bids",
        request: BinaryRequest::Get(GetRequest::AllValues {
            state_root_hash,
            key_tag: KeyTag::Bid,
        }),
        asserter: Box::new(|response| {
            assert_response::<Vec<StoredValue>, _>(
                response,
                Some(PayloadType::StoredValues),
                |res| res.iter().all(|v| matches!(v, StoredValue::BidKind(_))),
            )
        }),
    }
}

fn get_trie(digest: Digest) -> TestCase {
    TestCase {
        name: "get_trie",
        request: BinaryRequest::Get(GetRequest::Trie { trie_key: digest }),
        asserter: Box::new(|response| {
            assert_response::<GetTrieFullResult, _>(
                response,
                Some(PayloadType::GetTrieFullResult),
                |res| matches!(res.into_inner(), Some(_)),
            )
        }),
    }
}

fn try_accept_transaction(key: &SecretKey) -> TestCase {
    let transaction = Transaction::V1(
        TransactionV1Builder::new_targeting_invocable_entity_via_alias("Test", "call")
            .with_secret_key(key)
            .with_chain_name("casper-example")
            .build()
            .unwrap(),
    );
    TestCase {
        name: "try_accept_transaction",
        request: BinaryRequest::TryAcceptTransaction { transaction },
        asserter: Box::new(|response| response.error_code() == ErrorCode::NoError as u8),
    }
}

fn try_accept_transaction_invalid(rng: &mut TestRng) -> TestCase {
    let transaction = Transaction::V1(TransactionV1Builder::new_random(rng).build().unwrap());
    TestCase {
        name: "try_accept_transaction_invalid",
        request: BinaryRequest::TryAcceptTransaction { transaction },
        asserter: Box::new(|response| response.error_code() == ErrorCode::InvalidTransaction as u8),
    }
}

fn try_spec_exec_invalid(rng: &mut TestRng, header: BlockHeader) -> TestCase {
    let transaction = Transaction::V1(TransactionV1Builder::new_random(rng).build().unwrap());
    TestCase {
        name: "try_spec_exec_invalid",
        request: BinaryRequest::TrySpeculativeExec {
            state_root_hash: *header.state_root_hash(),
            block_time: header.timestamp(),
            protocol_version: header.protocol_version(),
            transaction,
            speculative_exec_at_block: header,
        },
        asserter: Box::new(|response| response.error_code() == ErrorCode::InvalidTransaction as u8),
    }
}
