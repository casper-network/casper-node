use num_traits::Zero;

use casper_engine_test_support::{
    utils, ExecuteRequestBuilder, LmdbWasmTestBuilder, TransferRequestBuilder,
    UpgradeRequestBuilder, DEFAULT_ACCOUNTS, DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_PUBLIC_KEY,
    DEFAULT_GENESIS_TIMESTAMP_MILLIS, DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
    DEFAULT_PROPOSER_PUBLIC_KEY, DEFAULT_PROTOCOL_VERSION, DEFAULT_UNBONDING_DELAY,
    LOCAL_GENESIS_REQUEST, MINIMUM_ACCOUNT_CREATION_BALANCE, SYSTEM_ADDR,
    TIMESTAMP_MILLIS_INCREMENT,
};
use casper_execution_engine::{engine_state::Error as EngineError, execution::ExecError};

use casper_types::{
    account::AccountHash,
    runtime_args,
    system::{
        auction::{
            self, BidsExt, DelegationRate, UnbondKind, ARG_VALIDATOR_PUBLIC_KEYS, INITIAL_ERA_ID,
            METHOD_SLASH,
        },
        mint,
    },
    ApiError, EraId, GenesisAccount, GenesisValidator, Motes, ProtocolVersion, PublicKey,
    SecretKey, DEFAULT_MINIMUM_BID_AMOUNT, U512,
};

const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const CONTRACT_ADD_BID: &str = "add_bid.wasm";
const CONTRACT_WITHDRAW_BID: &str = "withdraw_bid.wasm";

const GENESIS_VALIDATOR_STAKE: u64 = 50_000;
const GENESIS_ACCOUNT_STAKE: u64 = 100_000;
const TRANSFER_AMOUNT: u64 = MINIMUM_ACCOUNT_CREATION_BALANCE;

const ARG_AMOUNT: &str = "amount";
const ARG_PUBLIC_KEY: &str = "public_key";
const ARG_DELEGATION_RATE: &str = "delegation_rate";

const DELEGATION_RATE: DelegationRate = 42;

