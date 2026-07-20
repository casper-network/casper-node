use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_GENESIS_TIMESTAMP_MILLIS, DEFAULT_PROPOSER_PUBLIC_KEY, LOCAL_GENESIS_REQUEST,
};
use casper_types::{
    runtime_args,
    system::auction::{
        BidAddr, BidKind, BidsExt, DelegationRate, DelegatorBid, DelegatorKind, EraInfo,
        ValidatorBid, ARG_AMOUNT, ARG_NEW_VALIDATOR, ARG_VALIDATOR,
    },
    AddressableEntityHash, GenesisAccount, GenesisValidator, Key, Motes, PublicKey, SecretKey,
    StoredValue, U512,
};
use num_traits::Zero;

const STORED_STAKING_CONTRACT_NAME: &str = "staking_stored.wasm";

mod bids;
mod distribute;
mod reservations;

fn get_validator_bid(
    builder: &mut LmdbWasmTestBuilder,
    validator_public_key: PublicKey,
) -> Option<ValidatorBid> {
    let bids = builder.get_bids();
    bids.validator_bid(&validator_public_key)
}

pub fn get_delegator_staked_amount(
    builder: &mut LmdbWasmTestBuilder,
    validator_public_key: PublicKey,
    delegator_public_key: PublicKey,
) -> U512 {
    let bids = builder.get_bids();

    let delegator = bids
        .delegator_by_kind(&validator_public_key, &DelegatorKind::PublicKey(delegator_public_key.clone()))
        .expect("bid should exist for validator-{validator_public_key}, delegator-{delegator_public_key}");

    delegator.staked_amount()
}

pub fn get_era_info(builder: &mut LmdbWasmTestBuilder) -> EraInfo {
    let era_info_value = builder
        .query(None, Key::EraSummary, &[])
        .expect("should have value");

    era_info_value
        .as_era_info()
        .cloned()
        .expect("should be era info")
}

