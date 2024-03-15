use casper_engine_test_support::{
    utils, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_ACCOUNT_SECRET_KEY, DEFAULT_PROPOSER_PUBLIC_KEY, LOCAL_GENESIS_REQUEST,
};
use casper_execution_engine::{
    engine_state::{Error as StateError, ExecuteRequest},
    execution::ExecError,
};
use casper_types::{
    ApiError, BlockTime, Digest, RuntimeArgs, Transaction, TransactionSessionKind,
    TransactionV1Builder,
};

const CONTRACT: &str = "do_nothing_stored.wasm";
const ENTRY_POINT: &str = "call";
const CHAIN_NAME: &str = "a";
const STATE_HASH: Digest = Digest::from_raw([1; 32]);
const BLOCK_TIME: BlockTime = BlockTime::new(10);

#[ignore]
#[test]
fn should_allow_add_contract_version_via_deploy() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone()).commit();

    let deploy_request =
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, CONTRACT, RuntimeArgs::new())
            .build();

    builder.exec(deploy_request).expect_success().commit();
}

fn try_add_contract_version(kind: TransactionSessionKind, should_succeed: bool) {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone()).commit();

    let module_bytes = utils::read_wasm_file(CONTRACT);

    let txn = TransactionV1Builder::new_session(kind, module_bytes, ENTRY_POINT)
        .with_secret_key(&DEFAULT_ACCOUNT_SECRET_KEY)
        .with_chain_name(CHAIN_NAME)
        .build()
        .unwrap();

    let txn_request = ExecuteRequest::new(
        STATE_HASH,
        BLOCK_TIME,
        Transaction::from(txn),
        DEFAULT_PROPOSER_PUBLIC_KEY.clone(),
    )
    .unwrap();

    builder.exec(txn_request);

    if should_succeed {
        builder.expect_success();
    } else {
        builder.assert_error(StateError::Exec(ExecError::Revert(
            ApiError::NotAllowedToAddContractVersion,
        )))
    }
}

#[ignore]
#[test]
fn should_allow_add_contract_version_via_transaction_v1_installer() {
    try_add_contract_version(TransactionSessionKind::Installer, true)
}

#[ignore]
#[test]
fn should_allow_add_contract_version_via_transaction_v1_upgrader() {
    try_add_contract_version(TransactionSessionKind::Upgrader, true)
}

#[ignore]
#[test]
fn should_disallow_add_contract_version_via_transaction_v1_standard() {
    try_add_contract_version(TransactionSessionKind::Standard, false)
}

#[ignore]
#[test]
fn should_disallow_add_contract_version_via_transaction_v1_isolated() {
    try_add_contract_version(TransactionSessionKind::Isolated, false)
}
