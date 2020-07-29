use lazy_static::lazy_static;

use casperlabs_engine_test_support::{
    internal::{
        utils, DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNTS,
        DEFAULT_PAYMENT,
    },
    DEFAULT_ACCOUNT_ADDR,
};
use casperlabs_node::{types::Motes, GenesisAccount};
use casperlabs_types::{account::AccountHash, runtime_args, ApiError, RuntimeArgs, U512};

const ARG_AMOUNT: &str = "amount";
const ARG_ENTRY_POINT: &str = "entry_point";
const ARG_ACCOUNT_PK: &str = "account_hash";

const CONTRACT_POS_BONDING: &str = "pos_bonding.wasm";
const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([7u8; 32]);

const GENESIS_VALIDATOR_STAKE: u64 = 50_000;
lazy_static! {
    static ref ACCOUNT_1_FUND: U512 = *DEFAULT_PAYMENT;
    static ref ACCOUNT_1_BALANCE: U512 = *ACCOUNT_1_FUND + 100_000;
    static ref ACCOUNT_1_BOND: U512 = 25_000.into();
}

#[ignore]
#[test]
fn should_fail_unbonding_more_than_it_was_staked_ee_598_regression() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account = GenesisAccount::new(
            AccountHash::new([42; 32]),
            Motes::new(GENESIS_VALIDATOR_STAKE.into()) * Motes::new(2.into()),
            Motes::new(GENESIS_VALIDATOR_STAKE.into()),
        );
        tmp.push(account);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let exec_request_1 = ExecuteRequestBuilder::standard(
        DEFAULT_ACCOUNT_ADDR,
        CONTRACT_POS_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => "seed_new_account",
            ARG_ACCOUNT_PK => ACCOUNT_1_ADDR,
            ARG_AMOUNT => *ACCOUNT_1_BALANCE,
        },
    )
    .build();
    let exec_request_2 = {
        let deploy = DeployItemBuilder::new()
            .with_address(ACCOUNT_1_ADDR)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *ACCOUNT_1_FUND })
            .with_session_code(
                "ee_598_regression.wasm",
                runtime_args! { ARG_AMOUNT => *ACCOUNT_1_BOND },
            )
            .with_deploy_hash([2u8; 32])
            .with_authorization_keys(&[ACCOUNT_1_ADDR])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy).build()
    };

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&run_genesis_request);

    builder.exec(exec_request_1).expect_success().commit();

    let result = builder.exec(exec_request_2).commit().finish();

    let response = result
        .builder()
        .get_exec_response(1)
        .expect("should have a response")
        .to_owned();
    let error_message = utils::get_error_message(response);

    if !cfg!(feature = "enable-bonding") {
        assert!(
            error_message.contains(&format!("{:?}", ApiError::Unhandled)),
            error_message
        );
    } else {
        // Error::UnbondTooLarge => 7,
        assert!(
            error_message.contains(&format!("{:?}", ApiError::ProofOfStake(7))),
            error_message
        );
    }
}
