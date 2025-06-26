use std::{
    collections::{BTreeMap, HashMap},
    convert::{TryFrom, TryInto},
    iter,
    sync::Arc,
    time::Duration,
};

use casper_binary_port::{
    AccountInformation, AddressableEntityInformation, BalanceResponse, BinaryMessage,
    BinaryMessageCodec, BinaryResponse, BinaryResponseAndRequest, Command, CommandHeader,
    ConsensusStatus, ConsensusValidatorChanges, ContractInformation, DictionaryItemIdentifier,
    DictionaryQueryResult, EntityIdentifier, EraIdentifier, ErrorCode, GetRequest,
    GetTrieFullResult, GlobalStateEntityQualifier, GlobalStateQueryResult, GlobalStateRequest,
    InformationRequest, InformationRequestTag, KeyPrefix, LastProgress, NetworkName, NodeStatus,
    PackageIdentifier, PurseIdentifier, ReactorStateName, RecordId, ResponseType, RewardResponse,
    Uptime, ValueWithProof,
};
use casper_storage::global_state::state::CommitProvider;
use casper_types::{
    account::AccountHash,
    addressable_entity::{ActionThresholds, AssociatedKeys, NamedKeyAddr, NamedKeyValue},
    bytesrepr::{Bytes, FromBytes, ToBytes},
    contracts::{ContractHash, ContractPackage, ContractPackageHash},
    execution::{Effects, TransformKindV2, TransformV2},
    system::auction::DelegatorKind,
    testing::TestRng,
    Account, AddressableEntity, AvailableBlockRange, Block, BlockHash, BlockHeader,
    BlockIdentifier, BlockSynchronizerStatus, BlockWithSignatures, ByteCode, ByteCodeAddr,
    ByteCodeHash, ByteCodeKind, CLValue, CLValueDictionary, ChainspecRawBytes, Contract,
    ContractRuntimeTag, ContractWasm, ContractWasmHash, DictionaryAddr, Digest, EntityAddr,
    EntityKind, EntityVersions, GlobalStateIdentifier, Key, KeyTag, NextUpgrade, Package,
    PackageAddr, PackageHash, Peers, ProtocolVersion, PublicKey, Rewards, SecretKey, StoredValue,
    Transaction, Transfer, URef, U512,
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
    types::{transaction::transaction_v1_builder::TransactionV1Builder, NodeId},
};

use crate::reactor::main_reactor::tests::{
    fixture::TestFixture, initial_stakes::InitialStakes, ERA_ONE,
};

const GUARANTEED_BLOCK_HEIGHT: u64 = 4;

const TEST_DICT_NAME: &str = "test_dict";
const TEST_DICT_ITEM_KEY: &str = "test_key";
const MESSAGE_SIZE: u32 = 1024 * 1024 * 10;

struct TestData {
    rng: TestRng,
    protocol_version: ProtocolVersion,
    chainspec_raw_bytes: ChainspecRawBytes,
    highest_block: Block,
    secret_signing_key: Arc<SecretKey>,
    state_root_hash: Digest,
    effects: TestEffects,
    era_one_validator: PublicKey,
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
    let era_end = first_node
        .main_reactor()
        .storage()
        .get_switch_block_by_era_id(&ERA_ONE)
        .expect("should not fail retrieving switch block")
        .expect("should have switch block")
        .clone_era_end()
        .expect("should have era end");
    let Rewards::V2(rewards) = era_end.rewards() else {
        panic!("should have rewards V2");
    };

    let effects = test_effects(&mut rng);

    let state_root_hash = first_node
        .main_reactor()
        .contract_runtime()
        .data_access_layer()
        .commit_effects(*highest_block.state_root_hash(), effects.effects.clone())
        .expect("should commit effects");

    // Get the binary port address.
    let binary_port_addr = first_node
        .main_reactor()
        .binary_port
        .bind_address()
        .expect("should be bound");

    let protocol_version = first_node.main_reactor().chainspec.protocol_version();
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
                protocol_version,
                chainspec_raw_bytes,
                highest_block,
                secret_signing_key,
                state_root_hash,
                effects,
                era_one_validator: rewards
                    .last_key_value()
                    .expect("should have at least one reward")
                    .0
                    .clone(),
            },
        ),
    )
}