#[ignore]
#[test]
fn should_run_successful_bond_and_unbond_and_slashing() {
    let default_public_key_arg = DEFAULT_ACCOUNT_PUBLIC_KEY.clone();
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => *SYSTEM_ADDR,
            "amount" => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    let _default_account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get account 1");

    let auction = builder.get_auction_contract_hash();

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(GENESIS_ACCOUNT_STAKE),
            ARG_PUBLIC_KEY => default_public_key_arg.clone(),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
        },
    )
    .build();

    builder.exec(exec_request_1).expect_success().commit();

    let bids = builder.get_bids();
    let default_account_bid = bids
        .validator_bid(&DEFAULT_ACCOUNT_PUBLIC_KEY)
        .expect("should have bid");
    let bid_purse = *default_account_bid.bonding_purse();
    assert_eq!(
        builder.get_purse_balance(bid_purse),
        GENESIS_ACCOUNT_STAKE.into()
    );

    let unbond_purses = builder.get_unbonds();
    assert_eq!(unbond_purses.len(), 0);

    //
    // Partial unbond
    //

    let unbond_amount = U512::from(GENESIS_ACCOUNT_STAKE - DEFAULT_MINIMUM_BID_AMOUNT);

    let unbonding_purse = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account")
        .main_purse();
    let exec_request_3 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_WITHDRAW_BID,
        runtime_args! {
            ARG_AMOUNT => unbond_amount,
            ARG_PUBLIC_KEY => default_public_key_arg.clone(),
        },
    )
    .build();

    builder.exec(exec_request_3).expect_success().commit();

    let account_balance_before = builder.get_purse_balance(unbonding_purse);

    let unbonds = builder.get_unbonds();
    let unbond = {
        assert_eq!(unbonds.len(), 1);
        let unbond_kind = UnbondKind::Validator((*DEFAULT_ACCOUNT_PUBLIC_KEY).clone());
        let unbond = unbonds
            .get(&unbond_kind)
            .expect("should have unbond")
            .first()
            .expect("must have one unbond entry");
        assert_eq!(unbond.eras().len(), 1, "unexpected era count");
        assert_eq!(unbond.validator_public_key(), &default_public_key_arg,);
        assert!(unbond.is_validator());
        unbond
    };

    let unbond_era_1 = unbond.eras().first().expect("should have era");
    assert_eq!(unbond_era_1.era_of_creation(), INITIAL_ERA_ID,);

    builder.run_auction(
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS,
        Vec::new(),
    );

    let unbonds = builder.get_unbonds();
    let unbond = {
        assert_eq!(unbonds.len(), 1);

        let unbond = unbonds
            .get(&UnbondKind::Validator(
                (*DEFAULT_ACCOUNT_PUBLIC_KEY).clone(),
            ))
            .expect("should have unbond")
            .first()
            .expect("must have one unbond entry");
        assert_eq!(unbond.eras().len(), 1);
        assert_eq!(unbond.validator_public_key(), &default_public_key_arg,);
        assert!(unbond.is_validator());
        unbond
    };

    let account_balance = builder.get_purse_balance(unbonding_purse);
    assert_eq!(account_balance_before, account_balance);

    let unbond_era_2 = unbond.eras().first().expect("should have eras");
    assert_eq!(unbond_era_2.amount(), &unbond_amount,);

    assert_eq!(unbond_era_2, unbond_era_1);

    let exec_request_5 = ExecuteRequestBuilder::contract_call_by_hash(
        *SYSTEM_ADDR,
        auction,
        METHOD_SLASH,
        runtime_args! {
            ARG_VALIDATOR_PUBLIC_KEYS => vec![
               default_public_key_arg,
            ]
        },
    )
    .build();

    builder.exec(exec_request_5).expect_success().commit();

    let unbonds = builder.get_unbonds();
    let unbond_kind = UnbondKind::Validator((*DEFAULT_ACCOUNT_PUBLIC_KEY).clone());
    assert!(!unbonds.contains_key(&unbond_kind));

    let bids = builder.get_bids();
    assert!(bids.validator_bid(&DEFAULT_ACCOUNT_PUBLIC_KEY).is_none());

    let account_balance_after_slashing = builder.get_purse_balance(unbonding_purse);
    assert_eq!(account_balance_after_slashing, account_balance_before);
}

#[ignore]
#[test]
fn should_fail_bonding_with_insufficient_funds_directly() {
    let new_validator_sk = SecretKey::ed25519_from_bytes([123; SecretKey::ED25519_LENGTH]).unwrap();
    let new_validator_pk: PublicKey = (&new_validator_sk).into();
    let new_validator_hash = AccountHash::from(&new_validator_pk);
    assert_ne!(&DEFAULT_PROPOSER_PUBLIC_KEY.clone(), &new_validator_pk);

    let mut builder = LmdbWasmTestBuilder::default();

    let transfer_amount = U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE);
    let delegation_rate: DelegationRate = 10;

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let exec_request = TransferRequestBuilder::new(transfer_amount, new_validator_hash)
        .with_transfer_id(1)
        .build();

    builder.transfer_and_commit(exec_request).expect_success();

    let new_validator_account = builder
        .get_entity_by_account_hash(new_validator_hash)
        .expect("should work");

    let new_validator_balance = builder.get_purse_balance(new_validator_account.main_purse());

    assert_eq!(new_validator_balance, transfer_amount,);

    let add_bid_request = ExecuteRequestBuilder::contract_call_by_hash(
        new_validator_hash,
        builder.get_auction_contract_hash(),
        auction::METHOD_ADD_BID,
        runtime_args! {
            auction::ARG_PUBLIC_KEY => new_validator_pk,
            auction::ARG_AMOUNT => new_validator_balance + U512::one(),
            auction::ARG_DELEGATION_RATE => delegation_rate,
        },
    )
    .build();
    builder.exec(add_bid_request);

    let error = builder.get_error().expect("should be error");
    assert!(
        matches!(
            error,
            EngineError::Exec(ExecError::Revert(ApiError::Mint(mint_error))
        )
        if mint_error == mint::Error::InsufficientFunds as u8),
        "{:?}",
        error
    );
}

