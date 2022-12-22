use std::{collections::HashSet, convert::TryFrom, io::Write, time::Instant};

use lmdb::{Cursor, Transaction};
use tempfile::TempDir;

use casper_execution_engine::{
    core::{
        engine_state::{
            self, genesis::GenesisValidator, run_genesis_request::RunGenesisRequest,
            ChainspecRegistry, EngineState, ExecConfig, ExecuteRequest, GenesisAccount, RewardItem,
        },
        execution,
    },
    shared::newtypes::CorrelationId,
    storage::{
        global_state::{CommitProvider, StateProvider},
        trie::{Pointer, Trie},
    },
};
use casper_hashing::Digest;
use casper_types::{
    account::AccountHash,
    bytesrepr::{self},
    runtime_args,
    system::auction,
    Key, Motes, ProtocolVersion, PublicKey, RuntimeArgs, SecretKey, StoredValue, U512,
};

use rand::Rng;

use crate::{
    transfer, DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, StepRequestBuilder,
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

const TEST_DELEGATOR_INITIAL_ACCOUNT_BALANCE: u64 = 1_000_000 * 1_000_000_000;

/// Run a block with transfers and optionally run step.
#[allow(clippy::too_many_arguments)]
pub fn run_blocks_with_transfers_and_step(
    transfer_count: usize,
    purse_count: usize,
    use_scratch: bool,
    run_auction: bool,
    block_count: usize,
    delegator_count: usize,
    validator_count: usize,
    mut report_writer: impl Write,
) {
    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = LmdbWasmTestBuilder::new(data_dir.as_ref());
    let delegator_keys = generate_public_keys(delegator_count);
    let validator_keys = generate_public_keys(validator_count);
    let mut necessary_tries = HashSet::new();

    run_genesis_and_create_initial_accounts(
        &mut builder,
        &validator_keys,
        delegator_keys
            .iter()
            .map(|pk| pk.to_account_hash())
            .collect::<Vec<_>>(),
        U512::from(TEST_DELEGATOR_INITIAL_ACCOUNT_BALANCE),
    );
    let contract_hash = builder.get_auction_contract_hash();
    let mut next_validator_iter = validator_keys.iter().cycle();

    for delegator_public_key in delegator_keys {
        let delegation_amount = U512::from(2000 * 1_000_000_000u64);
        let delegator_account_hash = delegator_public_key.to_account_hash();
        let next_validator_key = next_validator_iter
            .next()
            .expect("should produce values forever");
        let delegate = create_delegate_request(
            delegator_public_key,
            next_validator_key.clone(),
            delegation_amount,
            delegator_account_hash,
            contract_hash,
        );
        builder.exec(delegate);
        builder.expect_success();
        builder.commit();
        builder.clear_results();
    }

    let purse_amount = U512::from(1_000_000_000u64);

    let purses = transfer::create_test_purses(
        &mut builder,
        *DEFAULT_ACCOUNT_ADDR,
        purse_count as u64,
        purse_amount,
    );

    let exec_requests = transfer::create_multiple_native_transfers_to_purses(
        *DEFAULT_ACCOUNT_ADDR,
        transfer_count,
        &purses,
    );

    let mut total_transfers = 0;
    {
        let engine_state = builder.get_engine_state();
        let lmdb_env = engine_state.get_state().environment().env();
        let db = engine_state.get_state().trie_store().get_db();

        let txn = lmdb_env.begin_ro_txn().unwrap();
        let mut cursor = txn.open_ro_cursor(db).unwrap();

        let existing_keys = cursor
            .iter()
            .map(|(key, _)| Digest::try_from(key).expect("should be a digest"));
        necessary_tries.extend(existing_keys);
    }
    writeln!(
        report_writer,
        "height,db-size,transfers,time_ms,necessary_tries,total_tries"
    )
    .unwrap();
    // simulating a block boundary here.
    for current_block in 0..block_count {
        let start = Instant::now();
        total_transfers += exec_requests.len();

        transfer::transfer_to_account_multiple_native_transfers(
            &mut builder,
            &exec_requests,
            use_scratch,
        );
        let transfer_root = builder.get_post_state_hash();
        let maybe_auction_root = if run_auction {
            if use_scratch {
                step_and_run_auction(&mut builder, &validator_keys);
            } else {
                builder.advance_era(
                    validator_keys
                        .iter()
                        .cloned()
                        .map(|id| RewardItem::new(id, 1)),
                );
                builder.commit();
            }
            Some(builder.get_post_state_hash())
        } else {
            None
        };
        let exec_time = start.elapsed();
        find_necessary_tries(
            builder.get_engine_state(),
            &mut necessary_tries,
            transfer_root,
        );

        if let Some(auction_root) = maybe_auction_root {
            find_necessary_tries(
                builder.get_engine_state(),
                &mut necessary_tries,
                auction_root,
            );
        }

        let total_tries = {
            let engine_state = builder.get_engine_state();
            let lmdb_env = engine_state.get_state().environment().env();
            let db = engine_state.get_state().trie_store().get_db();
            let txn = lmdb_env.begin_ro_txn().unwrap();
            let mut cursor = txn.open_ro_cursor(db).unwrap();
            cursor.iter().count()
        };

        if use_scratch {
            // This assertion is only valid with the scratch trie.
            assert_eq!(
                necessary_tries.len(),
                total_tries,
                "should not create unnecessary tries"
            );
        }

        writeln!(
            report_writer,
            "{},{},{},{},{},{}",
            current_block,
            builder.lmdb_on_disk_size().unwrap(),
            total_transfers,
            exec_time.as_millis() as usize,
            necessary_tries.len(),
            total_tries,
        )
        .unwrap();
        report_writer.flush().unwrap();
    }
}

// find all necessary tries - hoist to FN
fn find_necessary_tries<S>(
    engine_state: &EngineState<S>,
    necessary_tries: &mut HashSet<Digest>,
    state_root: Digest,
) where
    S: StateProvider + CommitProvider,
    S::Error: Into<execution::Error>,
    engine_state::Error: From<S::Error>,
{
    let mut queue = Vec::new();
    queue.push(state_root);

    while let Some(root) = queue.pop() {
        if necessary_tries.contains(&root) {
            continue;
        }
        necessary_tries.insert(root);

        let trie_bytes = engine_state
            .get_trie_full(CorrelationId::new(), root)
            .unwrap()
            .expect("trie should exist")
            .into_inner();

        if let Some(0) = trie_bytes.first() {
            continue;
        }

        let trie: Trie<Key, StoredValue> =
            bytesrepr::deserialize(trie_bytes.inner_bytes().to_owned())
                .expect("unable to deserialize");

        match trie {
            Trie::Leaf { .. } => continue,
            Trie::Node { pointer_block } => queue.extend(pointer_block.as_indexed_pointers().map(
                |(_idx, ptr)| match ptr {
                    Pointer::LeafPointer(digest) | Pointer::NodePointer(digest) => digest,
                },
            )),
            Trie::Extension { affix: _, pointer } => match pointer {
                Pointer::LeafPointer(digest) | Pointer::NodePointer(digest) => queue.push(digest),
            },
        }
    }
}

/// Runs genesis, creates system, validator and delegator accounts, and funds the system account and
/// delegator accounts.
pub fn run_genesis_and_create_initial_accounts(
    builder: &mut LmdbWasmTestBuilder,
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
pub fn step_and_run_auction(builder: &mut LmdbWasmTestBuilder, validator_keys: &[PublicKey]) {
    let mut step_request_builder = StepRequestBuilder::new()
        .with_parent_state_hash(builder.get_post_state_hash())
        .with_protocol_version(ProtocolVersion::V1_0_0);
    for validator in validator_keys {
        step_request_builder =
            step_request_builder.with_reward_item(RewardItem::new(validator.clone(), 1));
    }
    let step_request = step_request_builder
        .with_next_era_id(builder.get_era().successor())
        .build();
    builder.step_with_scratch(step_request);
    builder.write_scratch_to_db();
}