fn test_effects(rng: &mut TestRng) -> TestEffects {
    // we set up some basic data for global state tests, including an account and a dictionary
    let pre_migration_account_hash = AccountHash::new(rng.gen());
    let post_migration_account_hash = AccountHash::new(rng.gen());
    let main_purse: URef = rng.gen();

    let pre_migration_contract_package_hash = ContractPackageHash::new(rng.gen());
    let pre_migration_contract_hash = ContractHash::new(rng.gen());
    let post_migration_contract_package_hash = ContractPackageHash::new(rng.gen());
    let post_migration_contract_hash = ContractHash::new(rng.gen());
    let wasm_hash = ContractWasmHash::new(rng.gen());

    let package_addr: PackageAddr = rng.gen();
    let package_access_key: URef = rng.gen();
    let entity_addr: EntityAddr = rng.gen();
    let entity_bytecode_hash: ByteCodeHash = ByteCodeHash::new(rng.gen());

    let dict_seed_uref: URef = rng.gen();
    let dict_key = Key::dictionary(dict_seed_uref, TEST_DICT_ITEM_KEY.as_bytes());
    let dict_value = CLValueDictionary::new(
        CLValue::from_t(rng.gen::<i32>()).unwrap(),
        dict_seed_uref.addr().to_vec(),
        TEST_DICT_ITEM_KEY.as_bytes().to_vec(),
    );

    let mut effects = Effects::new();

    effects.push(TransformV2::new(
        Key::Account(pre_migration_account_hash),
        TransformKindV2::Write(StoredValue::Account(Account::new(
            pre_migration_account_hash,
            iter::once((TEST_DICT_NAME.to_owned(), Key::URef(dict_seed_uref)))
                .collect::<BTreeMap<_, _>>()
                .into(),
            main_purse,
            Default::default(),
            Default::default(),
        ))),
    ));
    effects.push(TransformV2::new(
        Key::Account(post_migration_account_hash),
        TransformKindV2::Write(StoredValue::CLValue(
            CLValue::from_t(Key::AddressableEntity(entity_addr)).expect("should create CLValue"),
        )),
    ));
    effects.push(TransformV2::new(
        dict_key,
        TransformKindV2::Write(StoredValue::CLValue(CLValue::from_t(dict_value).unwrap())),
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

    effects.push(TransformV2::new(
        Key::Hash(pre_migration_contract_package_hash.value()),
        TransformKindV2::Write(StoredValue::ContractPackage(ContractPackage::new(
            package_access_key,
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        ))),
    ));
    effects.push(TransformV2::new(
        Key::Hash(post_migration_contract_package_hash.value()),
        TransformKindV2::Write(StoredValue::CLValue(
            CLValue::from_t((Key::SmartContract(package_addr), package_access_key))
                .expect("should create CLValue"),
        )),
    ));

    effects.push(TransformV2::new(
        Key::Hash(pre_migration_contract_hash.value()),
        TransformKindV2::Write(StoredValue::Contract(Contract::new(
            pre_migration_contract_package_hash,
            wasm_hash,
            Default::default(),
            Default::default(),
            ProtocolVersion::V2_0_0,
        ))),
    ));
    effects.push(TransformV2::new(
        Key::Hash(post_migration_contract_hash.value()),
        TransformKindV2::Write(StoredValue::CLValue(
            CLValue::from_t(Key::AddressableEntity(entity_addr)).expect("should create CLValue"),
        )),
    ));

    effects.push(TransformV2::new(
        Key::Hash(wasm_hash.value()),
        TransformKindV2::Write(StoredValue::ContractWasm(ContractWasm::new(
            rng.random_vec(10..100),
        ))),
    ));

    effects.push(TransformV2::new(
        Key::SmartContract(package_addr),
        TransformKindV2::Write(StoredValue::SmartContract(Package::new(
            EntityVersions::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        ))),
    ));
    effects.push(TransformV2::new(
        Key::AddressableEntity(entity_addr),
        TransformKindV2::Write(StoredValue::AddressableEntity(AddressableEntity::new(
            PackageHash::new(package_addr),
            entity_bytecode_hash,
            ProtocolVersion::V2_0_0,
            main_purse,
            AssociatedKeys::default(),
            ActionThresholds::default(),
            EntityKind::SmartContract(ContractRuntimeTag::VmCasperV1),
        ))),
    ));
    effects.push(TransformV2::new(
        Key::ByteCode(ByteCodeAddr::new_wasm_addr(entity_bytecode_hash.value())),
        TransformKindV2::Write(StoredValue::ByteCode(ByteCode::new(
            ByteCodeKind::V1CasperWasm,
            rng.random_vec(10..100),
        ))),
    ));

    TestEffects {
        effects,
        pre_migration_account_hash,
        post_migration_account_hash,
        pre_migration_contract_package_hash,
        post_migration_contract_package_hash,
        pre_migration_contract_hash,
        post_migration_contract_hash,
        package_addr,
        entity_addr,
        dict_seed_uref,
    }
}

struct TestEffects {
    effects: Effects,
    pre_migration_account_hash: AccountHash,
    post_migration_account_hash: AccountHash,
    pre_migration_contract_package_hash: ContractPackageHash,
    post_migration_contract_package_hash: ContractPackageHash,
    pre_migration_contract_hash: ContractHash,
    post_migration_contract_hash: ContractHash,
    package_addr: PackageAddr,
    entity_addr: EntityAddr,
    dict_seed_uref: URef,
}

struct TestCase {
    name: &'static str,
    request: Command,
    asserter: Box<dyn Fn(&BinaryResponse) -> bool>,
}

fn validate_metadata(
    response: &BinaryResponse,
    expected_payload_type: Option<ResponseType>,
) -> bool {
    response.is_success()
        && response.returned_data_type_tag()
            == expected_payload_type.map(|payload_type| payload_type as u8)
        && expected_payload_type.is_none_or(|_| !response.payload().is_empty())
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
    payload_type: Option<ResponseType>,
    validator: F,
) -> bool
where
    T: FromBytes,
    F: FnOnce(T) -> bool,
{
    validate_metadata(response, payload_type)
        && payload_type
            .is_none_or(|_| validate_deserialization::<T>(response).is_some_and(validator))
}

#[tokio::test]
async fn binary_port_component_handles_all_requests() {
    testing::init_logging();

    let (
        mut client,
        (
            finish_cranking,
            TestData {
                mut rng,
                protocol_version,
                chainspec_raw_bytes: network_chainspec_raw_bytes,
                highest_block,
                secret_signing_key,
                state_root_hash,
                effects,
                era_one_validator,
            },
        ),
    ) = setup().await;

    let test_cases = &[
        block_header_info(*highest_block.hash()),
        block_with_signatures_info(*highest_block.hash()),
        peers(),
        uptime(),
        last_progress(),
        reactor_state(),
        network_name(),
        consensus_validator_changes(),
        block_synchronizer_status(),
        available_block_range(highest_block.height()),
        next_upgrade(),
        consensus_status(),
        chainspec_raw_bytes(network_chainspec_raw_bytes),
        latest_switch_block_header(),
        node_status(protocol_version),
        get_block_header(highest_block.clone_header()),
        get_block_transfers(highest_block.clone_header()),
        get_era_summary(state_root_hash),
        get_all_bids(state_root_hash),
        get_trie(state_root_hash),
        get_dictionary_item_by_addr(
            state_root_hash,
            *Key::dictionary(effects.dict_seed_uref, TEST_DICT_ITEM_KEY.as_bytes())
                .as_dictionary()
                .unwrap(),
        ),
        get_dictionary_item_by_seed_uref(
            state_root_hash,
            effects.dict_seed_uref,
            TEST_DICT_ITEM_KEY.to_owned(),
        ),
        get_dictionary_item_by_legacy_named_key(
            state_root_hash,
            effects.pre_migration_account_hash,
            TEST_DICT_NAME.to_owned(),
            TEST_DICT_ITEM_KEY.to_owned(),
        ),
        get_dictionary_item_by_named_key(
            state_root_hash,
            effects.entity_addr,
            TEST_DICT_NAME.to_owned(),
            TEST_DICT_ITEM_KEY.to_owned(),
        ),
        try_spec_exec_invalid(&mut rng),
        try_accept_transaction_invalid(&mut rng),
        try_accept_transaction(&secret_signing_key),
        get_balance(state_root_hash, effects.pre_migration_account_hash),
        get_balance_account_not_found(state_root_hash),
        get_balance_purse_uref_not_found(state_root_hash),
        get_named_keys_by_prefix(state_root_hash, effects.entity_addr),
        get_reward(
            Some(EraIdentifier::Era(ERA_ONE)),
            era_one_validator.clone(),
            None,
        ),
        get_reward(
            Some(EraIdentifier::Block(BlockIdentifier::Height(1))),
            era_one_validator,
            None,
        ),
        get_protocol_version(protocol_version),
        get_entity(state_root_hash, effects.entity_addr),
        get_entity_without_bytecode(state_root_hash, effects.entity_addr),
        get_entity_pre_migration_account(state_root_hash, effects.pre_migration_account_hash),
        get_entity_post_migration_account(state_root_hash, effects.post_migration_account_hash),
        get_entity_pre_migration_contract(state_root_hash, effects.pre_migration_contract_hash),
        get_entity_post_migration_contract(state_root_hash, effects.post_migration_contract_hash),
        get_package(state_root_hash, effects.package_addr),
        get_package_pre_migration(state_root_hash, effects.pre_migration_contract_package_hash),
        get_package_post_migration(
            state_root_hash,
            effects.post_migration_contract_package_hash,
        ),
    ];

    for (
        index,
        TestCase {
            name,
            request,
            asserter,
        },
    ) in test_cases.iter().enumerate()
    {
        let original_request_bytes = {
            let header = CommandHeader::new(request.tag(), index as u16);
            let header_bytes = ToBytes::to_bytes(&header).expect("should serialize");
            let request_bytes = ToBytes::to_bytes(&request).expect("should serialize");

            [header_bytes, request_bytes].concat()
        };

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

        let bytes_sent_via_tcp = Bytes::from(original_request_bytes).to_bytes().unwrap();
        let mirrored_request_bytes = binary_response_and_request.request();
        assert_eq!(mirrored_request_bytes, bytes_sent_via_tcp, "{}", name);

        binary_response_and_request.request();

        assert!(asserter(binary_response_and_request.response()), "{}", name);
    }

    let (_net, _rng) = timeout(Duration::from_secs(10), finish_cranking)
        .await
        .unwrap_or_else(|_| panic!("should finish cranking without timeout"));
}

fn block_header_info(hash: BlockHash) -> TestCase {
    TestCase {
        name: "block_header_info",
        request: Command::Get(
            InformationRequest::BlockHeader(Some(BlockIdentifier::Hash(hash)))
                .try_into()
                .expect("should convert"),
        ),
        asserter: Box::new(move |response| {
            assert_response::<BlockHeader, _>(response, Some(ResponseType::BlockHeader), |header| {
                header.block_hash() == hash
            })
        }),
    }
}

fn block_with_signatures_info(hash: BlockHash) -> TestCase {
    TestCase {
        name: "block_with_signatures_info",
        request: Command::Get(
            InformationRequest::BlockWithSignatures(Some(BlockIdentifier::Hash(hash)))
                .try_into()
                .expect("should convert"),
        ),
        asserter: Box::new(move |response| {
            assert_response::<BlockWithSignatures, _>(
                response,
                Some(ResponseType::BlockWithSignatures),
                |header| *header.block().hash() == hash,
            )
        }),
    }
}

fn peers() -> TestCase {
    TestCase {
        name: "peers",
        request: Command::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::Peers.into(),
            key: vec![],
        }),
        asserter: Box::new(|response| {
            assert_response::<Peers, _>(response, Some(ResponseType::Peers), |peers| {
                !peers.into_inner().is_empty()
            })
        }),
    }
}

