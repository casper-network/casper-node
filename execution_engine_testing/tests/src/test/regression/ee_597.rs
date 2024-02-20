use once_cell::sync::Lazy;

use casper_engine_test_support::{
    utils, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNTS,
    MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_types::{
    account::AccountHash, system::auction, ApiError, GenesisAccount, Motes, PublicKey, RuntimeArgs,
    SecretKey,
};

const CONTRACT_EE_597_REGRESSION: &str = "ee_597_regression.wasm";

static VALID_PUBLIC_KEY: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static VALID_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*VALID_PUBLIC_KEY));
const VALID_BALANCE: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;

#[ignore]
#[test]
fn should_fail_when_bonding_amount_is_zero_ee_597_regression() {
    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account = GenesisAccount::account(
            VALID_PUBLIC_KEY.clone(),
            Motes::new(VALID_BALANCE.into()),
            None,
        );
        tmp.push(account);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let exec_request = ExecuteRequestBuilder::standard(
        *VALID_ADDR,
        CONTRACT_EE_597_REGRESSION,
        RuntimeArgs::default(),
    )
    .build();

    let mut builder = LmdbWasmTestBuilder::default();
    builder
        .run_genesis(run_genesis_request)
        .exec(exec_request)
        .commit();

    let response = builder
        .get_exec_result_owned(0)
        .expect("should have a response");

    let error_message = utils::get_error_message(response);

    // Error::BondTooSmall => 5,
    assert!(
        error_message.contains(&format!(
            "{:?}",
            ApiError::from(auction::Error::BondTooSmall)
        )),
        "{}",
        error_message
    );
}
