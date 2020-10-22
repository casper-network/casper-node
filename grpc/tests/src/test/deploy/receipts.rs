use std::collections::{BTreeMap, BTreeSet};

use lazy_static::lazy_static;

use casper_engine_test_support::{
    internal::{ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_RUN_GENESIS_REQUEST},
    DEFAULT_ACCOUNT_ADDR,
};
use casper_types::{
    account::AccountHash, runtime_args, AccessRights, DeployHash, PublicKey, RuntimeArgs, Transfer,
    TransferAddr, U512,
};

const CONTRACT_TRANSFER_PURSE_TO_ACCOUNT: &str = "transfer_purse_to_account.wasm";
const TRANSFER_ARG_TARGET: &str = "target";
const TRANSFER_ARG_AMOUNT: &str = "amount";

const CONTRACT_TRANSFER_PURSE_TO_ACCOUNTS: &str = "transfer_purse_to_accounts.wasm";
const TRANSFER_ARG_SOURCE: &str = "source";
const TRANSFER_ARG_TARGETS: &str = "targets";

const CONTRACT_TRANSFER_PURSE_TO_ACCOUNTS_STORED: &str = "transfer_purse_to_accounts_stored.wasm";
const CONTRACT_TRANSFER_PURSE_TO_ACCOUNTS_SUBCALL: &str = "transfer_purse_to_accounts_subcall.wasm";

const ALICE_KEY: PublicKey = PublicKey::Ed25519([3; 32]);
const BOB_KEY: PublicKey = PublicKey::Ed25519([5; 32]);
const CAROL_KEY: PublicKey = PublicKey::Ed25519([7; 32]);

lazy_static! {
    static ref ALICE_ADDR: AccountHash = ALICE_KEY.into();
    static ref BOB_ADDR: AccountHash = BOB_KEY.into();
    static ref CAROL_ADDR: AccountHash = CAROL_KEY.into();
    static ref TRANSFER_AMOUNT_1: U512 = U512::from(100_000_000);
    static ref TRANSFER_AMOUNT_2: U512 = U512::from(200_000_000);
    static ref TRANSFER_AMOUNT_3: U512 = U512::from(300_000_000);
}

#[ignore]
#[test]
fn should_record_wasmless_transfer() {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let transfer_request = ExecuteRequestBuilder::transfer(
        *DEFAULT_ACCOUNT_ADDR,
        runtime_args! {
            TRANSFER_ARG_TARGET => *ALICE_ADDR,
            TRANSFER_ARG_AMOUNT => *TRANSFER_AMOUNT_1

        },
    )
    .build();

    let deploy_hash = {
        let deploy_items: Vec<DeployHash> = transfer_request
            .deploys()
            .iter()
            .map(Result::as_ref)
            .filter_map(Result::ok)
            .map(|deploy_item| deploy_item.deploy_hash)
            .collect();
        deploy_items[0]
    };

    builder.exec(transfer_request).commit().expect_success();

    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");

    let alice_account = builder
        .get_account(*ALICE_ADDR)
        .expect("should have Alice's account");

    let alice_attenuated_main_purse = alice_account
        .main_purse()
        .with_access_rights(AccessRights::ADD);

    let deploy_info = builder
        .get_deploy_info(deploy_hash)
        .expect("should have deploy info");

    assert_eq!(deploy_info.deploy_hash, deploy_hash);
    assert_eq!(deploy_info.from, *DEFAULT_ACCOUNT_ADDR);
    assert_eq!(deploy_info.source, default_account.main_purse());

    // TODO: this is borked
    if !cfg!(feature = "use-system-contracts") {
        assert_eq!(deploy_info.gas, U512::zero());
    }

    let transfers = deploy_info.transfers;
    assert_eq!(transfers.len(), 1);

    let transfer = builder
        .get_transfer(transfers[0])
        .expect("should have transfer");

    assert_eq!(transfer.deploy_hash, deploy_hash);
    assert_eq!(transfer.from, *DEFAULT_ACCOUNT_ADDR);
    assert_eq!(transfer.source, default_account.main_purse());
    assert_eq!(transfer.target, alice_attenuated_main_purse);
    assert_eq!(transfer.amount, *TRANSFER_AMOUNT_1);
    assert_eq!(transfer.gas, U512::zero()) // TODO
}