fn uptime() -> TestCase {
    TestCase {
        name: "uptime",
        request: Command::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::Uptime.into(),
            key: vec![],
        }),
        asserter: Box::new(|response| {
            assert_response::<Uptime, _>(response, Some(ResponseType::Uptime), |uptime| {
                uptime.into_inner() > 0
            })
        }),
    }
}

fn last_progress() -> TestCase {
    TestCase {
        name: "last_progress",
        request: Command::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::LastProgress.into(),
            key: vec![],
        }),
        asserter: Box::new(|response| {
            assert_response::<LastProgress, _>(
                response,
                Some(ResponseType::LastProgress),
                |last_progress| last_progress.into_inner().millis() > 0,
            )
        }),
    }
}

fn reactor_state() -> TestCase {
    TestCase {
        name: "reactor_state",
        request: Command::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::ReactorState.into(),
            key: vec![],
        }),
        asserter: Box::new(|response| {
            assert_response::<ReactorStateName, _>(
                response,
                Some(ResponseType::ReactorState),
                |reactor_state| matches!(reactor_state.into_inner().as_str(), "Validate"),
            )
        }),
    }
}

fn network_name() -> TestCase {
    TestCase {
        name: "network_name",
        request: Command::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::NetworkName.into(),
            key: vec![],
        }),
        asserter: Box::new(|response| {
            assert_response::<NetworkName, _>(
                response,
                Some(ResponseType::NetworkName),
                |network_name| &network_name.into_inner() == "casper-example",
            )
        }),
    }
}

