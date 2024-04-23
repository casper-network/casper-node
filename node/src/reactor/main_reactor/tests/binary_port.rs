use std::{
    collections::{BTreeMap, HashMap},
    convert::{TryFrom, TryInto},
    iter,
    sync::Arc,
    time::Duration,
};

use casper_binary_port::{
    BalanceResponse, BinaryMessage, BinaryMessageCodec, BinaryRequest, BinaryRequestHeader,
    BinaryResponse, BinaryResponseAndRequest, ConsensusStatus, ConsensusValidatorChanges,
    DictionaryItemIdentifier, DictionaryQueryResult, ErrorCode, GetRequest, GetTrieFullResult,
    GlobalStateQueryResult, GlobalStateRequest, InformationRequest, InformationRequestTag,
    LastProgress, NetworkName, NodeStatus, PayloadType, PurseIdentifier, ReactorStateName,
    RecordId, Uptime,
};
use casper_storage::global_state::state::CommitProvider;
use casper_types::{
    account::{AccountHash, ActionThresholds, AssociatedKeys},
    addressable_entity::{NamedKeyAddr, NamedKeyValue},
    bytesrepr::{FromBytes, ToBytes},
    execution::{Effects, TransformKindV2, TransformV2},
    testing::TestRng,
    Account, AvailableBlockRange, Block, BlockHash, BlockHeader, BlockIdentifier,
    BlockSynchronizerStatus, CLValue, CLValueDictionary, ChainspecRawBytes, DictionaryAddr, Digest,
    EntityAddr, GlobalStateIdentifier, Key, KeyTag, NextUpgrade, Peers, ProtocolVersion, SecretKey,
    SignedBlock, StoredValue, Transaction, TransactionV1Builder, Transfer, URef, U512,
};
use futures::{SinkExt, StreamExt};
use rand::Rng;
use tokio::{net::TcpStream, time::timeout};
use tokio_util::codec::Framed;

use crate::{
    reactor::{main_reactor::MainReactor, Runner},
    testing::{
        self, filter_reactor::FilterReactor, network::TestingNetwork, ConditionCheckReactor,
    },
    types::NodeId,
};

use super::{InitialStakes, TestFixture};

const GUARANTEED_BLOCK_HEIGHT: u64 = 2;

const TEST_DICT_NAME: &str = "test_dict";
const TEST_DICT_ITEM_KEY: &str = "test_key";
const MESSAGE_SIZE: u32 = 1024 * 1024 * 10;

struct TestData {
    rng: TestRng,
    chainspec_raw_bytes: ChainspecRawBytes,
    highest_block: Block,
    secret_signing_key: Arc<SecretKey>,
    state_root_hash: Digest,
    test_account_hash: AccountHash,
    test_entity_addr: EntityAddr,
    test_dict_seed_uref: URef,
}

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
    Framed<TcpStream, BinaryMessageCodec>,
    (
        impl futures::Future<Output = (TestingNetwork<FilterReactor<MainReactor>>, TestRng)>,
        TestData,
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
        .main_reactor()
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
                .read_highest_block()
        })
        .expect("should have highest block");

    let effects = test_effects(&mut rng);

    let state_root_hash = first_node
        .main_reactor()
        .contract_runtime()
        .data_access_layer()
        .commit(*highest_block.state_root_hash(), effects.effects)
        .expect("should commit effects");

    // Get the binary port address.
    let binary_port_addr = first_node
        .main_reactor()
        .binary_port
        .bind_address()
        .expect("should be bound");

    // We let the entire network run in the background, until our request completes.
    let finish_cranking = fixture.run_until_stopped(rng.create_child());

    // Set-up client.
    let address = format!("localhost:{}", binary_port_addr.port());
    let stream = TcpStream::connect(address.clone())
        .await
        .expect("should create stream");

    (
        Framed::new(stream, BinaryMessageCodec::new(MESSAGE_SIZE)),
        (
            finish_cranking,
            TestData {
                rng,
                chainspec_raw_bytes,
                highest_block,
                secret_signing_key,
                state_root_hash,
                test_account_hash: effects.test_account_hash,
                test_entity_addr: effects.test_entity_addr,
                test_dict_seed_uref: effects.test_dict_seed_uref,
            },
        ),
    )
}