#[ignore]
#[test]
fn should_fail_unbonding_validator_with_locked_funds() {
    let account_1_secret_key =
        SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH]).unwrap();
    let account_1_public_key = PublicKey::from(&account_1_secret_key);
    let account_1_hash = AccountHash::from(&account_1_public_key);
    let account_1_balance = U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE);

    let accounts = {
        let mut tmp: Vec<GenesisAccount> = DEFAULT_ACCOUNTS.clone();
        let account = GenesisAccount::account(
            account_1_public_key.clone(),
            Motes::new(account_1_balance),
            Some(GenesisValidator::new(
                Motes::new(GENESIS_VALIDATOR_STAKE),
                DelegationRate::zero(),
            )),
        );
        tmp.push(account);
        tmp
    };

    let run_genesis_request = utils::create_run_genesis_request(accounts);

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(run_genesis_request);

    let exec_request_2 = ExecuteRequestBuilder::standard(
        account_1_hash,
        CONTRACT_WITHDRAW_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(42),
            ARG_PUBLIC_KEY => account_1_public_key,
        },
    )
    .build();

    builder.exec(exec_request_2).commit();

    let error_message = builder.get_error_message().expect("should have a result");

    // handle_payment::Error::NotBonded => 0
    assert!(
        error_message.contains(&format!(
            "{:?}",
            ApiError::from(auction::Error::ValidatorFundsLocked)
        )),
        "error {:?}",
        error_message
    );
}

#[ignore]
#[test]
fn should_fail_unbonding_validator_without_bonding_first() {
    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_WITHDRAW_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(42),
            ARG_PUBLIC_KEY => DEFAULT_ACCOUNT_PUBLIC_KEY.clone(),
        },
    )
    .build();

    let mut builder = LmdbWasmTestBuilder::default();

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    builder.exec(exec_request).commit();

    let error_message = builder.get_error_message().expect("should have a result");

    assert!(
        error_message.contains(&format!(
            "{:?}",
            ApiError::from(auction::Error::ValidatorNotFound)
        )),
        "error {:?}",
        error_message
    );
}