fn consensus_validator_changes() -> TestCase {
    TestCase {
        name: "consensus_validator_changes",
        request: Command::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::ConsensusValidatorChanges.into(),
            key: vec![],
        }),
        asserter: Box::new(|response| {
            assert_response::<ConsensusValidatorChanges, _>(
                response,
                Some(ResponseType::ConsensusValidatorChanges),
                |cvc| cvc.into_inner().is_empty(),
            )
        }),
    }
}

fn block_synchronizer_status() -> TestCase {
    TestCase {
        name: "block_synchronizer_status",
        request: Command::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::BlockSynchronizerStatus.into(),
            key: vec![],
        }),
        asserter: Box::new(|response| {
            assert_response::<BlockSynchronizerStatus, _>(
                response,
                Some(ResponseType::BlockSynchronizerStatus),
                |bss| bss.historical().is_none() && bss.forward().is_none(),
            )
        }),
    }
}

fn available_block_range(expected_height: u64) -> TestCase {
    TestCase {
        name: "available_block_range",
        request: Command::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::AvailableBlockRange.into(),
            key: vec![],
        }),
        asserter: Box::new(move |response| {
            assert_response::<AvailableBlockRange, _>(
                response,
                Some(ResponseType::AvailableBlockRange),
                |abr| abr.low() == 0 && abr.high() >= expected_height,
            )
        }),
    }
}

