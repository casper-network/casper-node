use casper_engine_test_support::{
    utils, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_ACCOUNT_SECRET_KEY, LOCAL_GENESIS_REQUEST,
};
use casper_execution_engine::{
    engine_state::{Error as StateError, SessionDataDeploy, SessionDataV1, SessionInputData},
    execution::ExecError,
};
use casper_types::{
    ApiError, BlockTime, InitiatorAddr, Phase, PricingMode, RuntimeArgs, Transaction,
    TransactionEntryPoint, TransactionTarget, TransactionV1Builder,
};

const CONTRACT: &str = "do_nothing_stored.wasm";
const CHAIN_NAME: &str = "a";
const BLOCK_TIME: BlockTime = BlockTime::new(10);

pub(crate) const ARGS_MAP_KEY: u16 = 0;
pub(crate) const TARGET_MAP_KEY: u16 = 1;
pub(crate) const ENTRY_POINT_MAP_KEY: u16 = 2;

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

fn try_add_contract_version(is_install_upgrade: bool, should_succeed: bool) {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone()).commit();

    let module_bytes = utils::read_wasm_file(CONTRACT);

    let txn = Transaction::from(
        TransactionV1Builder::new_session(is_install_upgrade, module_bytes)
            .with_secret_key(&DEFAULT_ACCOUNT_SECRET_KEY)
            .with_chain_name(CHAIN_NAME)
            .build()
            .unwrap(),
    );
    let txn_request = match txn {
        Transaction::Deploy(ref deploy) => {
            let initiator_addr = txn.initiator_addr();
            let is_standard_payment = deploy.payment().is_standard_payment(Phase::Payment);
            let session_input_data =
                to_deploy_session_input_data(is_standard_payment, initiator_addr, &txn);
            ExecuteRequestBuilder::from_session_input_data(&session_input_data)
                .with_block_time(BLOCK_TIME)
                .build()
        }
        Transaction::V1(ref v1) => {
            let initiator_addr = txn.initiator_addr();
            let is_standard_payment = if let PricingMode::Classic {
                standard_payment, ..
            } = v1.pricing_mode()
            {
                *standard_payment
            } else {
                true
            };
            let args = v1.deserialize_field::<RuntimeArgs>(ARGS_MAP_KEY).unwrap();
            let target = v1
                .deserialize_field::<TransactionTarget>(TARGET_MAP_KEY)
                .unwrap();
            let entry_point = v1
                .deserialize_field::<TransactionEntryPoint>(ENTRY_POINT_MAP_KEY)
                .unwrap();
            let session_input_data = to_v1_session_input_data(
                is_standard_payment,
                initiator_addr,
                &args,
                &target,
                &entry_point,
                &txn,
            );
            ExecuteRequestBuilder::from_session_input_data(&session_input_data)
                .with_block_time(BLOCK_TIME)
                .build()
        }
    };
    builder.exec(txn_request);

    if should_succeed {
        builder.expect_success();
    } else {
        builder.assert_error(StateError::Exec(ExecError::Revert(
            ApiError::NotAllowedToAddContractVersion,
        )))
    }
}

fn to_deploy_session_input_data(
    is_standard_payment: bool,
    initiator_addr: InitiatorAddr,
    txn: &Transaction,
) -> SessionInputData<'_> {
    match txn {
        Transaction::Deploy(deploy) => {
            let data = SessionDataDeploy::new(
                deploy.hash(),
                deploy.session(),
                initiator_addr,
                txn.signers().clone(),
                is_standard_payment,
            );
            SessionInputData::DeploySessionData { data }
        }
        Transaction::V1(_) => {
            panic!("unexpected transaction v1");
        }
    }
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
    try_add_contract_version(true, true)
}

// #[ignore]
// #[test]
// fn should_disallow_add_contract_version_via_transaction_v1_standard() {
//     try_add_contract_version(false, false)
// }