#[ignore]
#[test]
fn should_record_wasm_transfer() {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let transfer_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_PURSE_TO_ACCOUNT,
        runtime_args! {
            TRANSFER_ARG_TARGET => *ALICE_ADDR,
            TRANSFER_ARG_AMOUNT => *TRANSFER_AMOUNT_1

        },
    )
    .build();

    let deploy_hash = {
        let deploy_items: Vec<DeployHash> = transfer_request
            .deploys()
            .iter()
            .map(Result::as_ref)
            .filter_map(Result::ok)
            .map(|deploy_item| deploy_item.deploy_hash)
            .collect();
        deploy_items[0]
    };

    builder.exec(transfer_request).commit().expect_success();

    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");

    let alice_account = builder
        .get_account(*ALICE_ADDR)
        .expect("should have Alice's account");

    let alice_attenuated_main_purse = alice_account
        .main_purse()
        .with_access_rights(AccessRights::ADD);

    let deploy_info = builder
        .get_deploy_info(deploy_hash)
        .expect("should have deploy info");

    assert_eq!(deploy_info.deploy_hash, deploy_hash);
    assert_eq!(deploy_info.from, *DEFAULT_ACCOUNT_ADDR);
    assert_eq!(deploy_info.source, default_account.main_purse());
    assert_ne!(deploy_info.gas, U512::zero());

    let transfers = deploy_info.transfers;
    assert_eq!(transfers.len(), 1);

    let transfer = builder
        .get_transfer(transfers[0])
        .expect("should have transfer");

    assert_eq!(transfer.deploy_hash, deploy_hash);
    assert_eq!(transfer.from, *DEFAULT_ACCOUNT_ADDR);
    assert_eq!(transfer.source, default_account.main_purse());
    assert_eq!(transfer.target, alice_attenuated_main_purse);
    assert_eq!(transfer.amount, *TRANSFER_AMOUNT_1);
    assert_eq!(transfer.gas, U512::zero()) // TODO
}

#[ignore]
#[test]
fn should_record_wasm_transfers() {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");

    let targets: BTreeMap<AccountHash, U512> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(*ALICE_ADDR, *TRANSFER_AMOUNT_1);
        tmp.insert(*BOB_ADDR, *TRANSFER_AMOUNT_2);
        tmp.insert(*CAROL_ADDR, *TRANSFER_AMOUNT_3);
        tmp
    };

    let transfer_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_PURSE_TO_ACCOUNTS,
        runtime_args! {
            TRANSFER_ARG_SOURCE => default_account.main_purse(),
            TRANSFER_ARG_TARGETS => targets,
        },
    )
    .build();

    let deploy_hash = {
        let deploy_items: Vec<DeployHash> = transfer_request
            .deploys()
            .iter()
            .map(Result::as_ref)
            .filter_map(Result::ok)
            .map(|deploy_item| deploy_item.deploy_hash)
            .collect();
        deploy_items[0]
    };

    builder.exec(transfer_request).commit().expect_success();

    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");

    let alice_account = builder
        .get_account(*ALICE_ADDR)
        .expect("should have Alice's account");

    let bob_account = builder
        .get_account(*BOB_ADDR)
        .expect("should have Bob's account");

    let carol_account = builder
        .get_account(*CAROL_ADDR)
        .expect("should have Carol's account");

    let alice_attenuated_main_purse = alice_account
        .main_purse()
        .with_access_rights(AccessRights::ADD);

    let bob_attenuated_main_purse = bob_account
        .main_purse()
        .with_access_rights(AccessRights::ADD);

    let carol_attenuated_main_purse = carol_account
        .main_purse()
        .with_access_rights(AccessRights::ADD);

    let deploy_info = builder
        .get_deploy_info(deploy_hash)
        .expect("should have deploy info");

    assert_eq!(deploy_info.deploy_hash, deploy_hash);
    assert_eq!(deploy_info.from, *DEFAULT_ACCOUNT_ADDR);
    assert_eq!(deploy_info.source, default_account.main_purse());
    assert_ne!(deploy_info.gas, U512::zero());

    const EXPECTED_LENGTH: usize = 3;
    let transfer_addrs = deploy_info.transfers;
    assert_eq!(transfer_addrs.len(), EXPECTED_LENGTH);
    assert_eq!(
        transfer_addrs
            .iter()
            .cloned()
            .collect::<BTreeSet<TransferAddr>>()
            .len(),
        EXPECTED_LENGTH
    );

    let transfers: BTreeSet<Transfer> = {
        let mut tmp = BTreeSet::new();
        for transfer_addr in transfer_addrs {
            let transfer = builder
                .get_transfer(transfer_addr)
                .expect("should have transfer");
            tmp.insert(transfer);
        }
        tmp
    };

    assert_eq!(transfers.len(), EXPECTED_LENGTH);

    assert!(transfers.contains(&Transfer {
        deploy_hash,
        from: *DEFAULT_ACCOUNT_ADDR,
        source: default_account.main_purse(),
        target: alice_attenuated_main_purse,
        amount: *TRANSFER_AMOUNT_1,
        gas: U512::zero(),
    }));

    assert!(transfers.contains(&Transfer {
        deploy_hash,
        from: *DEFAULT_ACCOUNT_ADDR,
        source: default_account.main_purse(),
        target: bob_attenuated_main_purse,
        amount: *TRANSFER_AMOUNT_2,
        gas: U512::zero(),
    }));

    assert!(transfers.contains(&Transfer {
        deploy_hash,
        from: *DEFAULT_ACCOUNT_ADDR,
        source: default_account.main_purse(),
        target: carol_attenuated_main_purse,
        amount: *TRANSFER_AMOUNT_3,
        gas: U512::zero(),
    }));
}