fn test_effects(rng: &mut TestRng) -> TestEffects {
    // we set up some basic data for global state tests, including an account and a dictionary
    let account_hash = AccountHash::new(rng.gen());
    let entity_addr = rng.gen();
    let dict_seed_uref = rng.gen();
    let dict_key = Key::dictionary(dict_seed_uref, TEST_DICT_ITEM_KEY.as_bytes());
    let dict_value = CLValueDictionary::new(
        CLValue::from_t(0).unwrap(),
        dict_seed_uref.addr().to_vec(),
        TEST_DICT_ITEM_KEY.as_bytes().to_vec(),
    );
    let main_purse = rng.gen();

    let mut effects = Effects::new();
    effects.push(TransformV2::new(
        dict_key,
        TransformKindV2::Write(StoredValue::CLValue(CLValue::from_t(dict_value).unwrap())),
    ));
    effects.push(TransformV2::new(
        Key::Account(account_hash),
        TransformKindV2::Write(StoredValue::Account(Account::new(
            account_hash,
            iter::once((TEST_DICT_NAME.to_owned(), Key::URef(dict_seed_uref)))
                .collect::<BTreeMap<_, _>>()
                .into(),
            main_purse,
            AssociatedKeys::default(),
            ActionThresholds::default(),
        ))),
    ));
    effects.push(TransformV2::new(
        Key::NamedKey(
            NamedKeyAddr::new_from_string(entity_addr, TEST_DICT_NAME.to_owned())
                .expect("should create named key addr"),
        ),
        TransformKindV2::Write(StoredValue::NamedKey(
            NamedKeyValue::from_concrete_values(
                Key::URef(dict_seed_uref),
                TEST_DICT_NAME.to_owned(),
            )
            .expect("should create named key value"),
        )),
    ));
    effects.push(TransformV2::new(
        Key::Balance(main_purse.addr()),
        TransformKindV2::Write(StoredValue::CLValue(
            CLValue::from_t(U512::one()).expect("should create CLValue"),
        )),
    ));
    TestEffects {
        effects,
        test_account_hash: account_hash,
        test_entity_addr: entity_addr,
        test_dict_seed_uref: dict_seed_uref,
    }
}

