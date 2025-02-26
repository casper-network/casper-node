use std::collections::{BTreeMap, BTreeSet};

use crate::lmdb_fixture;
use casper_engine_test_support::{
    utils, ExecuteRequestBuilder, LmdbWasmTestBuilder, UpgradeRequestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_ACCOUNT_SECRET_KEY, LOCAL_GENESIS_REQUEST,
};
use casper_execution_engine::{
    engine_state::{Error as StateError, SessionDataV1, SessionInputData},
    execution::ExecError,
};
use casper_types::{
    bytesrepr::{Bytes, ToBytes},
    ApiError, BlockTime, Digest, EraId, InitiatorAddr, Key, PricingMode, ProtocolVersion,
    PublicKey, RuntimeArgs, SecretKey, TimeDiff, Timestamp, Transaction, TransactionArgs,
    TransactionEntryPoint, TransactionRuntimeParams, TransactionScheduling, TransactionTarget,
    TransactionV1, TransactionV1Payload,
};

const CONTRACT: &str = "do_nothing_stored.wasm";
const CHAIN_NAME: &str = "a";
const BLOCK_TIME: BlockTime = BlockTime::new(10);

pub(crate) const ARGS_MAP_KEY: u16 = 0;
pub(crate) const TARGET_MAP_KEY: u16 = 1;
pub(crate) const ENTRY_POINT_MAP_KEY: u16 = 2;
pub(crate) const SCHEDULING_MAP_KEY: u16 = 3;

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

fn try_add_contract_version(
    is_install_upgrade: bool,
    should_succeed: bool,
    mut builder: LmdbWasmTestBuilder,
) {
    let module_bytes = utils::read_wasm_file(CONTRACT);

    let txn = new_transaction_v1_session(
        is_install_upgrade,
        module_bytes,
        TransactionRuntimeParams::VmCasperV1,
        &DEFAULT_ACCOUNT_SECRET_KEY,
    );

    let txn_request = {
        let initiator_addr = txn.initiator_addr().clone();
        let is_standard_payment = if let PricingMode::PaymentLimited {
            standard_payment, ..
        } = txn.pricing_mode()
        {
            *standard_payment
        } else {
            true
        };
        let tx_args = txn
            .deserialize_field::<TransactionArgs>(ARGS_MAP_KEY)
            .unwrap();
        let args = tx_args.as_named().unwrap();
        let target = txn
            .deserialize_field::<TransactionTarget>(TARGET_MAP_KEY)
            .unwrap();
        let entry_point = txn
            .deserialize_field::<TransactionEntryPoint>(ENTRY_POINT_MAP_KEY)
            .unwrap();
        let wrapped = Transaction::from(txn);
        let session_input_data = to_v1_session_input_data(
            is_standard_payment,
            initiator_addr,
            args,
            &target,
            &entry_point,
            &wrapped,
        );
        assert_eq!(
            session_input_data.is_install_upgrade_allowed(),
            is_install_upgrade,
            "session_input_data should match imputed arg"
        );
        ExecuteRequestBuilder::from_session_input_data(&session_input_data)
            .with_block_time(BLOCK_TIME)
            .build()
    };
    assert_eq!(
        txn_request.is_install_upgrade_allowed(),
        is_install_upgrade,
        "txn_request should match imputed arg"
    );
    builder.exec(txn_request);

    if should_succeed {
        builder.expect_success();
    } else {
        builder.assert_error(StateError::Exec(ExecError::Revert(
            ApiError::NotAllowedToAddContractVersion,
        )))
    }
}