#[ignore]
#[test]
fn should_support_contract_staking() {
    const ARG_ACTION: &str = "action";
    let timestamp_millis = DEFAULT_GENESIS_TIMESTAMP_MILLIS;
    let purse_name = "staking_purse".to_string();
    let contract_name = "staking".to_string();
    let entry_point_name = "run".to_string();
    let stake = "STAKE".to_string();
    let unstake = "UNSTAKE".to_string();
    let restake = "RESTAKE".to_string();
    let get_staked_amount = "STAKED_AMOUNT".to_string();
    let account = *DEFAULT_ACCOUNT_ADDR;
    let seed_amount = U512::from(10_000_000_000_000_000_u64);
    let delegate_amount = U512::from(5_000_000_000_000_000_u64);
    let validator_pk = &*DEFAULT_PROPOSER_PUBLIC_KEY;
    let other_validator_pk = {
        let secret_key = SecretKey::ed25519_from_bytes([199; SecretKey::ED25519_LENGTH]).unwrap();
        PublicKey::from(&secret_key)
    };

    let mut builder = LmdbWasmTestBuilder::default();
    let mut genesis_request = LOCAL_GENESIS_REQUEST.clone();
    genesis_request.set_enable_entity(true);

    genesis_request.push_genesis_validator(
        validator_pk,
        GenesisValidator::new(
            Motes::new(10_000_000_000_000_000_u64),
            DelegationRate::zero(),
        ),
    );
    genesis_request.push_genesis_account(GenesisAccount::Account {
        public_key: other_validator_pk.clone(),
        validator: Some(GenesisValidator::new(
            Motes::new(1_000_000_000_000_000_u64),
            DelegationRate::zero(),
        )),
        balance: Motes::new(10_000_000_000_000_000_u64),
    });
    builder.run_genesis(genesis_request);

    let auction_delay = builder.get_unbonding_delay();
    let unbond_delay = builder.get_unbonding_delay();

    for _ in 0..=auction_delay {
        // crank era
        builder.run_auction(timestamp_millis, vec![]);
    }

    let account_main_purse = builder
        .get_entity_with_named_keys_by_account_hash(account)
        .expect("should have account")
        .main_purse();
    let starting_account_balance = builder.get_purse_balance(account_main_purse);

    builder
        .exec(
            ExecuteRequestBuilder::standard(
                account,
                STORED_STAKING_CONTRACT_NAME,
                runtime_args! {
                    ARG_AMOUNT => seed_amount
                },
            )
            .build(),
        )
        .commit()
        .expect_success();

    let default_account = builder
        .get_entity_with_named_keys_by_account_hash(account)
        .expect("should have account");
    let named_keys = default_account.named_keys();

    let contract_key = named_keys
        .get(&contract_name)
        .expect("contract_name key should exist");

    let contract = builder
        .get_entity_with_named_keys_by_entity_hash(AddressableEntityHash::new(
            contract_key.into_hash_addr().expect("must be hash addr"),
        ))
        .expect("must have contract");
    let contract_named_keys = contract.named_keys();

    let contract_purse = contract_named_keys
        .get(&purse_name)
        .expect("purse_name key should exist")
        .into_uref()
        .expect("should be a uref");

    let post_install_account_balance = builder.get_purse_balance(account_main_purse);
    assert_eq!(
        post_install_account_balance,
        starting_account_balance.saturating_sub(seed_amount),
        "post install should be reduced due to seeding contract purse"
    );

    let pre_delegation_balance = builder.get_purse_balance(contract_purse);
    assert_eq!(pre_delegation_balance, seed_amount);

    // check delegated amount from contract
    builder
        .exec(
            ExecuteRequestBuilder::contract_call_by_name(
                account,
                &contract_name,
                &entry_point_name,
                runtime_args! {
                    ARG_ACTION => get_staked_amount.clone(),
                    ARG_VALIDATOR => validator_pk.clone(),
                },
            )
            .build(),
        )
        .commit()
        .expect_success();

    let result = builder.get_last_exec_result().unwrap();
    let staked_amount: U512 = result.ret().unwrap().to_owned().into_t().unwrap();
    assert_eq!(
        staked_amount,
        U512::zero(),
        "staked amount should be zero prior to staking"
    );

    // stake from contract
    builder
        .exec(
            ExecuteRequestBuilder::contract_call_by_name(
                account,
                &contract_name,
                &entry_point_name,
                runtime_args! {
                    ARG_ACTION => stake,
                    ARG_AMOUNT => delegate_amount,
                    ARG_VALIDATOR => validator_pk.clone(),
                },
            )
            .build(),
        )
        .commit()
        .expect_success();

    let post_delegation_balance = builder.get_purse_balance(contract_purse);
    assert_eq!(
        post_delegation_balance,
        pre_delegation_balance.saturating_sub(delegate_amount),
        "contract purse balance should be reduced by staked amount"
    );

    let delegation_key = Key::BidAddr(BidAddr::DelegatedPurse {
        validator: validator_pk.to_account_hash(),
        delegator: contract_purse.addr(),
    });

    let stored_value = builder
        .query(None, delegation_key, &[])
        .expect("should have delegation bid");

    assert!(
        matches!(stored_value, StoredValue::BidKind(BidKind::Delegator(_))),
        "expected delegator bid"
    );

    if let StoredValue::BidKind(BidKind::Delegator(delegator)) = stored_value {
        assert_eq!(
            delegator.staked_amount(),
            delegate_amount,
            "staked amount should match delegation amount"
        );
    }

    // check delegated amount from contract
    builder
        .exec(
            ExecuteRequestBuilder::contract_call_by_name(
                account,
                &contract_name,
                &entry_point_name,
                runtime_args! {
                    ARG_ACTION => get_staked_amount.clone(),
                    ARG_VALIDATOR => validator_pk.clone(),
                },
            )
            .build(),
        )
        .commit()
        .expect_success();

    let result = builder.get_last_exec_result().unwrap();
    let staked_amount: U512 = result.ret().unwrap().to_owned().into_t().unwrap();
    assert_eq!(
        staked_amount, delegate_amount,
        "staked amount should match delegation amount"
    );

    for _ in 0..=auction_delay {
        // crank era
        builder.run_auction(timestamp_millis, vec![]);
    }

    let increased_delegate_amount = if let StoredValue::BidKind(BidKind::Delegator(delegator)) =
        builder
            .query(None, delegation_key, &[])
            .expect("should have delegation bid")
    {
        delegator.staked_amount()
    } else {
        U512::zero()
    };

    // restake from contract
    builder
        .exec(
            ExecuteRequestBuilder::contract_call_by_name(
                account,
                &contract_name,
                &entry_point_name,
                runtime_args! {
                    ARG_ACTION => restake,
                    ARG_AMOUNT => increased_delegate_amount,
                    ARG_VALIDATOR => validator_pk.clone(),
                    ARG_NEW_VALIDATOR => other_validator_pk.clone()
                },
            )
            .build(),
        )
        .commit()
        .expect_success();

    assert!(
        builder.query(None, delegation_key, &[]).is_err(),
        "delegation record should be removed"
    );

    assert_eq!(
        post_delegation_balance,
        builder.get_purse_balance(contract_purse),
        "at this point, unstaked token has not been returned"
    );

    for _ in 0..=unbond_delay {
        // crank era
        builder.run_auction(timestamp_millis, vec![]);
    }

    let delegation_key = Key::BidAddr(BidAddr::DelegatedPurse {
        validator: other_validator_pk.to_account_hash(),
        delegator: contract_purse.addr(),
    });

    let stored_value = builder
        .query(None, delegation_key, &[])
        .expect("should have delegation bid");

    assert!(
        matches!(stored_value, StoredValue::BidKind(BidKind::Delegator(_))),
        "expected delegator bid"
    );

    if let StoredValue::BidKind(BidKind::Delegator(delegator)) = stored_value {
        assert_eq!(
            delegator.staked_amount(),
            delegate_amount,
            "staked amount should match delegation amount"
        );
    }

    // unstake from contract
    builder
        .exec(
            ExecuteRequestBuilder::contract_call_by_name(
                account,
                &contract_name,
                &entry_point_name,
                runtime_args! {
                    ARG_ACTION => unstake,
                    ARG_AMOUNT => increased_delegate_amount,
                    ARG_VALIDATOR => other_validator_pk.clone(),
                },
            )
            .build(),
        )
        .commit()
        .expect_success();

    assert!(
        builder.query(None, delegation_key, &[]).is_err(),
        "delegation record should be removed"
    );

    assert_eq!(
        post_delegation_balance,
        builder.get_purse_balance(contract_purse),
        "at this point, unstaked token has not been returned"
    );

    let unbond_key = Key::BidAddr(BidAddr::UnbondPurse {
        validator: other_validator_pk.to_account_hash(),
        unbonder: contract_purse.addr(),
    });
    let unbonded_amount = if let StoredValue::BidKind(BidKind::Unbond(unbond)) = builder
        .query(None, unbond_key, &[])
        .expect("should have unbond")
    {
        let unbond_era = unbond.eras().first().expect("should have an era entry");
        assert_eq!(
            *unbond_era.amount(),
            increased_delegate_amount,
            "unbonded amount should match expectations"
        );
        *unbond_era.amount()
    } else {
        U512::zero()
    };

    for _ in 0..=unbond_delay {
        // crank era
        builder.run_auction(timestamp_millis, vec![]);
    }

    assert_eq!(
        delegate_amount.saturating_add(unbonded_amount),
        builder.get_purse_balance(contract_purse),
        "unbonded amount should be available to contract staking purse"
    );
}