#[ignore]
#[test]
fn should_record_wasm_transfers_with_subcall() {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");

    let store_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_PURSE_TO_ACCOUNTS_STORED,
        runtime_args! {},
    )
    .build();

    let targets: BTreeMap<AccountHash, U512> = {
        let mut tmp = BTreeMap::new();
        tmp.insert(*ALICE_ADDR, *TRANSFER_AMOUNT_1);
        tmp.insert(*BOB_ADDR, *TRANSFER_AMOUNT_2);
        tmp.insert(*CAROL_ADDR, *TRANSFER_AMOUNT_3);
        tmp
    };

    let transfer_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_PURSE_TO_ACCOUNTS_SUBCALL,
        runtime_args! {
            TRANSFER_ARG_SOURCE => default_account.main_purse(),
            TRANSFER_ARG_TARGETS => targets,
        },
    )
    .build();

    let transfer_deploy_hash = {
        let deploy_items: Vec<DeployHash> = transfer_request
            .deploys()
            .iter()
            .map(Result::as_ref)
            .filter_map(Result::ok)
            .map(|deploy_item| deploy_item.deploy_hash)
            .collect();
        deploy_items[0]
    };

    builder.exec(store_request).commit().expect_success();
    builder.exec(transfer_request).commit().expect_success();

    let alice_account = builder
        .get_account(*ALICE_ADDR)
        .expect("should have Alice's account");

    let bob_account = builder
        .get_account(*BOB_ADDR)
        .expect("should have Bob's account");

    let carol_account = builder
        .get_account(*CAROL_ADDR)
        .expect("should have Carol's account");

    let alice_attenuated_main_purse = alice_account
        .main_purse()
        .with_access_rights(AccessRights::ADD);

    let bob_attenuated_main_purse = bob_account
        .main_purse()
        .with_access_rights(AccessRights::ADD);

    let carol_attenuated_main_purse = carol_account
        .main_purse()
        .with_access_rights(AccessRights::ADD);

    let deploy_info = builder
        .get_deploy_info(transfer_deploy_hash)
        .expect("should have deploy info");

    assert_eq!(deploy_info.deploy_hash, transfer_deploy_hash);
    assert_eq!(deploy_info.from, *DEFAULT_ACCOUNT_ADDR);
    assert_eq!(deploy_info.source, default_account.main_purse());
    assert_ne!(deploy_info.gas, U512::zero());

    const EXPECTED_LENGTH: usize = 6;
    let transfer_addrs = deploy_info.transfers;
    assert_eq!(transfer_addrs.len(), EXPECTED_LENGTH);
    assert_eq!(
        transfer_addrs
            .iter()
            .cloned()
            .collect::<BTreeSet<TransferAddr>>()
            .len(),
        EXPECTED_LENGTH
    );

    let transfer_counts: BTreeMap<Transfer, usize> = {
        let mut tmp = BTreeMap::new();
        for transfer_addr in transfer_addrs {
            let transfer = builder
                .get_transfer(transfer_addr)
                .expect("should have transfer");
            tmp.entry(transfer).and_modify(|i| *i += 1).or_insert(1);
        }
        tmp
    };

    let expected_alice = Transfer {
        deploy_hash: transfer_deploy_hash,
        from: *DEFAULT_ACCOUNT_ADDR,
        source: default_account.main_purse(),
        target: alice_attenuated_main_purse,
        amount: *TRANSFER_AMOUNT_1,
        gas: U512::zero(),
    };

    let expected_bob = Transfer {
        deploy_hash: transfer_deploy_hash,
        from: *DEFAULT_ACCOUNT_ADDR,
        source: default_account.main_purse(),
        target: bob_attenuated_main_purse,
        amount: *TRANSFER_AMOUNT_2,
        gas: U512::zero(),
    };

    let expected_carol = Transfer {
        deploy_hash: transfer_deploy_hash,
        from: *DEFAULT_ACCOUNT_ADDR,
        source: default_account.main_purse(),
        target: carol_attenuated_main_purse,
        amount: *TRANSFER_AMOUNT_3,
        gas: U512::zero(),
    };

    const EXPECTED_COUNT: Option<usize> = Some(2);
    for expected in &[expected_alice, expected_bob, expected_carol] {
        assert_eq!(transfer_counts.get(&expected).cloned(), EXPECTED_COUNT);
    }
}
