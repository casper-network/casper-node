use std::cell::RefCell;

use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, DEFAULT_BLOCK_TIME,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{
    bytesrepr::ToBytes,
    contract_messages::{MessagePayload, MessageSummary, MessageTopicHash, MessageTopicSummary},
    crypto, runtime_args, ContractHash, Digest, Key, RuntimeArgs, StoredValue,
};

const MESSAGE_EMITTER_INSTALLER_WASM: &str = "contract_messages_emitter.wasm";
const MESSAGE_EMITTER_PACKAGE_NAME: &str = "messages_emitter_package_name";
const MESSAGE_EMITTER_GENERIC_TOPIC: &str = "generic_messages";
const ENTRY_POINT_EMIT_MESSAGE: &str = "emit_message";
const ARG_MESSAGE_SUFFIX_NAME: &str = "message_suffix";

const EMITTER_MESSAGE_PREFIX: &str = "generic message: ";

fn install_messages_emitter_contract<'a>(
    builder: &'a RefCell<LmdbWasmTestBuilder>,
) -> (ContractHash, [u8; 32]) {
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
            &[MESSAGE_EMITTER_PACKAGE_NAME.into()],
        )
        .expect("should query");

    let message_emitter_package = if let StoredValue::ContractPackage(package) = query_result {
        package
    } else {
        panic!("Stored value is not a contract package: {:?}", query_result);
    };

    // Get the contract hash of the messages_emitter contract.
    let message_emitter_contract_hash = message_emitter_package
        .versions()
        .contract_hashes()
        .last()
        .expect("Should have contract hash");

    (
        *message_emitter_contract_hash,
        crypto::blake2b(MESSAGE_EMITTER_GENERIC_TOPIC),
    )
}

fn emit_message_with_suffix<'a>(
    builder: &'a RefCell<LmdbWasmTestBuilder>,
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

struct MessageEmitterQueryView<'a> {
    builder: &'a RefCell<LmdbWasmTestBuilder>,
    contract_hash: ContractHash,
    message_topic_hash: MessageTopicHash,
}

impl<'a> MessageEmitterQueryView<'a> {
    fn new(
        builder: &'a RefCell<LmdbWasmTestBuilder>,
        contract_hash: ContractHash,
        message_topic_hash: MessageTopicHash,
    ) -> Self {
        Self {
            builder,
            contract_hash,
            message_topic_hash,
        }
    }

    fn message_topic(&self) -> MessageTopicSummary {
        let query_result = self
            .builder
            .borrow_mut()
            .query(
                None,
                Key::message_topic(self.contract_hash.value(), self.message_topic_hash),
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
        message_index: u32,
        state_hash: Option<Digest>,
    ) -> Result<MessageSummary, String> {
        let query_result = self.builder.borrow_mut().query(
            state_hash,
            Key::message(
                self.contract_hash.value(),
                self.message_topic_hash,
                message_index,
            ),
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

    let (contract_hash, message_topic_hash) = install_messages_emitter_contract(&builder);
    let query_view = MessageEmitterQueryView::new(&builder, contract_hash, message_topic_hash);

    // Check that the topic exists for the installed contract.
    assert_eq!(query_view.message_topic().message_count(), 0);

    // Now call the entry point to emit some messages.
    emit_message_with_suffix(&builder, "test", &contract_hash, DEFAULT_BLOCK_TIME);
    let expected_message =
        MessagePayload::from_string(format!("{}{}", EMITTER_MESSAGE_PREFIX, "test"));
    let expected_message_hash = crypto::blake2b(expected_message.to_bytes().unwrap());
    let queried_message_summary = query_view
        .message_summary(0, None)
        .expect("should have value")
        .value();
    assert_eq!(expected_message_hash, queried_message_summary);
    assert_eq!(query_view.message_topic().message_count(), 1);

    // call again to emit a new message and check that the index in the topic incremented.
    emit_message_with_suffix(&builder, "test", &contract_hash, DEFAULT_BLOCK_TIME);
    let queried_message_summary = query_view
        .message_summary(1, None)
        .expect("should have value")
        .value();
    assert_eq!(expected_message_hash, queried_message_summary);
    assert_eq!(query_view.message_topic().message_count(), 2);

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
        .message_summary(0, None)
        .expect("should have value")
        .value();
    assert_eq!(expected_message_hash, queried_message_summary);
    assert_eq!(query_view.message_topic().message_count(), 1);

    // old messages should be pruned from tip and inaccessible at the latest state hash.
    assert!(query_view.message_summary(1, None).is_err());

    // old messages should still be discoverable at a state hash before pruning.
    assert!(query_view
        .message_summary(1, Some(first_block_state_hash))
        .is_ok());
}

#[ignore]
#[test]
fn should_emit_message_on_empty_topic_in_new_block() {
    let builder = RefCell::new(LmdbWasmTestBuilder::default());
    builder
        .borrow_mut()
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    let (contract_hash, message_topic_hash) = install_messages_emitter_contract(&builder);

    let query_view = MessageEmitterQueryView::new(&builder, contract_hash, message_topic_hash);
    assert_eq!(query_view.message_topic().message_count(), 0);

    emit_message_with_suffix(
        &builder,
        "new block time",
        &contract_hash,
        DEFAULT_BLOCK_TIME + 1,
    );
    assert_eq!(query_view.message_topic().message_count(), 1);
}
