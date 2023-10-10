use std::cell::RefCell;

use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, DEFAULT_BLOCK_TIME,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::engine_state::EngineConfigBuilder;
use casper_types::{
    bytesrepr::ToBytes,
    contract_messages::{MessageChecksum, MessagePayload, MessageTopicSummary, TopicNameHash},
    crypto, runtime_args, AddressableEntity, ContractHash, Digest, Key, MessageLimits, RuntimeArgs,
    StoredValue, WasmConfig,
};

const MESSAGE_EMITTER_INSTALLER_WASM: &str = "contract_messages_emitter.wasm";
const MESSAGE_EMITTER_UPGRADER_WASM: &str = "contract_messages_upgrader.wasm";
const MESSAGE_EMITTER_FROM_CONTRACT: &str = "contract_messages_from_account.wasm";
const MESSAGE_EMITTER_PACKAGE_HASH_KEY_NAME: &str = "messages_emitter_package_hash";
const MESSAGE_EMITTER_GENERIC_TOPIC: &str = "generic_messages";
const MESSAGE_EMITTER_UPGRADED_TOPIC: &str = "new_topic_after_upgrade";
const ENTRY_POINT_EMIT_MESSAGE: &str = "emit_message";
const ARG_TOPIC_NAME: &str = "topic_name";
const ENTRY_POINT_ADD_TOPIC: &str = "add_topic";
const ARG_MESSAGE_SUFFIX_NAME: &str = "message_suffix";

const EMITTER_MESSAGE_PREFIX: &str = "generic message: ";

fn install_messages_emitter_contract(builder: &RefCell<LmdbWasmTestBuilder>) -> ContractHash {
    // Request to install the contract that will be emitting messages.
    let install_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        MESSAGE_EMITTER_INSTALLER_WASM,
        RuntimeArgs::default(),
    )
    .build();

    // Execute the request to install the message emitting contract.
    // This will also register a topic for the contract to emit messages on.
    builder
        .borrow_mut()
        .exec(install_request)
        .expect_success()
        .commit();

    // Get the contract package for the messages_emitter.
    let query_result = builder
        .borrow_mut()
        .query(
            None,
            Key::from(*DEFAULT_ACCOUNT_ADDR),
            &[MESSAGE_EMITTER_PACKAGE_HASH_KEY_NAME.into()],
        )
        .expect("should query");

    let message_emitter_package = if let StoredValue::ContractPackage(package) = query_result {
        package
    } else {
        panic!("Stored value is not a contract package: {:?}", query_result);
    };

    // Get the contract hash of the messages_emitter contract.
    *message_emitter_package
        .versions()
        .contract_hashes()
        .last()
        .expect("Should have contract hash")
}

fn upgrade_messages_emitter_contract(builder: &RefCell<LmdbWasmTestBuilder>) -> ContractHash {
    let upgrade_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        MESSAGE_EMITTER_UPGRADER_WASM,
        RuntimeArgs::default(),
    )
    .build();

    // Execute the request to upgrade the message emitting contract.
    // This will also register a new topic for the contract to emit messages on.
    builder
        .borrow_mut()
        .exec(upgrade_request)
        .expect_success()
        .commit();

    // Get the contract package for the upgraded messages emitter contract.
    let query_result = builder
        .borrow_mut()
        .query(
            None,
            Key::from(*DEFAULT_ACCOUNT_ADDR),
            &[MESSAGE_EMITTER_PACKAGE_HASH_KEY_NAME.into()],
        )
        .expect("should query");

    let message_emitter_package = if let StoredValue::ContractPackage(package) = query_result {
        package
    } else {
        panic!("Stored value is not a contract package: {:?}", query_result);
    };

    // Get the contract hash of the latest version of the messages emitter contract.
    *message_emitter_package
        .versions()
        .contract_hashes()
        .last()
        .expect("Should have contract hash")
}

fn emit_message_with_suffix(
    builder: &RefCell<LmdbWasmTestBuilder>,
    suffix: &str,
    contract_hash: &ContractHash,
    block_time: u64,
) {
    let emit_message_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        *contract_hash,
        ENTRY_POINT_EMIT_MESSAGE,
        runtime_args! {
            ARG_MESSAGE_SUFFIX_NAME => suffix,
        },
    )
    .with_block_time(block_time)
    .build();

    builder
        .borrow_mut()
        .exec(emit_message_request)
        .expect_success()
        .commit();
}

