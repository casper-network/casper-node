use casper_engine_test_support::{
    internal::{
        DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder,
        DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR,
};
use casper_execution_engine::{
    core::{
        engine_state::{Error as CoreError, ExecuteRequest, MAX_PAYMENT},
        execution::Error as ExecError,
    },
    shared::wasm,
};
use casper_types::{runtime_args, Gas, RuntimeArgs, U512};
use num_traits::Zero;

const CONTRACT_DO_NOTHING: &str = "do_nothing.wasm";
const ARG_AMOUNT: &str = "amount";

// /// Creates minimal session code that does nothing
// pub fn do_nothing_with_nop() -> Vec<u8> {
//     let module = builder::module()
//         .function()
//         // A signature with 0 params and no return type
//         .signature()
//         .build()
//         .body()
//         .build()
//         .build()
//         // Export above function
//         .export()
//         .field(DEFAULT_ENTRY_POINT_NAME)
//         .with_instructions(Instructions::new(vec![
//             Instruction::Nop,
//         ]))
//         .build()
//         // Memory section is mandatory
//         .memory()
//         .build()
//         .build();
//     parity_wasm::serialize(module).expect("should serialize")
// }

#[ignore]
#[test]
fn should_charge_minimum_for_do_nothing() {
    let minimum_deploy_payment = U512::from(0);

    let do_nothing_request = {
        let account_hash = *DEFAULT_ACCOUNT_ADDR;
        let session_args = RuntimeArgs::default();
        let deploy_hash = [42; 32];

        let deploy = DeployItemBuilder::new()
            .with_address(account_hash)
            .with_session_code(CONTRACT_DO_NOTHING, session_args)
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => minimum_deploy_payment,
            })
            .with_authorization_keys(&[account_hash])
            .with_deploy_hash(deploy_hash)
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy)
    }
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
    let balance_before = builder.get_purse_balance(account.main_purse());

    builder.exec(do_nothing_request).commit();
    // .expect_success();

    let error = builder.get_error().unwrap();
    assert!(
        matches!(error, CoreError::InsufficientPayment),
        "{:?}",
        error
    );

    let gas = builder.last_exec_gas_cost();
    assert_eq!(gas, Gas::zero());

    let balance_after = builder.get_purse_balance(account.main_purse());
    assert_eq!(balance_before - U512::from(*MAX_PAYMENT), balance_after);
}

// #[ignore]
// #[test]
// fn should_charge_minimum_for_do_nothing_with_nop() {
//     let do_nothing_request = {
//         let account_hash = *DEFAULT_ACCOUNT_ADDR;
//         let session_args = RuntimeArgs::default();
//         let deploy_hash = [42; 32];

//         let deploy = DeployItemBuilder::new()
//             .with_address(account_hash)
//             .with_session_code(CONTRACT_DO_NOTHING, session_args)
//             .with_empty_payment_bytes(runtime_args! {
//                 ARG_AMOUNT => *DEFAULT_PAYMENT
//             })
//             .with_authorization_keys(&[account_hash])
//             .with_deploy_hash(deploy_hash)
//             .build();

//         ExecuteRequestBuilder::new().push_deploy(deploy)
//     }
//     .build();

//     let mut builder = InMemoryWasmTestBuilder::default();

//     builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

//     builder
//         .exec(do_nothing_request)
//         .commit()
//         .expect_success();

//     let gas = builder.last_exec_gas_cost();
//     assert_ne!(gas, Gas::zero(), "{:?} == 0", gas);
// }
