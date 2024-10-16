use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, DEFAULT_PAYMENT,
    LOCAL_GENESIS_REQUEST,
};
use casper_types::{
    account::AccountHash,
    contracts::{ContractHash, ContractPackageHash},
    runtime_args,
    system::{Caller, CallerInfo},
    CLValue, EntityAddr,
};

const CONTRACT_GET_CALLER: &str = "get_caller.wasm";
const CONTRACT_GET_CALLER_SUBCALL: &str = "get_caller_subcall.wasm";
const CONTRACT_TRANSFER_PURSE_TO_ACCOUNT: &str = "transfer_purse_to_account.wasm";
const LOAD_CALLER_INFORMATION: &str = "load_caller_info.wasm";
const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([1u8; 32]);
const LOAD_CALLER_INFO_HASH: &str = "load_caller_info_contract_hash";
const LOAD_CALLER_INFO_PACKAGE_HASH: &str = "load_caller_info_package";

#[ignore]
#[test]
fn should_run_get_caller_contract() {
    LmdbWasmTestBuilder::default()
        .run_genesis(LOCAL_GENESIS_REQUEST.clone())
        .exec(
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_GET_CALLER,
                runtime_args! {"account" => *DEFAULT_ACCOUNT_ADDR},
            )
            .build(),
        )
        .expect_success()
        .commit();
}

#[ignore]
#[test]
fn should_run_get_caller_contract_other_account() {
    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    builder
        .exec(
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_TRANSFER_PURSE_TO_ACCOUNT,
                runtime_args! {"target" => ACCOUNT_1_ADDR, "amount"=> *DEFAULT_PAYMENT},
            )
            .build(),
        )
        .expect_success()
        .commit();

    builder
        .exec(
            ExecuteRequestBuilder::standard(
                ACCOUNT_1_ADDR,
                CONTRACT_GET_CALLER,
                runtime_args! {"account" => ACCOUNT_1_ADDR},
            )
            .build(),
        )
        .expect_success()
        .commit();
}

#[ignore]
#[test]
fn should_run_get_caller_subcall_contract() {
    {
        let mut builder = LmdbWasmTestBuilder::default();
        builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

        builder
            .exec(
                ExecuteRequestBuilder::standard(
                    *DEFAULT_ACCOUNT_ADDR,
                    CONTRACT_GET_CALLER_SUBCALL,
                    runtime_args! {"account" => *DEFAULT_ACCOUNT_ADDR},
                )
                .build(),
            )
            .expect_success()
            .commit();
    }

    let mut builder = LmdbWasmTestBuilder::default();
    builder
        .run_genesis(LOCAL_GENESIS_REQUEST.clone())
        .exec(
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_TRANSFER_PURSE_TO_ACCOUNT,
                runtime_args! {"target" => ACCOUNT_1_ADDR, "amount"=>*DEFAULT_PAYMENT},
            )
            .build(),
        )
        .expect_success()
        .commit();
    builder
        .exec(
            ExecuteRequestBuilder::standard(
                ACCOUNT_1_ADDR,
                CONTRACT_GET_CALLER_SUBCALL,
                runtime_args! {"account" => ACCOUNT_1_ADDR},
            )
            .build(),
        )
        .expect_success()
        .commit();
}

#[ignore]
#[test]
fn should_load_caller_information_based_on_action() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder
        .run_genesis(LOCAL_GENESIS_REQUEST.clone())
        .exec(
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                LOAD_CALLER_INFORMATION,
                runtime_args! {},
            )
            .build(),
        )
        .expect_success()
        .commit();

    let caller_info_entity_hash = builder
        .get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .get(LOAD_CALLER_INFO_HASH)
        .expect("must have caller info entity key")
        .into_entity_hash()
        .expect("must get addressable entity hash");

    let initiator_call_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        caller_info_entity_hash,
        "initiator",
        runtime_args! {},
    )
    .build();

    builder
        .exec(initiator_call_request)
        .expect_success()
        .commit();

    let immediate_call_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        caller_info_entity_hash,
        "get_immediate_caller",
        runtime_args! {},
    )
    .build();

    builder
        .exec(immediate_call_request)
        .expect_success()
        .commit();

    let initiator_call_request = ExecuteRequestBuilder::contract_call_by_hash(
        *DEFAULT_ACCOUNT_ADDR,
        caller_info_entity_hash,
        "get_full_stack",
        runtime_args! {},
    )
    .build();

    builder
        .exec(initiator_call_request)
        .expect_success()
        .commit();

    let info_named_keys =
        builder.get_named_keys(EntityAddr::SmartContract(caller_info_entity_hash.value()));

    let initiator = *info_named_keys
        .get("initiator")
        .expect("must have key entry for initiator");

    let initiator_account_hash = builder
        .query(None, initiator, &[])
        .expect("must have stored value")
        .as_cl_value()
        .map(|cl_val| CLValue::into_t(cl_val.clone()))
        .expect("must have cl value")
        .expect("must get account hash");

    assert_eq!(*DEFAULT_ACCOUNT_ADDR, initiator_account_hash);

    let immediate = *info_named_keys
        .get("immediate")
        .expect("must have key entry for initiator");

    let caller: CallerInfo = builder
        .query(None, immediate, &[])
        .expect("must have stored value")
        .as_cl_value()
        .map(|cl_val| CLValue::into_t(cl_val.clone()))
        .expect("must have cl value")
        .expect("must get caller");

    let expected_caller = CallerInfo::try_from(Caller::initiator(*DEFAULT_ACCOUNT_ADDR))
        .expect("must get caller info");

    assert_eq!(expected_caller, caller);

    let full = *info_named_keys
        .get("full")
        .expect("must have key entry for full call stack");

    let full_call_stack: Vec<CallerInfo> = builder
        .query(None, full, &[])
        .expect("must have stored value")
        .as_cl_value()
        .map(|cl_val| CLValue::into_t(cl_val.clone()))
        .expect("must have cl value")
        .expect("must get full call stack");

    let package_hash = builder
        .get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .get(LOAD_CALLER_INFO_PACKAGE_HASH)
        .expect("must get package key")
        .into_hash_addr()
        .map(ContractPackageHash::new)
        .expect("must get package hash");

    let frame = CallerInfo::try_from(Caller::smart_contract(
        package_hash,
        ContractHash::new(caller_info_entity_hash.value()),
    ))
    .expect("must get frame");
    let expected_stack = vec![expected_caller, frame];
    assert_eq!(expected_stack, full_call_stack);
}