struct ContractQueryView<'a> {
    builder: &'a RefCell<LmdbWasmTestBuilder>,
    contract_hash: ContractHash,
}

impl<'a> ContractQueryView<'a> {
    fn new(builder: &'a RefCell<LmdbWasmTestBuilder>, contract_hash: ContractHash) -> Self {
        Self {
            builder,
            contract_hash,
        }
    }

    fn entity(&self) -> AddressableEntity {
        let query_result = self
            .builder
            .borrow_mut()
            .query(None, Key::from(self.contract_hash), &[])
            .expect("should query");

        let entity = if let StoredValue::AddressableEntity(entity) = query_result {
            entity
        } else {
            panic!(
                "Stored value is not an adressable entity: {:?}",
                query_result
            );
        };

        entity
    }

    fn message_topic(&self, topic_name_hash: TopicNameHash) -> MessageTopicSummary {
        let query_result = self
            .builder
            .borrow_mut()
            .query(
                None,
                Key::message_topic(self.contract_hash.value(), topic_name_hash),
                &[],
            )
            .expect("should query");

        match query_result {
            StoredValue::MessageTopic(summary) => summary,
            _ => {
                panic!(
                    "Stored value is not a message topic summary: {:?}",
                    query_result
                );
            }
        }
    }

    fn message_summary(
        &self,
        topic_name_hash: TopicNameHash,
        message_index: u32,
        state_hash: Option<Digest>,
    ) -> Result<MessageChecksum, String> {
        let query_result = self.builder.borrow_mut().query(
            state_hash,
            Key::message(self.contract_hash.value(), topic_name_hash, message_index),
            &[],
        )?;

        match query_result {
            StoredValue::Message(summary) => Ok(summary),
            _ => panic!("Stored value is not a message summary: {:?}", query_result),
        }
    }
}

#[ignore]
#[test]
fn should_emit_messages() {
    let builder = RefCell::new(LmdbWasmTestBuilder::default());
    builder
        .borrow_mut()
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    let contract_hash = install_messages_emitter_contract(&builder);
    let query_view = ContractQueryView::new(&builder, contract_hash);
    let entity = query_view.entity();

    let (topic_name, message_topic_hash) = entity
        .message_topics()
        .iter()
        .next()
        .expect("should have at least one topic");

    assert_eq!(topic_name, &MESSAGE_EMITTER_GENERIC_TOPIC.to_string());
    // Check that the topic exists for the installed contract.
    assert_eq!(
        query_view
            .message_topic(*message_topic_hash)
            .message_count(),
        0
    );

    // Now call the entry point to emit some messages.
    emit_message_with_suffix(&builder, "test", &contract_hash, DEFAULT_BLOCK_TIME);
    let expected_message =
        MessagePayload::from_string(format!("{}{}", EMITTER_MESSAGE_PREFIX, "test"));
    let expected_message_hash = crypto::blake2b(expected_message.to_bytes().unwrap());
    let queried_message_summary = query_view
        .message_summary(*message_topic_hash, 0, None)
        .expect("should have value")
        .value();
    assert_eq!(expected_message_hash, queried_message_summary);
    assert_eq!(
        query_view
            .message_topic(*message_topic_hash)
            .message_count(),
        1
    );

    // call again to emit a new message and check that the index in the topic incremented.
    emit_message_with_suffix(&builder, "test", &contract_hash, DEFAULT_BLOCK_TIME);
    let queried_message_summary = query_view
        .message_summary(*message_topic_hash, 1, None)
        .expect("should have value")
        .value();
    assert_eq!(expected_message_hash, queried_message_summary);
    assert_eq!(
        query_view
            .message_topic(*message_topic_hash)
            .message_count(),
        2
    );

    let first_block_state_hash = builder.borrow().get_post_state_hash();

    // call to emit a new message but in another block.
    emit_message_with_suffix(
        &builder,
        "new block time",
        &contract_hash,
        DEFAULT_BLOCK_TIME + 1,
    );
    let expected_message =
        MessagePayload::from_string(format!("{}{}", EMITTER_MESSAGE_PREFIX, "new block time"));
    let expected_message_hash = crypto::blake2b(expected_message.to_bytes().unwrap());
    let queried_message_summary = query_view
        .message_summary(*message_topic_hash, 0, None)
        .expect("should have value")
        .value();
    assert_eq!(expected_message_hash, queried_message_summary);
    assert_eq!(
        query_view
            .message_topic(*message_topic_hash)
            .message_count(),
        1
    );

    // old messages should be pruned from tip and inaccessible at the latest state hash.
    assert!(query_view
        .message_summary(*message_topic_hash, 1, None)
        .is_err());

    // old messages should still be discoverable at a state hash before pruning.
    assert!(query_view
        .message_summary(*message_topic_hash, 1, Some(first_block_state_hash))
        .is_ok());
}

