use rand::Rng;

use casper_execution_engine::core::engine_state::{
    genesis::GenesisValidator, run_genesis_request::RunGenesisRequest, ChainspecRegistry,
    ExecConfig, ExecuteRequest, GenesisAccount, RewardItem,
};
use casper_types::{
    account::AccountHash, runtime_args, system::auction, Motes, ProtocolVersion, PublicKey,
    RuntimeArgs, SecretKey, U512,
};

use crate::{
    DbWasmTestBuilder, DeployItemBuilder, ExecuteRequestBuilder, StepRequestBuilder,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE, DEFAULT_ACCOUNT_PUBLIC_KEY,
    DEFAULT_AUCTION_DELAY, DEFAULT_GENESIS_CONFIG_HASH, DEFAULT_GENESIS_TIMESTAMP_MILLIS,
    DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS, DEFAULT_PROPOSER_PUBLIC_KEY, DEFAULT_PROTOCOL_VERSION,
    DEFAULT_ROUND_SEIGNIORAGE_RATE, DEFAULT_SYSTEM_CONFIG, DEFAULT_UNBONDING_DELAY,
    DEFAULT_WASM_CONFIG, SYSTEM_ADDR,
};

const ARG_AMOUNT: &str = "amount";
const ARG_TARGET: &str = "target";
const ARG_ID: &str = "id";

const DELEGATION_RATE: u8 = 1;
const ID_NONE: Option<u64> = None;

/// Initial balance for delegators in our test.
pub const DELEGATOR_INITIAL_BALANCE: u64 = 500 * 1_000_000_000u64;

const VALIDATOR_BID_AMOUNT: u64 = 100;

/// Amount of time to step foward between runs of the auction in our tests.
pub const TIMESTAMP_INCREMENT_MILLIS: u64 = 30_000;

/// Runs genesis, creates system, validator and delegator accounts, and funds the system account and
/// delegator accounts.
pub fn run_genesis_and_create_initial_accounts(
    builder: &mut DbWasmTestBuilder,
    validator_keys: &[PublicKey],
    delegator_accounts: Vec<AccountHash>,
    delegator_initial_balance: U512,
) {
    let mut genesis_accounts = vec![
        GenesisAccount::account(
            DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
            Motes::new(U512::from(u128::MAX)),
            None,
        ),
        GenesisAccount::account(
            DEFAULT_PROPOSER_PUBLIC_KEY.clone(),
            Motes::new(DEFAULT_ACCOUNT_INITIAL_BALANCE.into()),
            None,
        ),
    ];
    for validator in validator_keys {
        genesis_accounts.push(GenesisAccount::account(
            validator.clone(),
            Motes::new(U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE)),
            Some(GenesisValidator::new(
                Motes::new(U512::from(VALIDATOR_BID_AMOUNT)),
                DELEGATION_RATE,
            )),
        ))
    }
    let run_genesis_request =
        create_run_genesis_request(validator_keys.len() as u32 + 2, genesis_accounts);
    builder.run_genesis(&run_genesis_request);

    // Setup the system account with enough cspr
    let transfer = ExecuteRequestBuilder::transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
                ARG_TARGET => *SYSTEM_ADDR,
                ARG_AMOUNT => U512::from(10_000 * 1_000_000_000u64),
                ARG_ID => ID_NONE,
        },
    )
    .build();
    builder.exec(transfer);
    builder.expect_success().commit();

    for (_i, delegator_account) in delegator_accounts.iter().enumerate() {
        let transfer = ExecuteRequestBuilder::transfer(
            *DEFAULT_ACCOUNT_ADDR,
            runtime_args! {
                    ARG_TARGET => *delegator_account,
                    ARG_AMOUNT => delegator_initial_balance,
                    ARG_ID => ID_NONE,
            },
        )
        .build();
        builder.exec(transfer);
        builder.expect_success().commit();
    }
}

fn create_run_genesis_request(
    validator_slots: u32,
    genesis_accounts: Vec<GenesisAccount>,
) -> RunGenesisRequest {
    let exec_config = {
        ExecConfig::new(
            genesis_accounts,
            *DEFAULT_WASM_CONFIG,
            *DEFAULT_SYSTEM_CONFIG,
            validator_slots,
            DEFAULT_AUCTION_DELAY,
            DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
            DEFAULT_ROUND_SEIGNIORAGE_RATE,
            DEFAULT_UNBONDING_DELAY,
            DEFAULT_GENESIS_TIMESTAMP_MILLIS,
        )
    };
    RunGenesisRequest::new(
        *DEFAULT_GENESIS_CONFIG_HASH,
        *DEFAULT_PROTOCOL_VERSION,
        exec_config,
        ChainspecRegistry::new_with_genesis(&[], &[]),
    )
}

/// Creates a delegation request.
pub fn create_delegate_request(
    delegator_public_key: PublicKey,
    next_validator_key: PublicKey,
    delegation_amount: U512,
    delegator_account_hash: AccountHash,
    contract_hash: casper_types::ContractHash,
) -> ExecuteRequest {
    let entry_point = auction::METHOD_DELEGATE;
    let args = runtime_args! {
        auction::ARG_DELEGATOR => delegator_public_key,
        auction::ARG_VALIDATOR => next_validator_key,
        auction::ARG_AMOUNT => delegation_amount,
    };
    let mut rng = rand::thread_rng();
    let deploy_hash = rng.gen();
    let deploy = DeployItemBuilder::new()
        .with_address(delegator_account_hash)
        .with_stored_session_hash(contract_hash, entry_point, args)
        .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => U512::from(100_000_000), })
        .with_authorization_keys(&[delegator_account_hash])
        .with_deploy_hash(deploy_hash)
        .build();
    ExecuteRequestBuilder::new().push_deploy(deploy).build()
}

/// Generate `key_count` public keys.
pub fn generate_public_keys(key_count: usize) -> Vec<PublicKey> {
    let mut ret = Vec::with_capacity(key_count);
    for _ in 0..key_count {
        let bytes: [u8; SecretKey::ED25519_LENGTH] = rand::random();
        let secret_key = SecretKey::ed25519_from_bytes(&bytes).unwrap();
        let public_key = PublicKey::from(&secret_key);
        ret.push(public_key);
    }
    ret
}

/// Build a step request and run the auction.
pub fn step_and_run_auction(builder: &mut DbWasmTestBuilder, validator_keys: &[PublicKey]) {
    let mut step_request_builder = StepRequestBuilder::new()
        .with_parent_state_hash(builder.get_post_state_hash())
        .with_protocol_version(ProtocolVersion::V1_0_0);
    for validator in validator_keys {
        step_request_builder =
            step_request_builder.with_reward_item(RewardItem::new(validator.clone(), 1));
    }
    let step_request = step_request_builder
        .with_next_era_id(builder.get_era() + 1)
        .build();
    builder.step_with_scratch(step_request);
    builder.write_scratch_to_db();
}
