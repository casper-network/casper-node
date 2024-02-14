use casper_execution_engine::engine_state::ExecuteRequest;
use casper_types::{account::AccountHash, runtime_args, Key, URef, U512};

use crate::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_PAYMENT, PRODUCTION_RUN_GENESIS_REQUEST,
};

const CONTRACT_CREATE_ACCOUNTS: &str = "create_accounts.wasm";
const CONTRACT_CREATE_PURSES: &str = "create_purses.wasm";
const CONTRACT_TRANSFER_TO_EXISTING_ACCOUNT: &str = "transfer_to_existing_account.wasm";
const CONTRACT_TRANSFER_TO_PURSE: &str = "transfer_to_purse.wasm";

/// Size of batch used in multiple execs benchmark, and multiple deploys per exec cases.
pub const TRANSFER_BATCH_SIZE: u64 = 3;

/// Test target address.
pub const TARGET_ADDR: AccountHash = AccountHash::new([127; 32]);

const ARG_AMOUNT: &str = "amount";
const ARG_ID: &str = "id";
const ARG_ACCOUNTS: &str = "accounts";
const ARG_SEED_AMOUNT: &str = "seed_amount";
const ARG_TOTAL_PURSES: &str = "total_purses";
const ARG_TARGET: &str = "target";
const ARG_TARGET_PURSE: &str = "target_purse";

/// Test value for number of deploys to generate for a block.
pub const BLOCK_TRANSFER_COUNT: usize = 2500;

/// Converts an integer into an array of type [u8; 32] by converting integer
/// into its big endian representation and embedding it at the end of the
/// range.
fn make_deploy_hash(i: u64) -> [u8; 32] {
    let mut result = [128; 32];
    result[32 - 8..].copy_from_slice(&i.to_be_bytes());
    result
}

/// Create initial accounts and run genesis.
pub fn create_initial_accounts_and_run_genesis(
    builder: &mut LmdbWasmTestBuilder,
    accounts: Vec<AccountHash>,
    amount: U512,
) {
    let exec_request = create_accounts_request(accounts, amount);
    builder
        .run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone())
        .exec(exec_request)
        .expect_success()
        .commit();
}

/// Creates a request that will call the create_accounts.wasm and create test accounts using the
/// default account for the initial transfer.
pub fn create_accounts_request(source_accounts: Vec<AccountHash>, amount: U512) -> ExecuteRequest {
    ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_CREATE_ACCOUNTS,
        runtime_args! { ARG_ACCOUNTS => source_accounts, ARG_SEED_AMOUNT => amount },
    )
    .build()
}

/// Create a number of test purses with an initial balance.
pub fn create_test_purses(
    builder: &mut LmdbWasmTestBuilder,
    source: AccountHash,
    total_purses: u64,
    purse_amount: U512,
) -> Vec<URef> {
    let exec_request = ExecuteRequestBuilder::standard(
        source,
        CONTRACT_CREATE_PURSES,
        runtime_args! {
            ARG_AMOUNT => U512::from(total_purses) * purse_amount,
            ARG_TOTAL_PURSES => total_purses,
            ARG_SEED_AMOUNT => purse_amount
        },
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    // Return creates purses for given account by filtering named keys
    let named_keys = builder.get_named_keys_by_account_hash(source);

    (0..total_purses)
        .map(|index| {
            let purse_lookup_key = format!("purse:{}", index);
            let purse_uref = named_keys
                .get(&purse_lookup_key)
                .and_then(Key::as_uref)
                .unwrap_or_else(|| panic!("should get named key {} as uref", purse_lookup_key));
            *purse_uref
        })
        .collect()
}

/// Uses multiple exec requests with a single deploy to transfer tokens. Executes all transfers in
/// batch determined by value of TRANSFER_BATCH_SIZE.
pub fn transfer_to_account_multiple_execs(
    builder: &mut LmdbWasmTestBuilder,
    account: AccountHash,
    should_commit: bool,
) {
    let amount = U512::one();

    for _ in 0..TRANSFER_BATCH_SIZE {
        let exec_request = ExecuteRequestBuilder::standard(
            *DEFAULT_ACCOUNT_ADDR,
            CONTRACT_TRANSFER_TO_EXISTING_ACCOUNT,
            runtime_args! {
                ARG_TARGET => account,
                ARG_AMOUNT => amount,
            },
        )
        .build();

        let builder = builder.exec(exec_request).expect_success();
        if should_commit {
            builder.commit();
        }
    }
}

/// Executes multiple deploys per single exec with based on TRANSFER_BATCH_SIZE.
pub fn transfer_to_account_multiple_deploys(
    builder: &mut LmdbWasmTestBuilder,
    account: AccountHash,
    should_commit: bool,
) {
    let mut exec_builder = ExecuteRequestBuilder::new();

    for i in 0..TRANSFER_BATCH_SIZE {
        let deploy = DeployItemBuilder::default()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT })
            .with_session_code(
                CONTRACT_TRANSFER_TO_EXISTING_ACCOUNT,
                runtime_args! {
                    ARG_TARGET => account,
                    ARG_AMOUNT => U512::one(),
                },
            )
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
            .with_deploy_hash(make_deploy_hash(i)) // deploy_hash
            .build();
        exec_builder = exec_builder.push_deploy(deploy);
    }

    let exec_request = exec_builder.build();

    let builder = builder.exec(exec_request).expect_success();
    if should_commit {
        builder.commit();
    }
}