#[ignore]
#[test]
fn should_not_enforce_max_spending_when_main_purse_not_in_use() {
    const ARG_ACTION: &str = "action";
    let timestamp_millis = DEFAULT_GENESIS_TIMESTAMP_MILLIS;
    let purse_name = "staking_purse".to_string();
    let contract_name = "staking".to_string();
    let entry_point_name = "run".to_string();
    let stake_all = "STAKE_ALL".to_string();
    let account = *DEFAULT_ACCOUNT_ADDR;
    let seed_amount = U512::from(10_000_000_000_000_000_u64);
    let validator_pk = &*DEFAULT_PROPOSER_PUBLIC_KEY;
    let other_validator_pk = {
        let secret_key = SecretKey::ed25519_from_bytes([199; SecretKey::ED25519_LENGTH]).unwrap();
        PublicKey::from(&secret_key)
    };

    let mut builder = LmdbWasmTestBuilder::default();
    let mut genesis_request = LOCAL_GENESIS_REQUEST.clone();
    genesis_request.set_enable_entity(true);

    genesis_request.push_genesis_validator(
        validator_pk,
        GenesisValidator::new(
            Motes::new(10_000_000_000_000_000_u64),
            DelegationRate::zero(),
        ),
    );
    genesis_request.push_genesis_account(GenesisAccount::Account {
        public_key: other_validator_pk.clone(),
        validator: Some(GenesisValidator::new(
            Motes::new(1_000_000_000_000_000_u64),
            DelegationRate::zero(),
        )),
        balance: Motes::new(10_000_000_000_000_000_u64),
    });
    builder.run_genesis(genesis_request);

    let auction_delay = builder.get_unbonding_delay();

    for _ in 0..=auction_delay {
        // crank era
        builder.run_auction(timestamp_millis, vec![]);
    }

    let account_main_purse = builder
        .get_entity_with_named_keys_by_account_hash(account)
        .expect("should have account")
        .main_purse();
    let starting_account_balance = builder.get_purse_balance(account_main_purse);

    builder
        .exec(
            ExecuteRequestBuilder::standard(
                account,
                STORED_STAKING_CONTRACT_NAME,
                runtime_args! {
                    ARG_AMOUNT => seed_amount
                },
            )
            .build(),
        )
        .commit()
        .expect_success();

    let default_account = builder
        .get_entity_with_named_keys_by_account_hash(account)
        .expect("should have account");
    let named_keys = default_account.named_keys();

    let contract_key = named_keys
        .get(&contract_name)
        .expect("contract_name key should exist");

    let contract = builder
        .get_entity_with_named_keys_by_entity_hash(AddressableEntityHash::new(
            contract_key.into_hash_addr().expect("must be hash addr"),
        ))
        .expect("must have contract");

    let contract_named_keys = contract.named_keys();

    let contract_purse = contract_named_keys
        .get(&purse_name)
        .expect("purse_name key should exist")
        .into_uref()
        .expect("should be a uref");

    let post_install_account_balance = builder.get_purse_balance(account_main_purse);
    assert_eq!(
        post_install_account_balance,
        starting_account_balance.saturating_sub(seed_amount),
        "post install should be reduced due to seeding contract purse"
    );

    let pre_delegation_balance = builder.get_purse_balance(contract_purse);
    assert_eq!(pre_delegation_balance, seed_amount);

    // stake from contract
    builder
        .exec(
            ExecuteRequestBuilder::contract_call_by_name(
                account,
                &contract_name,
                &entry_point_name,
                runtime_args! {
                    ARG_ACTION => stake_all,
                    ARG_VALIDATOR => validator_pk.clone(),
                },
            )
            .build(),
        )
        .commit()
        .expect_success();

    let post_delegation_balance = builder.get_purse_balance(contract_purse);
    assert_eq!(
        post_delegation_balance,
        U512::zero(),
        "contract purse balance should be reduced by staked amount"
    );

    let delegation_key = Key::BidAddr(BidAddr::DelegatedPurse {
        validator: validator_pk.to_account_hash(),
        delegator: contract_purse.addr(),
    });

    let stored_value = builder
        .query(None, delegation_key, &[])
        .expect("should have delegation bid");

    assert!(
        matches!(stored_value, StoredValue::BidKind(BidKind::Delegator(_))),
        "expected delegator bid"
    );

    if let StoredValue::BidKind(BidKind::Delegator(delegator)) = stored_value {
        assert_eq!(
            delegator.staked_amount(),
            pre_delegation_balance,
            "staked amount should match delegation amount"
        );
    }

    for _ in 0..=auction_delay {
        // crank era
        builder.run_auction(timestamp_millis, vec![]);
    }

    builder
        .query(None, delegation_key, &[])
        .expect("should have delegation bid");
}

