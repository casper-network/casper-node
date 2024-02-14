use num_traits::Zero;
use once_cell::sync::Lazy;

use casper_engine_test_support::{
    utils, DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNTS,
    DEFAULT_ACCOUNT_ADDR,
};
use casper_types::{
    account::AccountHash, runtime_args, system::auction::DelegationRate, GenesisAccount,
    GenesisValidator, Motes, PublicKey, SecretKey, U512,
};

const ARG_AMOUNT: &str = "amount";
const ARG_PUBLIC_KEY: &str = "public_key";
const ARG_ENTRY_POINT: &str = "entry_point";
const ARG_ACCOUNT_HASH: &str = "account_hash";

const CONTRACT_AUCTION_BIDDING: &str = "auction_bidding.wasm";

static ACCOUNT_1_PK: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([4; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});

const GENESIS_VALIDATOR_STAKE: u64 = 50_000;

static ACCOUNT_1_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*ACCOUNT_1_PK));
static ACCOUNT_1_FUND: Lazy<U512> = Lazy::new(|| U512::from(1_500_000_000_000u64));
static ACCOUNT_1_BALANCE: Lazy<U512> = Lazy::new(|| *ACCOUNT_1_FUND + 100_000);
static ACCOUNT_1_BOND: Lazy<U512> = Lazy::new(|| U512::from(25_000));

#[ignore]
#[test]
fn should_handle_unbond_for_more_than_stake_as_full_unbond_of_stake_ee_598_regression() {
    let secret_key = SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH]).unwrap();
    let public_key = PublicKey::from(&secret_key);
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account = GenesisAccount::account(
            public_key,
            Motes::new(GENESIS_VALIDATOR_STAKE.into()) * Motes::new(2.into()),
            Some(GenesisValidator::new(
                Motes::new(GENESIS_VALIDATOR_STAKE.into()),
                DelegationRate::zero(),
            )),
        );
        tmp.push(account);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let seed_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_AUCTION_BIDDING,
        runtime_args! {
            ARG_ENTRY_POINT => "seed_new_account",
            ARG_ACCOUNT_HASH => *ACCOUNT_1_ADDR,
            ARG_AMOUNT => *ACCOUNT_1_BALANCE,
        },
    )
    .build();
    let combined_bond_and_unbond_request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*ACCOUNT_1_ADDR)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *ACCOUNT_1_FUND })
            .with_session_code(
                "ee_598_regression.wasm",
                runtime_args! {
                    ARG_AMOUNT => *ACCOUNT_1_BOND,
                    ARG_PUBLIC_KEY => ACCOUNT_1_PK.clone(),
                },
            )
            .with_deploy_hash([2u8; 32])
            .with_authorization_keys(&[*ACCOUNT_1_ADDR])
            .build();
        ExecuteRequestBuilder::from_deploy_item(deploy).build()
    };

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(run_genesis_request);

    builder.exec(seed_request).expect_success().commit();

    builder
        .exec(combined_bond_and_unbond_request)
        .expect_success()
        .commit();
}