struct TestEffects {
    effects: Effects,
    test_account_hash: AccountHash,
    test_entity_addr: EntityAddr,
    test_dict_seed_uref: URef,
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
        mut client,
        (
            finish_cranking,
            TestData {
                mut rng,
                chainspec_raw_bytes: network_chainspec_raw_bytes,
                highest_block,
                secret_signing_key,
                state_root_hash,
                test_account_hash,
                test_entity_addr,
                test_dict_seed_uref,
            },
        ),
    ) = setup().await;

    let test_cases = &[
        block_header_info(*highest_block.hash()),
        signed_block_info(*highest_block.hash()),
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
        latest_switch_block_header(),
        node_status(),
        get_block_header(highest_block.clone_header()),
        get_block_transfers(highest_block.clone_header()),
        get_era_summary(state_root_hash),
        get_all_bids(state_root_hash),
        get_trie(state_root_hash),
        get_dictionary_item_by_addr(
            state_root_hash,
            *Key::dictionary(test_dict_seed_uref, TEST_DICT_ITEM_KEY.as_bytes())
                .as_dictionary()
                .unwrap(),
        ),
        get_dictionary_item_by_seed_uref(
            state_root_hash,
            test_dict_seed_uref,
            TEST_DICT_ITEM_KEY.to_owned(),
        ),
        get_dictionary_item_by_legacy_named_key(
            state_root_hash,
            test_account_hash,
            TEST_DICT_NAME.to_owned(),
            TEST_DICT_ITEM_KEY.to_owned(),
        ),
        get_dictionary_item_by_named_key(
            state_root_hash,
            test_entity_addr,
            TEST_DICT_NAME.to_owned(),
            TEST_DICT_ITEM_KEY.to_owned(),
        ),
        try_spec_exec_invalid(&mut rng),
        try_accept_transaction_invalid(&mut rng),
        try_accept_transaction(&secret_signing_key),
        get_balance_by_block(*highest_block.hash()),
        get_balance_by_state_root(state_root_hash, test_account_hash),
    ];

    for TestCase {
        name,
        request,
        asserter,
    } in test_cases
    {
        let header = BinaryRequestHeader::new(ProtocolVersion::from_parts(2, 0, 0), request.tag());
        let header_bytes = ToBytes::to_bytes(&header).expect("should serialize");

        let original_request_bytes = header_bytes
            .iter()
            .chain(
                ToBytes::to_bytes(&request)
                    .expect("should serialize")
                    .iter(),
            )
            .cloned()
            .collect::<Vec<_>>();

        client
            .send(BinaryMessage::new(original_request_bytes.clone()))
            .await
            .expect("should send message");

        let response = timeout(Duration::from_secs(10), client.next())
            .await
            .unwrap_or_else(|err| panic!("{}: should complete without timeout: {}", name, err))
            .unwrap_or_else(|| panic!("{}: should have bytes", name))
            .unwrap_or_else(|err| panic!("{}: should have ok response: {}", name, err));
        let (binary_response_and_request, _): (BinaryResponseAndRequest, _) =
            FromBytes::from_bytes(response.payload()).expect("should deserialize response");

        let mirrored_request_bytes = binary_response_and_request.original_request();
        assert_eq!(
            mirrored_request_bytes,
            original_request_bytes.as_slice(),
            "{}",
            name
        );
        assert!(asserter(binary_response_and_request.response()), "{}", name);
    }

    let (_net, _rng) = timeout(Duration::from_secs(10), finish_cranking)
        .await
        .unwrap_or_else(|_| panic!("should finish cranking without timeout"));
}

fn block_header_info(hash: BlockHash) -> TestCase {
    TestCase {
        name: "block_header_info",
        request: BinaryRequest::Get(
            InformationRequest::BlockHeader(Some(BlockIdentifier::Hash(hash)))
                .try_into()
                .expect("should convert"),
        ),
        asserter: Box::new(move |response| {
            assert_response::<BlockHeader, _>(response, Some(PayloadType::BlockHeader), |header| {
                header.block_hash() == hash
            })
        }),
    }
}

fn signed_block_info(hash: BlockHash) -> TestCase {
    TestCase {
        name: "signed_block_info",
        request: BinaryRequest::Get(
            InformationRequest::SignedBlock(Some(BlockIdentifier::Hash(hash)))
                .try_into()
                .expect("should convert"),
        ),
        asserter: Box::new(move |response| {
            assert_response::<SignedBlock, _>(response, Some(PayloadType::SignedBlock), |header| {
                *header.block().hash() == hash
            })
        }),
    }
}

fn peers() -> TestCase {
    TestCase {
        name: "peers",
        request: BinaryRequest::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::Peers.into(),
            key: vec![],
        }),
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
        request: BinaryRequest::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::Uptime.into(),
            key: vec![],
        }),
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
        request: BinaryRequest::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::LastProgress.into(),
            key: vec![],
        }),
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
        request: BinaryRequest::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::ReactorState.into(),
            key: vec![],
        }),
        asserter: Box::new(|response| {
            assert_response::<ReactorStateName, _>(
                response,
                Some(PayloadType::ReactorState),
                |reactor_state| matches!(reactor_state.into_inner().as_str(), "Validate"),
            )
        }),
    }
}

fn network_name() -> TestCase {
    TestCase {
        name: "network_name",
        request: BinaryRequest::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::NetworkName.into(),
            key: vec![],
        }),
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
        request: BinaryRequest::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::ConsensusValidatorChanges.into(),
            key: vec![],
        }),
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
        request: BinaryRequest::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::BlockSynchronizerStatus.into(),
            key: vec![],
        }),
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
        request: BinaryRequest::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::AvailableBlockRange.into(),
            key: vec![],
        }),
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
        request: BinaryRequest::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::NextUpgrade.into(),
            key: vec![],
        }),
        asserter: Box::new(|response| assert_response::<NextUpgrade, _>(response, None, |_| true)),
    }
}