#[ignore]
#[test]
fn should_read_bid_with_vesting_schedule_populated() {
    const ARG_ACTION: &str = "action";
    let purse_name = "staking_purse".to_string();
    let contract_name = "staking".to_string();
    let entry_point_name = "run".to_string();
    let get_staked_amount = "STAKED_AMOUNT".to_string();
    let account = *DEFAULT_ACCOUNT_ADDR;
    let seed_amount = U512::from(10_000_000_000_000_000_u64);
    let validator_pk = &*DEFAULT_PROPOSER_PUBLIC_KEY;

    let mut builder = LmdbWasmTestBuilder::default();
    let mut genesis_request = LOCAL_GENESIS_REQUEST.clone();
    genesis_request.set_enable_entity(true);
    genesis_request.push_genesis_validator(
        validator_pk,
        GenesisValidator::new(
            Motes::new(10_000_000_000_000_000_u64),
            DelegationRate::zero(),
        ),
    );
    builder.run_genesis(genesis_request);

    builder
        .exec(
            ExecuteRequestBuilder::standard(
                account,
                STORED_STAKING_CONTRACT_NAME,
                runtime_args! {
                    ARG_AMOUNT => seed_amount
                },
            )
            .build(),
        )
        .commit()
        .expect_success();

    let default_account = builder
        .get_entity_with_named_keys_by_account_hash(account)
        .expect("should have account");
    let named_keys = default_account.named_keys();

    let contract_key = named_keys
        .get(&contract_name)
        .expect("contract_name key should exist");

    let contract = builder
        .get_entity_with_named_keys_by_entity_hash(AddressableEntityHash::new(
            contract_key.into_hash_addr().expect("must be hash addr"),
        ))
        .expect("must have contract");

    let contract_named_keys = contract.named_keys();

    let contract_purse = contract_named_keys
        .get(&purse_name)
        .expect("purse_name key should exist")
        .into_uref()
        .expect("should be a uref");

    // Create a mock bid with a vesting schedule initialized.
    // This is only there to make sure size constraints are not a problem
    // when trying to read this relatively large structure as a guest.
    let mut mock_bid = DelegatorBid::locked(
        DelegatorKind::Purse(contract_purse.addr()),
        U512::from(100_000_000),
        contract_purse,
        validator_pk.clone(),
        0,
    );

    mock_bid
        .vesting_schedule_mut()
        .unwrap()
        .initialize_with_schedule(U512::from(100_000_000), 0);

    let delegation_key = Key::BidAddr(BidAddr::DelegatedPurse {
        validator: validator_pk.to_account_hash(),
        delegator: contract_purse.addr(),
    });

    builder.write_data_and_commit(
        [(
            delegation_key,
            StoredValue::BidKind(BidKind::Delegator(Box::new(mock_bid))),
        )]
        .iter()
        .cloned(),
    );

    builder
        .query(None, delegation_key, &[])
        .expect("should have delegation bid")
        .as_bid_kind()
        .expect("should be bidkind")
        .vesting_schedule()
        .expect("should have vesting schedule")
        .locked_amounts()
        .expect("should have locked amounts");

    builder
        .exec(
            ExecuteRequestBuilder::contract_call_by_name(
                account,
                &contract_name,
                &entry_point_name,
                runtime_args! {
                    ARG_ACTION => get_staked_amount.clone(),
                    ARG_VALIDATOR => validator_pk.clone(),
                },
            )
            .build(),
        )
        .commit()
        .expect_success();
}
