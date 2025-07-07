use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, LOCAL_GENESIS_REQUEST,
};
use casper_types::{RuntimeArgs, Key};

const CONTRACT_VM1_RET_TEST: &str = "ret_journal_test.wasm";
const EXPECTED_RET_BYTES: &[u8] = b"casper_ret test data";

#[ignore]
#[test]
fn vm1_casper_ret_emits_ret_transforms() {
    // This test verifies that casper_ret host function emits Ret transforms
    // to the execution journal and that the returned data is correct
    
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    // Execute the contract that calls casper_ret directly
    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_VM1_RET_TEST,
        RuntimeArgs::default(),
    )
    .build();

    // Execute but do not commit yet
    let results = builder.exec(exec_request).expect_success().get_exec_result_owned(0).unwrap();
    
    // Check that Ret transforms are present in the effects
    let effects = results.effects();
    let transforms = effects.transforms();
    
    let ret_transform = transforms.iter().find_map(|transform| {
        if let casper_types::execution::TransformKindV2::Ret(bytes) = transform.kind() {
            Some((transform.key(), bytes))
        } else {
            None
        }
    });
    
    let (key, bytes) = ret_transform.expect("Expected to find a Ret transform in the effects");
    assert_eq!(
        bytes.as_slice(),
        EXPECTED_RET_BYTES,
        "Return data should match what was passed to casper_ret"
    );
    // Check the key is a contract key
    if let Key::SmartContract(_) | Key::AddressableEntity(_) = key {
        // ok
    } else {
        panic!("Ret transform should be under a contract key, got: {:?}", key);
    }
    
    // Now commit
    builder.commit();
} 