pub fn new_transaction_v1_session(
    is_install_upgrade: bool,
    module_bytes: Bytes,
    runtime: TransactionRuntimeParams,
    secret_key: &SecretKey,
) -> TransactionV1 {
    let timestamp = Timestamp::now();

    let target = TransactionTarget::Session {
        is_install_upgrade,
        module_bytes,
        runtime,
    };
    let args = TransactionArgs::Named(RuntimeArgs::new());
    let entry_point = TransactionEntryPoint::Call;
    let scheduling = TransactionScheduling::Standard;
    let mut fields: BTreeMap<u16, Bytes> = BTreeMap::new();

    fields.insert(ARGS_MAP_KEY, args.to_bytes().unwrap().into());
    fields.insert(TARGET_MAP_KEY, target.to_bytes().unwrap().into());
    fields.insert(ENTRY_POINT_MAP_KEY, entry_point.to_bytes().unwrap().into());
    fields.insert(SCHEDULING_MAP_KEY, scheduling.to_bytes().unwrap().into());

    let public_key = PublicKey::from(secret_key);
    let initiator_addr = InitiatorAddr::from(public_key);
    build_transaction(
        CHAIN_NAME.to_string(),
        timestamp,
        TimeDiff::from_millis(30 * 60 * 1_000),
        PricingMode::Fixed {
            gas_price_tolerance: 5,
            additional_computation_factor: 0,
        },
        fields,
        initiator_addr,
        secret_key,
    )
}

fn build_transaction(
    chain_name: String,
    timestamp: Timestamp,
    ttl: TimeDiff,
    pricing_mode: PricingMode,
    fields: BTreeMap<u16, Bytes>,
    initiator_addr: InitiatorAddr,
    secret_key: &SecretKey,
) -> TransactionV1 {
    let transaction_v1_payload = TransactionV1Payload::new(
        chain_name,
        timestamp,
        ttl,
        pricing_mode,
        initiator_addr,
        fields,
    );
    let hash = Digest::hash(
        transaction_v1_payload
            .to_bytes()
            .unwrap_or_else(|error| panic!("should serialize body: {}", error)),
    );
    let mut transaction = TransactionV1::new(hash.into(), transaction_v1_payload, BTreeSet::new());
    transaction.sign(secret_key);
    transaction
}

fn to_v1_session_input_data<'a>(
    is_standard_payment: bool,
    initiator_addr: InitiatorAddr,
    args: &'a RuntimeArgs,
    target: &'a TransactionTarget,
    entry_point: &'a TransactionEntryPoint,
    txn: &'a Transaction,
) -> SessionInputData<'a> {
    let is_install_upgrade = match target {
        TransactionTarget::Session {
            is_install_upgrade, ..
        } => *is_install_upgrade,
        _ => false,
    };
    match txn {
        Transaction::Deploy(_) => panic!("unexpected deploy transaction"),
        Transaction::V1(transaction_v1) => {
            let data = SessionDataV1::new(
                args,
                target,
                entry_point,
                is_install_upgrade,
                transaction_v1.hash(),
                transaction_v1.pricing_mode(),
                initiator_addr,
                txn.signers().clone(),
                is_standard_payment,
            );
            SessionInputData::SessionDataV1 { data }
        }
    }
}

#[ignore]
#[test]
fn should_allow_add_contract_version_via_transaction_v1_installer_upgrader() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone()).commit();
    try_add_contract_version(true, true, builder)
}

#[ignore]
#[test]
fn should_disallow_add_contract_version_via_transaction_v1_standard() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone()).commit();
    try_add_contract_version(false, false, builder)
}

#[ignore]
#[test]
fn should_allow_1x_user_to_add_contract_version_via_transaction_v1_installer_upgrader() {
    let (mut builder, lmdb_fixture_state, _temp_dir) =
        lmdb_fixture::builder_from_global_state_fixture_with_enable_ae(
            lmdb_fixture::RELEASE_1_5_8,
            true,
        );
    let old_protocol_version = lmdb_fixture_state.genesis_protocol_version();

    let mut upgrade_request = UpgradeRequestBuilder::new()
        .with_current_protocol_version(old_protocol_version)
        .with_new_protocol_version(ProtocolVersion::from_parts(2, 0, 0))
        .with_activation_point(EraId::new(1))
        .with_enable_addressable_entity(true)
        .build();

    builder
        .upgrade(&mut upgrade_request)
        .expect_upgrade_success();

    let account_as_1x = builder
        .query(None, Key::Account(*DEFAULT_ACCOUNT_ADDR), &[])
        .expect("must have stored value")
        .as_account()
        .is_some();

    assert!(account_as_1x);
    try_add_contract_version(true, true, builder)
}
