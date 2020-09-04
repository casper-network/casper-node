use lazy_static::lazy_static;

use casper_engine_test_support::{
    internal::{
        utils, DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNTS,
        DEFAULT_PAYMENT,
    },
    DEFAULT_ACCOUNT_ADDR,
};
use casper_execution_engine::{core::engine_state::genesis::GenesisAccount, shared::motes::Motes};
use casper_types::{
    account::AccountHash, runtime_args, system_contract_errors::auction, ApiError, PublicKey,
    RuntimeArgs, U512,
};

const ARG_AMOUNT: &str = "amount";
const ARG_PUBLIC_KEY: &str = "public_key";
const ARG_ENTRY_POINT: &str = "entry_point";
const ARG_ACCOUNT_HASH: &str = "account_hash";

const CONTRACT_MINT_BONDING: &str = "mint_bonding.wasm";
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
    let bond_public_key = PublicKey::Ed25519([111; 32]);

    let public_key = PublicKey::Ed25519([42; 32]);
    let account_hash = AccountHash::new([43; 32]);
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account = GenesisAccount::new(
            public_key,
            account_hash,
            Motes::new(GENESIS_VALIDATOR_STAKE.into()) * Motes::new(2.into()),
            Motes::new(GENESIS_VALIDATOR_STAKE.into()),
        );
        tmp.push(account);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_MINT_BONDING,
        runtime_args! {
            ARG_ENTRY_POINT => "seed_new_account",
            ARG_ACCOUNT_HASH => ACCOUNT_1_ADDR,
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
                runtime_args! {
                    ARG_AMOUNT => *ACCOUNT_1_BOND,
                    ARG_PUBLIC_KEY => bond_public_key,
                },
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

    // Error::UnbondTooLarge => 7,
    assert!(
        error_message.contains(&format!(
            "{:?}",
            ApiError::from(auction::Error::UnbondTooLarge)
        )),
        error_message
    );
}