fn consensus_status() -> TestCase {
    TestCase {
        name: "consensus_status",
        request: BinaryRequest::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::ConsensusStatus.into(),
            key: vec![],
        }),
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
        request: BinaryRequest::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::ChainspecRawBytes.into(),
            key: vec![],
        }),
        asserter: Box::new(move |response| {
            assert_response::<ChainspecRawBytes, _>(
                response,
                Some(PayloadType::ChainspecRawBytes),
                |crb| crb == network_chainspec_raw_bytes,
            )
        }),
    }
}

fn latest_switch_block_header() -> TestCase {
    TestCase {
        name: "latest_switch_block_header",
        request: BinaryRequest::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::LatestSwitchBlockHeader.into(),
            key: vec![],
        }),
        asserter: Box::new(move |response| {
            assert_response::<BlockHeader, _>(response, Some(PayloadType::BlockHeader), |header| {
                header.is_switch_block()
            })
        }),
    }
}

fn node_status() -> TestCase {
    TestCase {
        name: "node_status",
        request: BinaryRequest::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::NodeStatus.into(),
            key: vec![],
        }),
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
                        && matches!(node_status.reactor_state.into_inner().as_str(), "Validate")
                        && node_status.latest_switch_block_hash.is_some()
                },
            )
        }),
    }
}