#[ignore]
#[test]
fn should_emit_message_on_empty_topic_in_new_block() {
    let builder = RefCell::new(LmdbWasmTestBuilder::default());
    builder
        .borrow_mut()
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    let contract_hash = install_messages_emitter_contract(&builder);
    let query_view = ContractQueryView::new(&builder, contract_hash);
    let entity = query_view.entity();

    let (_, message_topic_hash) = entity
        .message_topics()
        .iter()
        .next()
        .expect("should have at least one topic");

    assert_eq!(
        query_view
            .message_topic(*message_topic_hash)
            .message_count(),
        0
    );

    emit_message_with_suffix(
        &builder,
        "new block time",
        &contract_hash,
        DEFAULT_BLOCK_TIME + 1,
    );
    assert_eq!(
        query_view
            .message_topic(*message_topic_hash)
            .message_count(),
        1
    );
}

#[ignore]
#[test]
fn should_add_topics() {
    let builder = RefCell::new(LmdbWasmTestBuilder::default());
    builder
        .borrow_mut()
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);
    let contract_hash = install_messages_emitter_contract(&builder);
    let query_view = ContractQueryView::new(&builder, contract_hash);

    let add_topic_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        ENTRY_POINT_ADD_TOPIC,
        runtime_args! {
            ARG_TOPIC_NAME => "topic_1",
        },
    )
    .build();

    builder
        .borrow_mut()
        .exec(add_topic_request)
        .expect_success()
        .commit();

    let topic_1_hash = *query_view
        .entity()
        .message_topics()
        .get("topic_1")
        .expect("should have added topic `topic_1");
    assert_eq!(query_view.message_topic(topic_1_hash).message_count(), 0);

    let add_topic_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        ENTRY_POINT_ADD_TOPIC,
        runtime_args! {
            ARG_TOPIC_NAME => "topic_2",
        },
    )
    .build();

    builder
        .borrow_mut()
        .exec(add_topic_request)
        .expect_success()
        .commit();

    let topic_2_hash = *query_view
        .entity()
        .message_topics()
        .get("topic_2")
        .expect("should have added topic `topic_2");

    assert!(query_view
        .entity()
        .message_topics()
        .get("topic_1")
        .is_some());
    assert_eq!(query_view.message_topic(topic_1_hash).message_count(), 0);
    assert_eq!(query_view.message_topic(topic_2_hash).message_count(), 0);
}

#[ignore]
#[test]
fn should_not_add_duplicate_topics() {
    let builder = RefCell::new(LmdbWasmTestBuilder::default());
    builder
        .borrow_mut()
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    let contract_hash = install_messages_emitter_contract(&builder);
    let query_view = ContractQueryView::new(&builder, contract_hash);

    let entity = query_view.entity();
    let (first_topic_name, _) = entity
        .message_topics()
        .iter()
        .next()
        .expect("should have at least one topic");

    let add_topic_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        ENTRY_POINT_ADD_TOPIC,
        runtime_args! {
            ARG_TOPIC_NAME => first_topic_name,
        },
    )
    .build();

    builder
        .borrow_mut()
        .exec(add_topic_request)
        .expect_failure()
        .commit();
}