fn next_upgrade() -> TestCase {
    TestCase {
        name: "next_upgrade",
        request: Command::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::NextUpgrade.into(),
            key: vec![],
        }),
        asserter: Box::new(|response| assert_response::<NextUpgrade, _>(response, None, |_| true)),
    }
}

fn consensus_status() -> TestCase {
    TestCase {
        name: "consensus_status",
        request: Command::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::ConsensusStatus.into(),
            key: vec![],
        }),
        asserter: Box::new(|response| {
            assert_response::<ConsensusStatus, _>(
                response,
                Some(ResponseType::ConsensusStatus),
                |_| true,
            )
        }),
    }
}

fn chainspec_raw_bytes(network_chainspec_raw_bytes: ChainspecRawBytes) -> TestCase {
    TestCase {
        name: "chainspec_raw_bytes",
        request: Command::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::ChainspecRawBytes.into(),
            key: vec![],
        }),
        asserter: Box::new(move |response| {
            assert_response::<ChainspecRawBytes, _>(
                response,
                Some(ResponseType::ChainspecRawBytes),
                |crb| crb == network_chainspec_raw_bytes,
            )
        }),
    }
}

fn latest_switch_block_header() -> TestCase {
    TestCase {
        name: "latest_switch_block_header",
        request: Command::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::LatestSwitchBlockHeader.into(),
            key: vec![],
        }),
        asserter: Box::new(move |response| {
            assert_response::<BlockHeader, _>(response, Some(ResponseType::BlockHeader), |header| {
                header.is_switch_block()
            })
        }),
    }
}