fn get_block_header(expected: BlockHeader) -> TestCase {
    TestCase {
        name: "get_block_header",
        request: BinaryRequest::Get(GetRequest::Record {
            record_type_tag: RecordId::BlockHeader.into(),
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
        request: BinaryRequest::Get(GetRequest::Record {
            record_type_tag: RecordId::Transfer.into(),
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
        request: BinaryRequest::Get(GetRequest::State(Box::new(GlobalStateRequest::Item {
            state_identifier: Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
            base_key: Key::EraSummary,
            path: vec![],
        }))),
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
        request: BinaryRequest::Get(GetRequest::State(Box::new(GlobalStateRequest::AllItems {
            state_identifier: Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
            key_tag: KeyTag::Bid,
        }))),
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
        request: BinaryRequest::Get(GetRequest::State(Box::new(GlobalStateRequest::Trie {
            trie_key: digest,
        }))),
        asserter: Box::new(|response| {
            assert_response::<GetTrieFullResult, _>(
                response,
                Some(PayloadType::GetTrieFullResult),
                |res| matches!(res.into_inner(), Some(_)),
            )
        }),
    }
}

fn get_dictionary_item_by_addr(state_root_hash: Digest, addr: DictionaryAddr) -> TestCase {
    TestCase {
        name: "get_dictionary_item_by_addr",
        request: BinaryRequest::Get(GetRequest::State(Box::new(
            GlobalStateRequest::DictionaryItem {
                state_identifier: Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
                identifier: DictionaryItemIdentifier::DictionaryItem(addr),
            },
        ))),
        asserter: Box::new(move |response| {
            assert_response::<DictionaryQueryResult, _>(
                response,
                Some(PayloadType::DictionaryQueryResult),
                |res| {
                    matches!(
                        res.into_inner(),
                        (key, res) if key == Key::Dictionary(addr) && res.value().as_cl_value().is_some()
                    )
                },
            )
        }),
    }
}

fn get_dictionary_item_by_seed_uref(
    state_root_hash: Digest,
    seed_uref: URef,
    dictionary_item_key: String,
) -> TestCase {
    TestCase {
        name: "get_dictionary_item_by_seed_uref",
        request: BinaryRequest::Get(GetRequest::State(Box::new(
            GlobalStateRequest::DictionaryItem {
                state_identifier: Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
                identifier: DictionaryItemIdentifier::URef {
                    seed_uref,
                    dictionary_item_key: dictionary_item_key.clone(),
                },
            },
        ))),
        asserter: Box::new(move |response| {
            assert_response::<DictionaryQueryResult, _>(
                response,
                Some(PayloadType::DictionaryQueryResult),
                |res| {
                    let expected_key = Key::dictionary(seed_uref, dictionary_item_key.as_bytes());
                    matches!(
                        res.into_inner(),
                        (key, res) if key == expected_key && res.value().as_cl_value().is_some()
                    )
                },
            )
        }),
    }
}

fn get_dictionary_item_by_legacy_named_key(
    state_root_hash: Digest,
    hash: AccountHash,
    dictionary_name: String,
    dictionary_item_key: String,
) -> TestCase {
    TestCase {
        name: "get_dictionary_item_by_legacy_named_key",
        request: BinaryRequest::Get(GetRequest::State(Box::new(
            GlobalStateRequest::DictionaryItem {
                state_identifier: Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
                identifier: DictionaryItemIdentifier::AccountNamedKey {
                    hash,
                    dictionary_name,
                    dictionary_item_key,
                },
            },
        ))),
        asserter: Box::new(|response| {
            assert_response::<DictionaryQueryResult, _>(
                response,
                Some(PayloadType::DictionaryQueryResult),
                |res| matches!(res.into_inner(),(_, res) if res.value().as_cl_value().is_some()),
            )
        }),
    }
}

fn get_dictionary_item_by_named_key(
    state_root_hash: Digest,
    addr: EntityAddr,
    dictionary_name: String,
    dictionary_item_key: String,
) -> TestCase {
    TestCase {
        name: "get_dictionary_item_by_named_key",
        request: BinaryRequest::Get(GetRequest::State(Box::new(
            GlobalStateRequest::DictionaryItem {
                state_identifier: Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
                identifier: DictionaryItemIdentifier::EntityNamedKey {
                    addr,
                    dictionary_name,
                    dictionary_item_key,
                },
            },
        ))),
        asserter: Box::new(|response| {
            assert_response::<DictionaryQueryResult, _>(
                response,
                Some(PayloadType::DictionaryQueryResult),
                |res| matches!(res.into_inner(),(_, res) if res.value().as_cl_value().is_some()),
            )
        }),
    }
}

fn get_balance_by_block(block_hash: BlockHash) -> TestCase {
    TestCase {
        name: "get_balance_by_block",
        request: BinaryRequest::Get(GetRequest::State(Box::new(
            GlobalStateRequest::BalanceByBlock {
                block_identifier: Some(BlockIdentifier::Hash(block_hash)),
                purse_identifier: PurseIdentifier::Payment,
            },
        ))),
        asserter: Box::new(|response| {
            assert_response::<BalanceResponse, _>(
                response,
                Some(PayloadType::BalanceResponse),
                |res| res.available_balance == U512::zero(),
            )
        }),
    }
}

fn get_balance_by_state_root(state_root_hash: Digest, account_hash: AccountHash) -> TestCase {
    TestCase {
        name: "get_balance_by_state_root",
        request: BinaryRequest::Get(GetRequest::State(Box::new(
            GlobalStateRequest::BalanceByStateRoot {
                state_identifier: Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
                purse_identifier: PurseIdentifier::Account(account_hash),
            },
        ))),
        asserter: Box::new(|response| {
            assert_response::<BalanceResponse, _>(
                response,
                Some(PayloadType::BalanceResponse),
                |res| res.available_balance == U512::one(),
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
        asserter: Box::new(|response| ErrorCode::try_from(response.error_code()).is_ok()),
    }
}

fn try_spec_exec_invalid(rng: &mut TestRng) -> TestCase {
    let transaction = Transaction::V1(TransactionV1Builder::new_random(rng).build().unwrap());
    TestCase {
        name: "try_spec_exec_invalid",
        request: BinaryRequest::TrySpeculativeExec { transaction },
        asserter: Box::new(|response| ErrorCode::try_from(response.error_code()).is_ok()),
    }
}
