use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{
    bytesrepr::ToBytes,
    contract_messages::{MessagePayload, MessageTopicHash, MessageTopicSummary},
    crypto, runtime_args, ContractHash, Key, RuntimeArgs, StoredValue,
};

const MESSAGE_EMITTER_INSTALLER_WASM: &str = "contract_messages_emitter.wasm";
const MESSAGE_EMITTER_PACKAGE_NAME: &str = "messages_emitter_package_name";
const MESSAGE_EMITTER_GENERIC_TOPIC: &str = "generic_messages";
const ENTRY_POINT_EMIT_MESSAGE: &str = "emit_message";
const ARG_MESSAGE_SUFFIX_NAME: &str = "message_suffix";

const EMITTER_MESSAGE_PREFIX: &str = "generic message: ";

fn query_message_topic(
    builder: &mut LmdbWasmTestBuilder,
    contract_hash: &ContractHash,
    message_topic_hash: MessageTopicHash,
) -> MessageTopicSummary {
    let query_result = builder
        .query(
            None,
            Key::message_topic(contract_hash.value(), message_topic_hash),
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

#[ignore]
#[test]
fn should_emit_messages() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    // Request to install the contract that will be emitting messages.
    let install_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        MESSAGE_EMITTER_INSTALLER_WASM,
        RuntimeArgs::default(),
    )
    .build();

    // Execute the request to install the message emitting contract.
    // This will also register a topic for the contract to emit messages on.
    builder.exec(install_request).expect_success().commit();

    // Get the contract package for the messages_emitter.
    let query_result = builder
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

    // Check that the topic exists for the installed contract.
    let message_topic_hash = crypto::blake2b(MESSAGE_EMITTER_GENERIC_TOPIC);
    assert_eq!(
        query_message_topic(
            &mut builder,
            message_emitter_contract_hash,
            message_topic_hash
        )
        .message_count(),
        0
    );

    // Now call the entry point to emit some messages.
    let emit_message_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        *message_emitter_contract_hash,
        ENTRY_POINT_EMIT_MESSAGE,
        runtime_args! {
            ARG_MESSAGE_SUFFIX_NAME => "test",
        },
    )
    .build();

    builder.exec(emit_message_request).expect_success().commit();

    let query_result = builder
        .query(
            None,
            Key::message(message_emitter_contract_hash.value(), message_topic_hash, 0),
            &[],
        )
        .expect("should query");

    let queried_message_summary = if let StoredValue::Message(summary) = query_result {
        summary.value()
    } else {
        panic!("Stored value is not a message summary: {:?}", query_result);
    };

    let expected_message =
        MessagePayload::from_string(format!("{}{}", EMITTER_MESSAGE_PREFIX, "test"));
    let expected_message_hash = crypto::blake2b(expected_message.to_bytes().unwrap());

    assert_eq!(expected_message_hash, queried_message_summary);

    assert_eq!(
        query_message_topic(
            &mut builder,
            message_emitter_contract_hash,
            message_topic_hash
        )
        .message_count(),
        1
    )
}