#[ignore]
#[test]
fn should_not_exceed_configured_limits() {
    let default_wasm_config = WasmConfig::default();
    let custom_engine_config = EngineConfigBuilder::default()
        .with_wasm_config(WasmConfig::new(
            default_wasm_config.max_memory,
            default_wasm_config.max_stack_height,
            default_wasm_config.opcode_costs(),
            default_wasm_config.storage_costs(),
            default_wasm_config.take_host_function_costs(),
            MessageLimits {
                max_topic_name_size: 32,
                max_message_size: 100,
                max_topics_per_contract: 2,
            },
        ))
        .build();

    let builder = RefCell::new(LmdbWasmTestBuilder::new_temporary_with_config(
        custom_engine_config,
    ));
    builder
        .borrow_mut()
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    let contract_hash = install_messages_emitter_contract(&builder);

    // if the topic larger than the limit, registering should fail.
    // string is 29 bytes + 4 bytes for size = 33 bytes > limit established above
    let too_large_topic_name = std::str::from_utf8(&[0x4du8; 29]).unwrap();
    let add_topic_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        ENTRY_POINT_ADD_TOPIC,
        runtime_args! {
            ARG_TOPIC_NAME => too_large_topic_name,
        },
    )
    .build();

    builder
        .borrow_mut()
        .exec(add_topic_request)
        .expect_failure()
        .commit();

    // if the topic name is equal to the limit, registering should work.
    // string is 28 bytes + 4 bytes for size = 32 bytes == limit established above
    let topic_name_at_limit = std::str::from_utf8(&[0x4du8; 28]).unwrap();
    let add_topic_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        ENTRY_POINT_ADD_TOPIC,
        runtime_args! {
            ARG_TOPIC_NAME => topic_name_at_limit,
        },
    )
    .build();

    builder
        .borrow_mut()
        .exec(add_topic_request)
        .expect_success()
        .commit();

    // Check that the max number of topics limit is enforced.
    // 2 topics are already registered, so registering another topic should
    // fail since the limit is already reached.
    let add_topic_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        ENTRY_POINT_ADD_TOPIC,
        runtime_args! {
            ARG_TOPIC_NAME => "topic_1",
        },
    )
    .build();

    builder
        .borrow_mut()
        .exec(add_topic_request)
        .expect_failure()
        .commit();

    // Check message size limit
    let large_message = std::str::from_utf8(&[0x4du8; 128]).unwrap();
    let emit_message_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        contract_hash,
        ENTRY_POINT_EMIT_MESSAGE,
        runtime_args! {
            ARG_MESSAGE_SUFFIX_NAME => large_message,
        },
    )
    .build();

    builder
        .borrow_mut()
        .exec(emit_message_request)
        .expect_failure()
        .commit();
}

#[ignore]
#[test]
fn should_carry_message_topics_on_upgraded_contract() {
    let builder = RefCell::new(LmdbWasmTestBuilder::default());
    builder
        .borrow_mut()
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    let _ = install_messages_emitter_contract(&builder);
    let contract_hash = upgrade_messages_emitter_contract(&builder);
    let query_view = ContractQueryView::new(&builder, contract_hash);

    let entity = query_view.entity();
    assert_eq!(entity.message_topics().len(), 2);
    let mut expected_topic_names = 0;
    for (topic_name, topic_hash) in entity.message_topics().iter() {
        if topic_name == MESSAGE_EMITTER_GENERIC_TOPIC
            || topic_name == MESSAGE_EMITTER_UPGRADED_TOPIC
        {
            expected_topic_names += 1;
        }

        assert_eq!(query_view.message_topic(*topic_hash).message_count(), 0);
    }
    assert_eq!(expected_topic_names, 2);
}

#[ignore]
#[test]
fn should_not_emit_messages_from_account() {
    let builder = RefCell::new(LmdbWasmTestBuilder::default());
    builder
        .borrow_mut()
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    // Request to run a deploy that tries to register a message topic without a contract.
    let install_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        MESSAGE_EMITTER_FROM_CONTRACT,
        RuntimeArgs::default(),
    )
    .build();

    // Expect to fail since topics can only be registered by stored contracts.
    builder
        .borrow_mut()
        .exec(install_request)
        .expect_failure()
        .commit();
}