/// Uses multiple exec requests with a single deploy to transfer tokens from purse to purse.
/// Executes all transfers in batch determined by value of TRANSFER_BATCH_SIZE.
pub fn transfer_to_purse_multiple_execs(
    builder: &mut LmdbWasmTestBuilder,
    purse: URef,
    should_commit: bool,
) {
    let amount = U512::one();

    for _ in 0..TRANSFER_BATCH_SIZE {
        let exec_request = ExecuteRequestBuilder::standard(
            TARGET_ADDR,
            CONTRACT_TRANSFER_TO_PURSE,
            runtime_args! { ARG_TARGET_PURSE => purse, ARG_AMOUNT => amount },
        )
        .build();

        let builder = builder.exec(exec_request).expect_success();
        if should_commit {
            builder.commit();
        }
    }
}

/// Executes multiple deploys per single exec with based on TRANSFER_BATCH_SIZE.
pub fn transfer_to_purse_multiple_deploys(
    builder: &mut LmdbWasmTestBuilder,
    purse: URef,
    should_commit: bool,
) {
    let mut exec_builder = ExecuteRequestBuilder::new();

    for i in 0..TRANSFER_BATCH_SIZE {
        let deploy = DeployItemBuilder::default()
            .with_address(TARGET_ADDR)
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => *DEFAULT_PAYMENT, })
            .with_session_code(
                CONTRACT_TRANSFER_TO_PURSE,
                runtime_args! { ARG_TARGET_PURSE => purse, ARG_AMOUNT => U512::one() },
            )
            .with_authorization_keys(&[TARGET_ADDR])
            .with_deploy_hash(make_deploy_hash(i)) // deploy_hash
            .build();
        exec_builder = exec_builder.push_deploy(deploy);
    }

    let exec_request = exec_builder.build();

    let builder = builder.exec(exec_request).expect_success();
    if should_commit {
        builder.commit();
    }
}

/// This test simulates flushing at the end of a block.
pub fn transfer_to_account_multiple_native_transfers(
    builder: &mut LmdbWasmTestBuilder,
    execute_requests: &[ExecuteRequest],
    use_scratch: bool,
) {
    for exec_request in execute_requests {
        let request = ExecuteRequest::new(
            exec_request.parent_state_hash,
            exec_request.block_time,
            exec_request.deploys.clone(),
            exec_request.protocol_version,
            exec_request.proposer.clone(),
        );
        if use_scratch {
            builder.scratch_exec_and_commit(request).expect_success();
        } else {
            builder.exec(request).expect_success();
            builder.commit();
        }
    }
    if use_scratch {
        builder.write_scratch_to_db();
    }
    // flush to disk only after entire block (simulates manual_sync_enabled=true config entry)
    builder.flush_environment();

    // WasmTestBuilder holds on to all execution results. This needs to be cleared to reduce
    // overhead in this test - it will likely OOM without.
    builder.clear_results();
}

/// Generate many native transfers from target_account.
pub fn create_multiple_native_transfers_to_purses(
    source_account: AccountHash,
    transfer_count: usize,
    purses: &[URef],
) -> Vec<ExecuteRequest> {
    let mut purse_index = 0usize;
    let mut exec_requests = Vec::with_capacity(transfer_count);
    for _ in 0..transfer_count {
        let account = {
            let account = purses[purse_index];
            if purse_index == purses.len() - 1 {
                purse_index = 0;
            } else {
                purse_index += 1;
            }
            account
        };
        let mut exec_builder = ExecuteRequestBuilder::new();
        let runtime_args = runtime_args! {
            ARG_TARGET => account,
            ARG_AMOUNT => U512::one(),
            ARG_ID => <Option<u64>>::None
        };
        let native_transfer = DeployItemBuilder::new()
            .with_address(source_account)
            .with_empty_payment_bytes(runtime_args! {})
            .with_transfer_args(runtime_args)
            .with_authorization_keys(&[source_account])
            .build();
        exec_builder = exec_builder.push_deploy(native_transfer);
        let exec_request = exec_builder.build();
        exec_requests.push(exec_request);
    }
    exec_requests
}