#[ignore]
#[test]
fn should_run_successful_bond_and_unbond_with_release() {
    let default_public_key_arg = DEFAULT_ACCOUNT_PUBLIC_KEY.clone();

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let default_account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");

    let unbonding_purse = default_account.main_purse();

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => *SYSTEM_ADDR,
            "amount" => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    let _default_account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get account 1");

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(GENESIS_ACCOUNT_STAKE),
            ARG_PUBLIC_KEY => default_public_key_arg.clone(),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
        },
    )
    .build();

    builder.exec(exec_request_1).expect_success().commit();

    let bids = builder.get_bids();
    let bid = bids
        .validator_bid(&default_public_key_arg)
        .expect("should have bid");
    let bid_purse = *bid.bonding_purse();
    assert_eq!(
        builder.get_purse_balance(bid_purse),
        GENESIS_ACCOUNT_STAKE.into()
    );

    let unbond_purses = builder.get_unbonds();
    assert_eq!(unbond_purses.len(), 0);

    //
    // Advance era by calling run_auction
    //
    builder.run_auction(timestamp_millis, Vec::new());
    timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    //
    // Partial unbond
    //

    let unbond_amount = U512::from(GENESIS_ACCOUNT_STAKE) - DEFAULT_MINIMUM_BID_AMOUNT;

    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_WITHDRAW_BID,
        runtime_args! {
            ARG_AMOUNT => unbond_amount,
            ARG_PUBLIC_KEY => default_public_key_arg.clone(),
        },
    )
    .build();

    builder.exec(exec_request_2).expect_success().commit();

    let unbonds = builder.get_unbonds();
    assert_eq!(unbonds.len(), 1);

    let unbond_kind = UnbondKind::Validator((*DEFAULT_ACCOUNT_PUBLIC_KEY).clone());
    let unbond = unbonds
        .get(&unbond_kind)
        .expect("should have unbond")
        .first()
        .expect("must have one unbond entry");
    assert_eq!(unbond.eras().len(), 1);
    assert_eq!(unbond.validator_public_key(), &default_public_key_arg,);
    assert!(unbond.is_validator());

    let era = unbond.eras().first().expect("should have era");
    assert_eq!(*era.amount(), unbond_amount);
    let unbond_era_1 = era.era_of_creation();
    assert_eq!(unbond_era_1, INITIAL_ERA_ID + 1);

    let account_balance_before_auction = builder.get_purse_balance(unbonding_purse);

    builder.run_auction(timestamp_millis, Vec::new());
    timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;

    assert_eq!(
        builder.get_purse_balance(unbonding_purse),
        account_balance_before_auction, // Not paid yet
    );

    let unbond_era_2 = unbond
        .eras()
        .first()
        .expect("should have eras")
        .era_of_creation();

    assert_eq!(unbond_era_2, unbond_era_1); // era of withdrawal didn't change since first run

    //
    // Advance state to hit the unbonding period
    //
    for _ in 0..DEFAULT_UNBONDING_DELAY {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    // Should pay out
    builder.run_auction(timestamp_millis, Vec::new());
    assert_eq!(
        builder.get_purse_balance(unbonding_purse),
        account_balance_before_auction + unbond_amount
    );

    let unbonds = builder.get_unbonds();
    let unbond_kind = UnbondKind::Validator((*DEFAULT_ACCOUNT_PUBLIC_KEY).clone());
    assert!(!unbonds.contains_key(&unbond_kind));

    let bids = builder.get_bids();
    assert!(!bids.is_empty());

    let bid = bids
        .validator_bid(&default_public_key_arg)
        .expect("should have bid");
    let bid_purse = *bid.bonding_purse();
    assert_eq!(
        builder.get_purse_balance(bid_purse),
        U512::from(GENESIS_ACCOUNT_STAKE) - unbond_amount, // remaining funds
    );
}

