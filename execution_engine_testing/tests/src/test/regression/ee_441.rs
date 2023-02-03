use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, ARG_AMOUNT,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_PAYMENT, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{runtime_args, Key, RuntimeArgs, URef};

const EE_441_RNG_STATE: &str = "ee_441_rng_state.wasm";

fn get_uref(key: Key) -> URef {
    match key {
        Key::URef(uref) => uref,
        _ => panic!("Key {:?} is not an URef", key),
    }
}

fn do_pass(pass: &str) -> (URef, URef) {
    // This test runs a contract that's after every call extends the same key with
    // more data
    let exec_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_session_code(
                EE_441_RNG_STATE,
                runtime_args! {
                    "flag" => pass,
                },
            )
            .with_deploy_hash([1u8; 32])
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy).build()
    };

    let mut builder = InMemoryWasmTestBuilder::default();
    builder
        .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST)
        .exec(exec_request)
        .expect_success()
        .commit();

    let account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have account");

    (
        get_uref(account.named_keys()["uref1"]),
        get_uref(account.named_keys()["uref2"]),
    )
}

#[ignore]
#[test]
fn should_properly_pass_rng_state_to_subcontracts() {
    // the baseline pass, no subcalls
    let (pass1_uref1, pass1_uref2) = do_pass("pass1");
    // second pass does a subcall that does nothing, should be consistent with pass1
    let (pass2_uref1, pass2_uref2) = do_pass("pass2");
    // second pass calls new_uref, and uref2 is returned from a sub call
    let (pass3_uref1, pass3_uref2) = do_pass("pass3");

    // First urefs from each pass should yield same results where pass1 is the
    // baseline
    assert_eq!(pass1_uref1.addr(), pass2_uref1.addr());
    assert_eq!(pass2_uref1.addr(), pass3_uref1.addr());

    // Second urefs from each pass should yield the same result where pass1 is the
    // baseline
    assert_eq!(pass1_uref2.addr(), pass2_uref2.addr());
    assert_eq!(pass2_uref2.addr(), pass3_uref2.addr());
}
