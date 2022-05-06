use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_PAYMENT, PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::core::{engine_state::Error, execution};
use casper_types::{
    account::{AccountHash, Weight},
    runtime_args, ApiError, RuntimeArgs,
};

const CONTRACT_ADD_ASSOCIATED_KEY: &str = "add_associated_key.wasm";
const CONTRACT_MULTISIG_AUTHORIZATION: &str = "multisig_authorization.wasm";
const CONTRACT_KEY: &str = "contract";

const ARG_AMOUNT: &str = "amount";
const ARG_ACCOUNT: &str = "account";
const ARG_WEIGHT: &str = "weight";
const DEFAULT_WEIGHT: Weight = Weight::new(1);
const ENTRYPOINT_A: &str = "entrypoint_a";
const ENTRYPOINT_B: &str = "entrypoint_b";

const ROLE_A_KEYS: [AccountHash; 3] = [
    AccountHash::new([1; 32]),
    AccountHash::new([2; 32]),
    AccountHash::new([3; 32]),
];

const ROLE_B_KEYS: [AccountHash; 3] = [
    AccountHash::new([4; 32]),
    AccountHash::new([5; 32]),
    AccountHash::new([6; 32]),
];

const USER_ERROR_PERMISSION_DENIED: u16 = 0;

#[ignore]
#[test]
fn should_verify_multisig_authorization_key_roles() {
    // Role A tests
    assert!(
        !test_multisig_auth(
            *DEFAULT_ACCOUNT_ADDR,
            ENTRYPOINT_A,
            &[*DEFAULT_ACCOUNT_ADDR,]
        ),
        "entrypoint A does not work with identity key"
    );
    assert!(
        test_multisig_auth(
            *DEFAULT_ACCOUNT_ADDR,
            ENTRYPOINT_A,
            &[*DEFAULT_ACCOUNT_ADDR, ROLE_A_KEYS[0],]
        ),
        "entrypoint A works with addional role A keys"
    );
    assert!(
        !test_multisig_auth(
            *DEFAULT_ACCOUNT_ADDR,
            ENTRYPOINT_A,
            &[*DEFAULT_ACCOUNT_ADDR, ROLE_B_KEYS[0],]
        ),
        "entrypoint A does not allow role B key"
    );
    assert!(
        !test_multisig_auth(
            *DEFAULT_ACCOUNT_ADDR,
            ENTRYPOINT_A,
            &[
                *DEFAULT_ACCOUNT_ADDR,
                ROLE_B_KEYS[2],
                ROLE_B_KEYS[1],
                ROLE_B_KEYS[0],
            ]
        ),
        "entrypoint A does not allow role B keys"
    );
    assert!(
        test_multisig_auth(
            *DEFAULT_ACCOUNT_ADDR,
            ENTRYPOINT_A,
            &[
                *DEFAULT_ACCOUNT_ADDR,
                ROLE_A_KEYS[2],
                ROLE_A_KEYS[1],
                ROLE_A_KEYS[0],
            ]
        ),
        "entrypoint A works with all role A keys"
    );

    // Role B tests
    assert!(
        !test_multisig_auth(
            *DEFAULT_ACCOUNT_ADDR,
            ENTRYPOINT_B,
            &[*DEFAULT_ACCOUNT_ADDR,]
        ),
        "entrypoint B does not work with identity key"
    );
    assert!(
        test_multisig_auth(
            *DEFAULT_ACCOUNT_ADDR,
            ENTRYPOINT_B,
            &[*DEFAULT_ACCOUNT_ADDR, ROLE_B_KEYS[0],]
        ),
        "entrypoint B works with addional role A keys"
    );
    assert!(
        !test_multisig_auth(
            *DEFAULT_ACCOUNT_ADDR,
            ENTRYPOINT_B,
            &[*DEFAULT_ACCOUNT_ADDR, ROLE_A_KEYS[0],]
        ),
        "entrypoint B does not allow role B key"
    );
    assert!(
        !test_multisig_auth(
            *DEFAULT_ACCOUNT_ADDR,
            ENTRYPOINT_B,
            &[
                *DEFAULT_ACCOUNT_ADDR,
                ROLE_A_KEYS[2],
                ROLE_A_KEYS[1],
                ROLE_A_KEYS[0],
            ]
        ),
        "entrypoint B does not allow role B keys"
    );
    assert!(
        test_multisig_auth(
            *DEFAULT_ACCOUNT_ADDR,
            ENTRYPOINT_B,
            &[
                *DEFAULT_ACCOUNT_ADDR,
                ROLE_B_KEYS[2],
                ROLE_B_KEYS[1],
                ROLE_B_KEYS[0],
            ]
        ),
        "entrypoint B works with all role B keys"
    );
}

fn test_multisig_auth(
    caller: AccountHash,
    entry_point: &str,
    authorization_keys: &[AccountHash],
) -> bool {
    let mut builder = setup();
    let exec_request = {
        let session_args = runtime_args! {};
        let payment_args = runtime_args! {
            ARG_AMOUNT => *DEFAULT_PAYMENT
        };
        let deploy_hash = [42; 32];
        let deploy = DeployItemBuilder::new()
            .with_address(caller)
            .with_stored_session_named_key(CONTRACT_KEY, entry_point, session_args)
            .with_empty_payment_bytes(payment_args)
            .with_authorization_keys(authorization_keys)
            .with_deploy_hash(deploy_hash)
            .build();
        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };
    builder.exec(exec_request).commit();

    match builder.get_error() {
        Some(Error::Exec(execution::Error::Revert(ApiError::User(
            USER_ERROR_PERMISSION_DENIED,
        )))) => false,
        Some(error) => panic!("Unexpected error {:?}", error),
        None => {
            // Success
            true
        }
    }
}

fn setup() -> InMemoryWasmTestBuilder {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    for account in ROLE_A_KEYS.iter().chain(&ROLE_B_KEYS) {
        let add_key_request = {
            let session_args = runtime_args! {
                ARG_ACCOUNT => *account,
                ARG_WEIGHT => DEFAULT_WEIGHT,
            };
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_ADD_ASSOCIATED_KEY,
                session_args,
            )
            .build()
        };

        builder.exec(add_key_request).expect_success().commit();
    }
    let install_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_MULTISIG_AUTHORIZATION,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(install_request).expect_success().commit();

    builder
}
