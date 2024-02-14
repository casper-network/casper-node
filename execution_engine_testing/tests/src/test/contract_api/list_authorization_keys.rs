use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_PAYMENT, MINIMUM_ACCOUNT_CREATION_BALANCE, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::{engine_state::Error, execution};
use casper_types::{
    account::AccountHash,
    addressable_entity::Weight,
    runtime_args,
    system::{mint, standard_payment::ARG_AMOUNT},
    ApiError, PublicKey, SecretKey, U512,
};
use once_cell::sync::Lazy;

const ARG_ACCOUNT: &str = "account";
const ARG_WEIGHT: &str = "weight";
const DEFAULT_WEIGHT: Weight = Weight::new(1);

const CONTRACT_ADD_ASSOCIATED_KEY: &str = "add_associated_key.wasm";

const CONTRACT_LIST_AUTHORIZATION_KEYS: &str = "list_authorization_keys.wasm";
const ARG_EXPECTED_AUTHORIZATION_KEYS: &str = "expected_authorization_keys";

static ACCOUNT_1_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::secp256k1_from_bytes([234u8; 32]).unwrap());
static ACCOUNT_1_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*ACCOUNT_1_SECRET_KEY));
static ACCOUNT_1_ADDR: Lazy<AccountHash> = Lazy::new(|| ACCOUNT_1_PUBLIC_KEY.to_account_hash());

static ACCOUNT_2_SECRET_KEY: Lazy<SecretKey> =
    Lazy::new(|| SecretKey::secp256k1_from_bytes([243u8; 32]).unwrap());
static ACCOUNT_2_PUBLIC_KEY: Lazy<PublicKey> =
    Lazy::new(|| PublicKey::from(&*ACCOUNT_2_SECRET_KEY));
static ACCOUNT_2_ADDR: Lazy<AccountHash> = Lazy::new(|| ACCOUNT_2_PUBLIC_KEY.to_account_hash());

const USER_ERROR_ASSERTION: u16 = 0;

#[ignore]
#[test]
fn should_list_authorization_keys() {
    assert!(
        test_match(
            *DEFAULT_ACCOUNT_ADDR,
            vec![*DEFAULT_ACCOUNT_ADDR],
            vec![*DEFAULT_ACCOUNT_ADDR]
        ),
        "one signature should match the expected authorization key"
    );
    assert!(
        !test_match(
            *DEFAULT_ACCOUNT_ADDR,
            vec![*ACCOUNT_2_ADDR, *DEFAULT_ACCOUNT_ADDR],
            vec![*DEFAULT_ACCOUNT_ADDR, *ACCOUNT_1_ADDR]
        ),
        "two signatures are off by one"
    );
    assert!(
        test_match(
            *DEFAULT_ACCOUNT_ADDR,
            vec![*ACCOUNT_2_ADDR, *DEFAULT_ACCOUNT_ADDR],
            vec![*DEFAULT_ACCOUNT_ADDR, *ACCOUNT_2_ADDR]
        ),
        "two signatures should match the expected list"
    );
    assert!(
        test_match(
            *ACCOUNT_1_ADDR,
            vec![*ACCOUNT_1_ADDR],
            vec![*ACCOUNT_1_ADDR]
        ),
        "one signature should match the output for non-default account"
    );

    assert!(
        test_match(
            *DEFAULT_ACCOUNT_ADDR,
            vec![*ACCOUNT_2_ADDR, *DEFAULT_ACCOUNT_ADDR, *ACCOUNT_1_ADDR],
            vec![*ACCOUNT_1_ADDR, *ACCOUNT_2_ADDR, *DEFAULT_ACCOUNT_ADDR]
        ),
        "multisig matches expected list"
    );
    assert!(
        !test_match(
            *DEFAULT_ACCOUNT_ADDR,
            vec![*ACCOUNT_2_ADDR, *DEFAULT_ACCOUNT_ADDR, *ACCOUNT_1_ADDR],
            vec![]
        ),
        "multisig is not empty"
    );
    assert!(
        !test_match(
            *DEFAULT_ACCOUNT_ADDR,
            vec![*ACCOUNT_2_ADDR, *DEFAULT_ACCOUNT_ADDR, *ACCOUNT_1_ADDR],
            vec![*ACCOUNT_2_ADDR, *ACCOUNT_1_ADDR]
        ),
        "multisig does not include caller account"
    );
}

fn test_match(
    caller: AccountHash,
    signatures: Vec<AccountHash>,
    expected_authorization_keys: Vec<AccountHash>,
) -> bool {
    let mut builder = setup();
    let exec_request = {
        let session_args = runtime_args! {
            ARG_EXPECTED_AUTHORIZATION_KEYS => expected_authorization_keys
        };
        let deploy_hash = [42; 32];

        let deploy = DeployItemBuilder::new()
            .with_address(caller)
            .with_session_code(CONTRACT_LIST_AUTHORIZATION_KEYS, session_args)
            .with_empty_payment_bytes(runtime_args! {
                ARG_AMOUNT => *DEFAULT_PAYMENT
            })
            .with_authorization_keys(&signatures)
            .with_deploy_hash(deploy_hash)
            .build();
        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };
    builder.exec(exec_request).commit();

    match builder.get_error() {
        Some(Error::Exec(execution::Error::Revert(ApiError::User(USER_ERROR_ASSERTION)))) => false,
        Some(error) => panic!("Unexpected error {:?}", error),
        None => {
            // Success
            true
        }
    }
}

fn setup() -> LmdbWasmTestBuilder {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    for account in [*ACCOUNT_1_ADDR, *ACCOUNT_2_ADDR] {
        let add_key_request = {
            let session_args = runtime_args! {
                ARG_ACCOUNT => account,
                ARG_WEIGHT => DEFAULT_WEIGHT,
            };
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_ADD_ASSOCIATED_KEY,
                session_args,
            )
            .build()
        };

        let transfer_request = {
            let transfer_args = runtime_args! {
                mint::ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE),
                mint::ARG_TARGET => account,
                mint::ARG_ID => Option::<u64>::None,
            };

            ExecuteRequestBuilder::transfer(*DEFAULT_ACCOUNT_ADDR, transfer_args).build()
        };

        builder.exec(add_key_request).expect_success().commit();
        builder.exec(transfer_request).expect_success().commit();
    }

    builder
}