#[ignore]
#[test]
fn should_run_successful_unbond_funds_after_changing_unbonding_delay() {
    let default_public_key_arg = DEFAULT_ACCOUNT_PUBLIC_KEY.clone();

    let mut timestamp_millis =
        DEFAULT_GENESIS_TIMESTAMP_MILLIS + DEFAULT_LOCKED_FUNDS_PERIOD_MILLIS;

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let new_unbonding_delay = DEFAULT_UNBONDING_DELAY + 5;

    let old_protocol_version = DEFAULT_PROTOCOL_VERSION;
    let sem_ver = old_protocol_version.value();
    let new_protocol_version =
        ProtocolVersion::from_parts(sem_ver.major, sem_ver.minor, sem_ver.patch + 1);
    let default_activation_point = EraId::from(0);

    let mut upgrade_request = {
        UpgradeRequestBuilder::new()
            .with_current_protocol_version(old_protocol_version)
            .with_new_protocol_version(new_protocol_version)
            .with_activation_point(default_activation_point)
            .with_new_unbonding_delay(new_unbonding_delay)
            .build()
    };

    builder.upgrade(&mut upgrade_request);

    let default_account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should have default account");

    let unbonding_purse = default_account.main_purse();

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            "target" => *SYSTEM_ADDR,
            "amount" => U512::from(TRANSFER_AMOUNT)
        },
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    let _default_account = builder
        .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get account 1");

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_AMOUNT => U512::from(GENESIS_ACCOUNT_STAKE),
            ARG_PUBLIC_KEY => default_public_key_arg.clone(),
            ARG_DELEGATION_RATE => DELEGATION_RATE,
        },
    )
    .build();

    builder.exec(exec_request_1).expect_success().commit();

    let bids = builder.get_bids();
    let bid = bids
        .validator_bid(&default_public_key_arg)
        .expect("should have bid");
    let bid_purse = *bid.bonding_purse();
    assert_eq!(
        builder.get_purse_balance(bid_purse),
        GENESIS_ACCOUNT_STAKE.into()
    );

    let unbond_purses = builder.get_unbonds();
    assert_eq!(unbond_purses.len(), 0);

    //
    // Advance era by calling run_auction
    //
    builder.run_auction(timestamp_millis, Vec::new());

    //
    // Partial unbond
    //

    let unbond_amount = U512::from(GENESIS_ACCOUNT_STAKE) - DEFAULT_MINIMUM_BID_AMOUNT;

    let exec_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_WITHDRAW_BID,
        runtime_args! {
            ARG_AMOUNT => unbond_amount,
            ARG_PUBLIC_KEY => default_public_key_arg.clone(),
        },
    )
    .build();

    builder.exec(exec_request_2).expect_success().commit();

    let account_balance_before_auction = builder.get_purse_balance(unbonding_purse);

    let unbonds = builder.get_unbonds();
    assert_eq!(unbonds.len(), 1);

    let unbond_kind = UnbondKind::Validator((*DEFAULT_ACCOUNT_PUBLIC_KEY).clone());
    let unbond = unbonds
        .get(&unbond_kind)
        .expect("should have unbond")
        .first()
        .expect("must have one unbond entry");
    assert_eq!(unbond.eras().len(), 1);
    assert_eq!(unbond.validator_public_key(), &default_public_key_arg,);
    assert!(unbond.is_validator());

    let era = unbond.eras().first().expect("should have eras");
    assert_eq!(era.era_of_creation(), INITIAL_ERA_ID + 1);

    let unbond_era_1 = era.era_of_creation();

    builder.run_auction(timestamp_millis, Vec::new());

    let unbond_purses = builder.get_unbonds();
    assert_eq!(unbond_purses.len(), 1);

    let unbond_kind = UnbondKind::Validator((*DEFAULT_ACCOUNT_PUBLIC_KEY).clone());
    let unbond = unbond_purses
        .get(&unbond_kind)
        .expect("should have unbond")
        .first()
        .expect("must have one unbond entry");
    assert_eq!(unbond.validator_public_key(), &default_public_key_arg,);
    assert!(unbond.is_validator());

    assert_eq!(
        builder.get_purse_balance(unbonding_purse),
        account_balance_before_auction, // Not paid yet
    );

    let unbond_era_2 = unbond
        .eras()
        .first()
        .expect("should have era")
        .era_of_creation();

    assert_eq!(unbond_era_2, unbond_era_1); // era of withdrawal didn't change since first run

    //
    // Advance state to hit the unbonding period
    //

    for _ in 0..DEFAULT_UNBONDING_DELAY {
        builder.run_auction(timestamp_millis, Vec::new());
        timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;
    }

    // Won't pay out (yet) as we increased unbonding period
    builder.run_auction(timestamp_millis, Vec::new());
    timestamp_millis += TIMESTAMP_MILLIS_INCREMENT;

    // Not paid yet
    assert_eq!(
        builder.get_purse_balance(unbonding_purse),
        account_balance_before_auction,
        "should not pay after reaching default unbond delay era"
    );

    // -1 below is the extra run auction above in `run_auction_request_1`
    for _ in 0..new_unbonding_delay - DEFAULT_UNBONDING_DELAY - 1 {
        builder.run_auction(timestamp_millis, Vec::new());
    }

    assert_eq!(
        builder.get_purse_balance(unbonding_purse),
        account_balance_before_auction + unbond_amount
    );

    let unbonds = builder.get_unbonds();
    let unbond_kind = UnbondKind::Validator((*DEFAULT_ACCOUNT_PUBLIC_KEY).clone());
    assert!(!unbonds.contains_key(&unbond_kind));

    let bids = builder.get_bids();
    assert!(!bids.is_empty());

    let bid = bids
        .validator_bid(&default_public_key_arg)
        .expect("should have bid");
    let bid_purse = *bid.bonding_purse();
    assert_eq!(
        builder.get_purse_balance(bid_purse),
        U512::from(GENESIS_ACCOUNT_STAKE) - unbond_amount, // remaining funds
    );
}