fn node_status(expected_version: ProtocolVersion) -> TestCase {
    TestCase {
        name: "node_status",
        request: Command::Get(GetRequest::Information {
            info_type_tag: InformationRequestTag::NodeStatus.into(),
            key: vec![],
        }),
        asserter: Box::new(move |response| {
            assert_response::<NodeStatus, _>(
                response,
                Some(ResponseType::NodeStatus),
                |node_status| {
                    node_status.protocol_version == expected_version
                        && !node_status.peers.into_inner().is_empty()
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
        request: Command::Get(GetRequest::Record {
            record_type_tag: RecordId::BlockHeader.into(),
            key: expected.block_hash().to_bytes().unwrap(),
        }),
        asserter: Box::new(move |response| {
            assert_response::<BlockHeader, _>(response, Some(ResponseType::BlockHeader), |header| {
                header == expected
            })
        }),
    }
}

fn get_block_transfers(expected: BlockHeader) -> TestCase {
    TestCase {
        name: "get_block_transfers",
        request: Command::Get(GetRequest::Record {
            record_type_tag: RecordId::Transfer.into(),
            key: expected.block_hash().to_bytes().unwrap(),
        }),
        asserter: Box::new(move |response| {
            validate_metadata(response, Some(ResponseType::Transfers))
                && bincode::deserialize::<Vec<Transfer>>(response.payload()).is_ok()
        }),
    }
}

fn get_era_summary(state_root_hash: Digest) -> TestCase {
    TestCase {
        name: "get_era_summary",
        request: Command::Get(GetRequest::State(Box::new(GlobalStateRequest::new(
            Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
            GlobalStateEntityQualifier::Item {
                base_key: Key::EraSummary,
                path: vec![],
            },
        )))),
        asserter: Box::new(|response| {
            assert_response::<GlobalStateQueryResult, _>(
                response,
                Some(ResponseType::GlobalStateQueryResult),
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
        request: Command::Get(GetRequest::State(Box::new(GlobalStateRequest::new(
            Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
            GlobalStateEntityQualifier::AllItems {
                key_tag: KeyTag::Bid,
            },
        )))),
        asserter: Box::new(|response| {
            assert_response::<Vec<StoredValue>, _>(
                response,
                Some(ResponseType::StoredValues),
                |res| res.iter().all(|v| matches!(v, StoredValue::BidKind(_))),
            )
        }),
    }
}

fn get_trie(digest: Digest) -> TestCase {
    TestCase {
        name: "get_trie",
        request: Command::Get(GetRequest::Trie { trie_key: digest }),
        asserter: Box::new(|response| {
            assert_response::<GetTrieFullResult, _>(
                response,
                Some(ResponseType::GetTrieFullResult),
                |res| res.into_inner().is_some(),
            )
        }),
    }
}

fn get_dictionary_item_by_addr(state_root_hash: Digest, addr: DictionaryAddr) -> TestCase {
    TestCase {
        name: "get_dictionary_item_by_addr",
        request: Command::Get(GetRequest::State(Box::new(GlobalStateRequest::new(
            Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
            GlobalStateEntityQualifier::DictionaryItem {
                identifier: DictionaryItemIdentifier::DictionaryItem(addr),
            },
        )))),
        asserter: Box::new(move |response| {
            assert_response::<DictionaryQueryResult, _>(
                response,
                Some(ResponseType::DictionaryQueryResult),
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
        request: Command::Get(GetRequest::State(Box::new(GlobalStateRequest::new(
            Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
            GlobalStateEntityQualifier::DictionaryItem {
                identifier: DictionaryItemIdentifier::URef {
                    seed_uref,
                    dictionary_item_key: dictionary_item_key.clone(),
                },
            },
        )))),
        asserter: Box::new(move |response| {
            assert_response::<DictionaryQueryResult, _>(
                response,
                Some(ResponseType::DictionaryQueryResult),
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
        request: Command::Get(GetRequest::State(Box::new(GlobalStateRequest::new(
            Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
            GlobalStateEntityQualifier::DictionaryItem {
                identifier: DictionaryItemIdentifier::AccountNamedKey {
                    hash,
                    dictionary_name,
                    dictionary_item_key,
                },
            },
        )))),
        asserter: Box::new(|response| {
            assert_response::<DictionaryQueryResult, _>(
                response,
                Some(ResponseType::DictionaryQueryResult),
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
        request: Command::Get(GetRequest::State(Box::new(GlobalStateRequest::new(
            Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
            GlobalStateEntityQualifier::DictionaryItem {
                identifier: DictionaryItemIdentifier::EntityNamedKey {
                    addr,
                    dictionary_name,
                    dictionary_item_key,
                },
            },
        )))),
        asserter: Box::new(|response| {
            assert_response::<DictionaryQueryResult, _>(
                response,
                Some(ResponseType::DictionaryQueryResult),
                |res| matches!(res.into_inner(),(_, res) if res.value().as_cl_value().is_some()),
            )
        }),
    }
}

fn get_balance(state_root_hash: Digest, account_hash: AccountHash) -> TestCase {
    TestCase {
        name: "get_balance",
        request: Command::Get(GetRequest::State(Box::new(GlobalStateRequest::new(
            Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
            GlobalStateEntityQualifier::Balance {
                purse_identifier: PurseIdentifier::Account(account_hash),
            },
        )))),
        asserter: Box::new(|response| {
            assert_response::<BalanceResponse, _>(
                response,
                Some(ResponseType::BalanceResponse),
                |res| res.available_balance == U512::one(),
            )
        }),
    }
}

fn get_balance_account_not_found(state_root_hash: Digest) -> TestCase {
    TestCase {
        name: "get_balance_account_not_found",
        request: Command::Get(GetRequest::State(Box::new(GlobalStateRequest::new(
            Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
            GlobalStateEntityQualifier::Balance {
                purse_identifier: PurseIdentifier::Account(AccountHash([9; 32])),
            },
        )))),
        asserter: Box::new(|response| response.error_code() == ErrorCode::PurseNotFound as u16),
    }
}

fn get_balance_purse_uref_not_found(state_root_hash: Digest) -> TestCase {
    TestCase {
        name: "get_balance_purse_uref_not_found",
        request: Command::Get(GetRequest::State(Box::new(GlobalStateRequest::new(
            Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
            GlobalStateEntityQualifier::Balance {
                purse_identifier: PurseIdentifier::Purse(URef::new([9; 32], Default::default())),
            },
        )))),
        asserter: Box::new(|response| response.error_code() == ErrorCode::PurseNotFound as u16),
    }
}

fn get_named_keys_by_prefix(state_root_hash: Digest, entity_addr: EntityAddr) -> TestCase {
    TestCase {
        name: "get_named_keys_by_prefix",
        request: Command::Get(GetRequest::State(Box::new(GlobalStateRequest::new(
            Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
            GlobalStateEntityQualifier::ItemsByPrefix {
                key_prefix: KeyPrefix::NamedKeysByEntity(entity_addr),
            },
        )))),
        asserter: Box::new(|response| {
            assert_response::<Vec<StoredValue>, _>(
                response,
                Some(ResponseType::StoredValues),
                |res| res.iter().all(|v| matches!(v, StoredValue::NamedKey(_))),
            )
        }),
    }
}

fn get_reward(
    era_identifier: Option<EraIdentifier>,
    validator: PublicKey,
    delegator: Option<DelegatorKind>,
) -> TestCase {
    let key = InformationRequest::Reward {
        era_identifier,
        validator: validator.into(),
        delegator: delegator.map(Box::new),
    };

    TestCase {
        name: "get_reward",
        request: Command::Get(GetRequest::Information {
            info_type_tag: key.tag().into(),
            key: key.to_bytes().expect("should serialize key"),
        }),
        asserter: Box::new(move |response| {
            assert_response::<RewardResponse, _>(response, Some(ResponseType::Reward), |reward| {
                // test fixture sets delegation rate to 0
                reward.amount() > U512::zero() && reward.delegation_rate() == 0
            })
        }),
    }
}

fn get_protocol_version(expected: ProtocolVersion) -> TestCase {
    let key = InformationRequest::ProtocolVersion;

    TestCase {
        name: "get_protocol_version",
        request: Command::Get(GetRequest::Information {
            info_type_tag: key.tag().into(),
            key: vec![],
        }),
        asserter: Box::new(move |response| {
            assert_response::<ProtocolVersion, _>(
                response,
                Some(ResponseType::ProtocolVersion),
                |version| expected == version,
            )
        }),
    }
}

fn get_entity(state_root_hash: Digest, entity_addr: EntityAddr) -> TestCase {
    let key = InformationRequest::Entity {
        state_identifier: Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
        identifier: EntityIdentifier::EntityAddr(entity_addr),
        include_bytecode: true,
    };

    TestCase {
        name: "get_entity",
        request: Command::Get(GetRequest::Information {
            info_type_tag: key.tag().into(),
            key: key.to_bytes().expect("should serialize key"),
        }),
        asserter: Box::new(|response| {
            assert_response::<AddressableEntityInformation, _>(
                response,
                Some(ResponseType::AddressableEntityInformation),
                |res| res.bytecode().is_some(),
            )
        }),
    }
}

fn get_entity_without_bytecode(state_root_hash: Digest, entity_addr: EntityAddr) -> TestCase {
    let key = InformationRequest::Entity {
        state_identifier: Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
        identifier: EntityIdentifier::EntityAddr(entity_addr),
        include_bytecode: false,
    };

    TestCase {
        name: "get_entity_without_bytecode",
        request: Command::Get(GetRequest::Information {
            info_type_tag: key.tag().into(),
            key: key.to_bytes().expect("should serialize key"),
        }),
        asserter: Box::new(|response| {
            assert_response::<AddressableEntityInformation, _>(
                response,
                Some(ResponseType::AddressableEntityInformation),
                |res| res.bytecode().is_none(),
            )
        }),
    }
}

fn get_entity_pre_migration_account(
    state_root_hash: Digest,
    account_hash: AccountHash,
) -> TestCase {
    let key = InformationRequest::Entity {
        state_identifier: Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
        identifier: EntityIdentifier::AccountHash(account_hash),
        include_bytecode: false,
    };

    TestCase {
        name: "get_entity_pre_migration_account",
        request: Command::Get(GetRequest::Information {
            info_type_tag: key.tag().into(),
            key: key.to_bytes().expect("should serialize key"),
        }),
        asserter: Box::new(move |response| {
            assert_response::<AccountInformation, _>(
                response,
                Some(ResponseType::AccountInformation),
                |res| res.account().account_hash() == account_hash,
            )
        }),
    }
}

fn get_entity_post_migration_account(
    state_root_hash: Digest,
    account_hash: AccountHash,
) -> TestCase {
    let key = InformationRequest::Entity {
        state_identifier: Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
        identifier: EntityIdentifier::AccountHash(account_hash),
        include_bytecode: false,
    };

    TestCase {
        name: "get_entity_post_migration_account",
        request: Command::Get(GetRequest::Information {
            info_type_tag: key.tag().into(),
            key: key.to_bytes().expect("should serialize key"),
        }),
        asserter: Box::new(move |response| {
            assert_response::<AddressableEntityInformation, _>(
                response,
                Some(ResponseType::AddressableEntityInformation),
                |_| true,
            )
        }),
    }
}

fn get_entity_pre_migration_contract(
    state_root_hash: Digest,
    contract_hash: ContractHash,
) -> TestCase {
    let key = InformationRequest::Entity {
        state_identifier: Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
        identifier: EntityIdentifier::ContractHash(contract_hash),
        include_bytecode: true,
    };

    TestCase {
        name: "get_entity_pre_migration_contract",
        request: Command::Get(GetRequest::Information {
            info_type_tag: key.tag().into(),
            key: key.to_bytes().expect("should serialize key"),
        }),
        asserter: Box::new(move |response| {
            assert_response::<ContractInformation, _>(
                response,
                Some(ResponseType::ContractInformation),
                |res| res.wasm().is_some(),
            )
        }),
    }
}

fn get_entity_post_migration_contract(
    state_root_hash: Digest,
    contract_hash: ContractHash,
) -> TestCase {
    let key = InformationRequest::Entity {
        state_identifier: Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
        identifier: EntityIdentifier::ContractHash(contract_hash),
        include_bytecode: true,
    };

    TestCase {
        name: "get_entity_post_migration_contract",
        request: Command::Get(GetRequest::Information {
            info_type_tag: key.tag().into(),
            key: key.to_bytes().expect("should serialize key"),
        }),
        asserter: Box::new(move |response| {
            assert_response::<AddressableEntityInformation, _>(
                response,
                Some(ResponseType::AddressableEntityInformation),
                |res| res.bytecode().is_some(),
            )
        }),
    }
}

fn get_package(state_root_hash: Digest, package_addr: PackageAddr) -> TestCase {
    let key = InformationRequest::Package {
        state_identifier: Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
        identifier: PackageIdentifier::PackageAddr(package_addr),
    };

    TestCase {
        name: "get_package",
        request: Command::Get(GetRequest::Information {
            info_type_tag: key.tag().into(),
            key: key.to_bytes().expect("should serialize key"),
        }),
        asserter: Box::new(move |response| {
            assert_response::<ValueWithProof<Package>, _>(
                response,
                Some(ResponseType::PackageWithProof),
                |_| true,
            )
        }),
    }
}

fn get_package_pre_migration(
    state_root_hash: Digest,
    contract_package_hash: ContractPackageHash,
) -> TestCase {
    let key = InformationRequest::Package {
        state_identifier: Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
        identifier: PackageIdentifier::ContractPackageHash(contract_package_hash),
    };

    TestCase {
        name: "get_package_pre_migration",
        request: Command::Get(GetRequest::Information {
            info_type_tag: key.tag().into(),
            key: key.to_bytes().expect("should serialize key"),
        }),
        asserter: Box::new(move |response| {
            assert_response::<ValueWithProof<ContractPackage>, _>(
                response,
                Some(ResponseType::ContractPackageWithProof),
                |_| true,
            )
        }),
    }
}

fn get_package_post_migration(
    state_root_hash: Digest,
    contract_package_hash: ContractPackageHash,
) -> TestCase {
    let key = InformationRequest::Package {
        state_identifier: Some(GlobalStateIdentifier::StateRootHash(state_root_hash)),
        identifier: PackageIdentifier::ContractPackageHash(contract_package_hash),
    };

    TestCase {
        name: "get_package_post_migration",
        request: Command::Get(GetRequest::Information {
            info_type_tag: key.tag().into(),
            key: key.to_bytes().expect("should serialize key"),
        }),
        asserter: Box::new(move |response| {
            assert_response::<ValueWithProof<Package>, _>(
                response,
                Some(ResponseType::PackageWithProof),
                |_| true,
            )
        }),
    }
}

fn try_accept_transaction(key: &SecretKey) -> TestCase {
    let transaction = Transaction::V1(
        TransactionV1Builder::new_targeting_invocable_entity_via_alias(
            "Test",
            "call",
            casper_types::TransactionRuntimeParams::VmCasperV1,
        )
        .with_secret_key(key)
        .with_chain_name("casper-example")
        .build()
        .unwrap(),
    );
    TestCase {
        name: "try_accept_transaction",
        request: Command::TryAcceptTransaction { transaction },
        asserter: Box::new(|response| response.error_code() == ErrorCode::NoError as u16),
    }
}

fn try_accept_transaction_invalid(rng: &mut TestRng) -> TestCase {
    let transaction = Transaction::V1(TransactionV1Builder::new_random(rng).build().unwrap());
    TestCase {
        name: "try_accept_transaction_invalid",
        request: Command::TryAcceptTransaction { transaction },
        asserter: Box::new(|response| ErrorCode::try_from(response.error_code()).is_ok()),
    }
}

fn try_spec_exec_invalid(rng: &mut TestRng) -> TestCase {
    let transaction = Transaction::V1(TransactionV1Builder::new_random(rng).build().unwrap());
    TestCase {
        name: "try_spec_exec_invalid",
        request: Command::TrySpeculativeExec { transaction },
        asserter: Box::new(|response| ErrorCode::try_from(response.error_code()).is_ok()),
    }
}

#[tokio::test]
async fn binary_port_component_rejects_requests_with_invalid_header_version() {
    testing::init_logging();

    let (mut client, (finish_cranking, _)) = setup().await;

    let request = Command::Get(GetRequest::Information {
        info_type_tag: InformationRequestTag::Uptime.into(),
        key: vec![],
    });

    let mut header = CommandHeader::new(request.tag(), 0);

    // Make the binary protocol version incompatible.
    header.set_binary_request_version(header.version() + 1);

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
        .unwrap_or_else(|_| panic!("should complete without timeout"))
        .unwrap_or_else(|| panic!("should have bytes"))
        .unwrap_or_else(|_| panic!("should have ok response"));
    let (binary_response_and_request, _): (BinaryResponseAndRequest, _) =
        FromBytes::from_bytes(response.payload()).expect("should deserialize response");

    assert_eq!(
        binary_response_and_request.response().error_code(),
        ErrorCode::CommandHeaderVersionMismatch as u16
    );

    let (_net, _rng) = timeout(Duration::from_secs(10), finish_cranking)
        .await
        .unwrap_or_else(|_| panic!("should finish cranking without timeout"));
}
