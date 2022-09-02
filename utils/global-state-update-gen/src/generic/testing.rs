use std::collections::BTreeMap;

use rand::Rng;

use casper_types::{
    account::{Account, AccountHash},
    system::auction::{
        Bid, Bids, Delegator, SeigniorageRecipient, SeigniorageRecipients,
        SeigniorageRecipientsSnapshot, UnbondingPurses,
    },
    testing::TestRng,
    AccessRights, CLValue, Key, PublicKey, StoredValue, URef, URefAddr, U512,
};

use super::{
    config::{AccountConfig, Config, DelegatorConfig, Transfer, ValidatorConfig},
    get_update,
    state_reader::StateReader,
};

const TOTAL_SUPPLY_KEY: URef = URef::new([1; 32], AccessRights::READ_ADD_WRITE);
const SEIGNIORAGE_RECIPIENTS_KEY: URef = URef::new([2; 32], AccessRights::READ_ADD_WRITE);

struct MockStateReader {
    accounts: BTreeMap<AccountHash, Account>,
    purses: BTreeMap<URefAddr, U512>,
    total_supply: U512,
    seigniorage_recipients: SeigniorageRecipientsSnapshot,
    bids: Bids,
}

impl MockStateReader {
    fn new() -> Self {
        Self {
            accounts: BTreeMap::new(),
            purses: BTreeMap::new(),
            total_supply: U512::zero(),
            seigniorage_recipients: SeigniorageRecipientsSnapshot::new(),
            bids: Bids::new(),
        }
    }

    fn with_account<R: Rng>(
        mut self,
        account_hash: AccountHash,
        balance: U512,
        rng: &mut R,
    ) -> Self {
        let main_purse = URef::new(rng.gen(), AccessRights::READ_ADD_WRITE);
        let account = Account::create(account_hash, Default::default(), main_purse);
        self.purses.insert(main_purse.addr(), balance);
        self.accounts.insert(account_hash, account);
        self.total_supply += balance;
        self
    }

    fn with_validators<R: Rng>(
        mut self,
        validators: Vec<(PublicKey, U512, ValidatorConfig)>,
        rng: &mut R,
    ) -> Self {
        let mut recipients = SeigniorageRecipients::new();
        for (public_key, balance, validator_cfg) in validators {
            let stake = validator_cfg.bonded_amount;
            let delegation_rate = validator_cfg.delegation_rate.unwrap_or_default();
            let delegators = validator_cfg.delegators_map().unwrap_or_default();
            // add an entry to the recipients snapshot
            let recipient = SeigniorageRecipient::new(stake, delegation_rate, delegators.clone());
            recipients.insert(public_key.clone(), recipient);

            // create the account if it doesn't exist
            let account_hash = public_key.to_account_hash();
            if !self.accounts.contains_key(&account_hash) {
                self = self.with_account(account_hash, balance, rng);
            }

            let bonding_purse = URef::new(rng.gen(), AccessRights::READ_ADD_WRITE);
            self.purses.insert(bonding_purse.addr(), stake);
            self.total_supply += stake;

            for delegator_pub_key in delegators.keys() {
                let account_hash = delegator_pub_key.to_account_hash();
                if !self.accounts.contains_key(&account_hash) {
                    self = self.with_account(account_hash, U512::zero(), rng);
                }
            }

            // create the bid
            let mut bid = Bid::unlocked(public_key.clone(), bonding_purse, stake, delegation_rate);

            for (delegator_pub_key, delegator_stake) in &delegators {
                let bonding_purse = URef::new(rng.gen(), AccessRights::READ_ADD_WRITE);
                self.purses.insert(bonding_purse.addr(), *delegator_stake);
                self.total_supply += *delegator_stake;

                let delegator = Delegator::unlocked(
                    delegator_pub_key.clone(),
                    *delegator_stake,
                    bonding_purse,
                    public_key.clone(),
                );
                bid.delegators_mut()
                    .insert(delegator_pub_key.clone(), delegator);
            }

            self.bids.insert(public_key, bid);
        }

        for era_id in 0..5 {
            self.seigniorage_recipients
                .insert(era_id.into(), recipients.clone());
        }

        self
    }
}

impl StateReader for MockStateReader {
    fn query(&mut self, key: Key) -> Option<StoredValue> {
        match key {
            Key::URef(uref) if uref == TOTAL_SUPPLY_KEY => Some(StoredValue::from(
                CLValue::from_t(self.total_supply).expect("should convert to CLValue"),
            )),
            Key::URef(uref) if uref == SEIGNIORAGE_RECIPIENTS_KEY => Some(StoredValue::from(
                CLValue::from_t(self.seigniorage_recipients.clone())
                    .expect("should convert seigniorage recipients to CLValue"),
            )),
            Key::Account(acc_hash) => self
                .accounts
                .get(&acc_hash)
                .map(|account| StoredValue::from(account.clone())),
            Key::Balance(purse_addr) => self.purses.get(&purse_addr).map(|balance| {
                StoredValue::from(CLValue::from_t(*balance).expect("should convert to CLValue"))
            }),
            _ => None,
        }
    }

    fn get_total_supply_key(&mut self) -> Key {
        Key::URef(TOTAL_SUPPLY_KEY)
    }

    fn get_seigniorage_recipients_key(&mut self) -> Key {
        Key::URef(SEIGNIORAGE_RECIPIENTS_KEY)
    }

    fn get_account(&mut self, account_hash: AccountHash) -> Option<Account> {
        self.accounts.get(&account_hash).cloned()
    }

    fn get_bids(&mut self) -> Bids {
        self.bids.clone()
    }

    fn get_unbonds(&mut self) -> UnbondingPurses {
        UnbondingPurses::new()
    }
}

#[test]
fn should_transfer_funds() {
    let mut rng = TestRng::new();

    let account1: AccountHash = rng.gen();
    let account2: AccountHash = rng.gen();

    let mut reader = MockStateReader::new()
        .with_account(account1, U512::from(1_000_000_000), &mut rng)
        .with_account(account2, U512::zero(), &mut rng);

    let config = Config {
        transfers: vec![Transfer {
            from: account1,
            to: account2,
            amount: U512::from(300_000_000),
        }],
        ..Default::default()
    };

    let result = get_update(&mut reader, config);

    let account1 = reader.get_account(account1).unwrap();
    let account2 = reader.get_account(account2).unwrap();

    // should write decreased balance to the first purse
    assert_eq!(
        result.get(&Key::Balance(account1.main_purse().addr())),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(700_000_000)).expect("should convert U512 to CLValue")
        )),
    );

    // should write increased balance to the second purse
    assert_eq!(
        result.get(&Key::Balance(account2.main_purse().addr())),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(300_000_000)).expect("should convert U512 to CLValue")
        )),
    );

    // total supply is written on every purse balance change, so we'll have a write to this key
    // even though the changes cancel each other out
    assert_eq!(
        result.get(&reader.get_total_supply_key()),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(1_000_000_000)).expect("should convert U512 to CLValue")
        ))
    );

    // 3 keys tested above should be all that would be written
    assert_eq!(result.len(), 3);
}

#[test]
fn should_create_account_when_transferring_funds() {
    let mut rng = TestRng::new();

    let account1: AccountHash = rng.gen();
    let account2: AccountHash = rng.gen();

    let mut reader =
        MockStateReader::new().with_account(account1, U512::from(1_000_000_000), &mut rng);

    let config = Config {
        transfers: vec![Transfer {
            from: account1,
            to: account2,
            amount: U512::from(300_000_000),
        }],
        ..Default::default()
    };

    let result = get_update(&mut reader, config);

    let account1 = reader.get_account(account1).unwrap();
    assert!(reader.get_account(account2).is_none());

    // should write decreased balance to the first purse
    assert_eq!(
        result.get(&Key::Balance(account1.main_purse().addr())),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(700_000_000)).expect("should convert U512 to CLValue")
        )),
    );

    // the new account should be created
    let account_write = result
        .get(&Key::Account(account2))
        .expect("should create account")
        .as_account()
        .expect("should be account")
        .clone();
    let new_purse = account_write.main_purse();

    // check that the main purse for the new account has been created with the correct amount
    assert_eq!(
        result.get(&Key::URef(new_purse)),
        Some(&StoredValue::from(
            CLValue::from_t(()).expect("should convert unit to CLValue")
        ))
    );
    assert_eq!(
        result.get(&Key::Balance(new_purse.addr())),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(300_000_000)).expect("should convert U512 to CLValue")
        ))
    );

    // total supply is written on every purse balance change, so we'll have a write to this key
    // even though the changes cancel each other out
    assert_eq!(
        result.get(&reader.get_total_supply_key()),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(1_000_000_000)).expect("should convert U512 to CLValue")
        ))
    );

    // 5 keys tested above should be all that would be written
    assert_eq!(result.len(), 5);
}

#[test]
fn should_change_one_validator() {
    let mut rng = TestRng::new();

    let validator1 = PublicKey::random(&mut rng);
    let validator2 = PublicKey::random(&mut rng);
    let validator3 = PublicKey::random(&mut rng);

    let mut reader = MockStateReader::new().with_validators(
        vec![
            (
                validator1,
                U512::from(101),
                ValidatorConfig {
                    bonded_amount: U512::from(101),
                    ..Default::default()
                },
            ),
            (
                validator2,
                U512::from(102),
                ValidatorConfig {
                    bonded_amount: U512::from(102),
                    ..Default::default()
                },
            ),
            (
                validator3.clone(),
                U512::from(103),
                ValidatorConfig {
                    bonded_amount: U512::from(103),
                    ..Default::default()
                },
            ),
        ],
        &mut rng,
    );

    // we'll be updating only the stake and balance of validator 3
    let config = Config {
        accounts: vec![AccountConfig {
            public_key: validator3.clone(),
            balance: Some(U512::from(100)),
            validator: Some(ValidatorConfig {
                bonded_amount: U512::from(104),
                delegation_rate: None,
                delegators: None,
            }),
        }],
        ..Default::default()
    };

    let result = get_update(&mut reader, config);

    assert!(result.contains_key(&reader.get_seigniorage_recipients_key()));
    assert_eq!(
        result.get(&reader.get_total_supply_key()),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(610)).expect("should convert U512 to CLValue")
        ))
    );

    // check purse writes
    let account3 = validator3.to_account_hash();
    let bid_purse = *reader
        .get_bids()
        .get(&validator3)
        .expect("should have bid")
        .bonding_purse();
    let main_purse = reader
        .get_account(account3)
        .expect("should have account")
        .main_purse();

    assert_eq!(
        result.get(&Key::Balance(bid_purse.addr())),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(104)).expect("should convert U512 to CLValue")
        ))
    );
    assert_eq!(
        result.get(&Key::Balance(main_purse.addr())),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(100)).expect("should convert U512 to CLValue")
        ))
    );

    // check bid overwrite
    let expected_bid = Bid::unlocked(validator3, bid_purse, U512::from(104), Default::default());
    assert_eq!(
        result.get(&Key::Bid(account3)),
        Some(&StoredValue::from(expected_bid))
    );

    // 5 keys above should be all that was overwritten
    assert_eq!(result.len(), 5);
}

#[test]
fn should_change_only_stake_of_one_validator() {
    let mut rng = TestRng::new();

    let validator1 = PublicKey::random(&mut rng);
    let validator2 = PublicKey::random(&mut rng);
    let validator3 = PublicKey::random(&mut rng);

    let mut reader = MockStateReader::new().with_validators(
        vec![
            (
                validator1,
                U512::from(101),
                ValidatorConfig {
                    bonded_amount: U512::from(101),
                    ..Default::default()
                },
            ),
            (
                validator2,
                U512::from(102),
                ValidatorConfig {
                    bonded_amount: U512::from(102),
                    ..Default::default()
                },
            ),
            (
                validator3.clone(),
                U512::from(103),
                ValidatorConfig {
                    bonded_amount: U512::from(103),
                    ..Default::default()
                },
            ),
        ],
        &mut rng,
    );

    // we'll be updating only the stake of validator 3
    let config = Config {
        accounts: vec![AccountConfig {
            public_key: validator3.clone(),
            balance: None,
            validator: Some(ValidatorConfig {
                bonded_amount: U512::from(104),
                delegation_rate: None,
                delegators: None,
            }),
        }],
        ..Default::default()
    };

    let result = get_update(&mut reader, config);

    assert!(result.contains_key(&reader.get_seigniorage_recipients_key()));
    assert_eq!(
        result.get(&reader.get_total_supply_key()),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(613)).expect("should convert U512 to CLValue")
        ))
    );

    // check purse writes
    let account3 = validator3.to_account_hash();
    let bid_purse = *reader
        .get_bids()
        .get(&validator3)
        .expect("should have bid")
        .bonding_purse();

    assert_eq!(
        result.get(&Key::Balance(bid_purse.addr())),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(104)).expect("should convert U512 to CLValue")
        ))
    );

    // check bid overwrite
    let expected_bid = Bid::unlocked(validator3, bid_purse, U512::from(104), Default::default());
    assert_eq!(
        result.get(&Key::Bid(account3)),
        Some(&StoredValue::from(expected_bid))
    );

    // 4 keys above should be all that was overwritten
    assert_eq!(result.len(), 4);
}

#[test]
fn should_change_only_balance_of_one_validator() {
    let mut rng = TestRng::new();

    let validator1 = PublicKey::random(&mut rng);
    let validator2 = PublicKey::random(&mut rng);
    let validator3 = PublicKey::random(&mut rng);

    let mut reader = MockStateReader::new().with_validators(
        vec![
            (
                validator1,
                U512::from(101),
                ValidatorConfig {
                    bonded_amount: U512::from(101),
                    ..Default::default()
                },
            ),
            (
                validator2,
                U512::from(102),
                ValidatorConfig {
                    bonded_amount: U512::from(102),
                    ..Default::default()
                },
            ),
            (
                validator3.clone(),
                U512::from(103),
                ValidatorConfig {
                    bonded_amount: U512::from(103),
                    ..Default::default()
                },
            ),
        ],
        &mut rng,
    );

    // we'll be updating only the balance of validator 3
    let config = Config {
        accounts: vec![AccountConfig {
            public_key: validator3.clone(),
            balance: Some(U512::from(100)),
            validator: None,
        }],
        ..Default::default()
    };

    let result = get_update(&mut reader, config);

    assert_eq!(
        result.get(&reader.get_total_supply_key()),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(609)).expect("should convert U512 to CLValue")
        ))
    );

    // check purse writes
    let account3 = validator3.to_account_hash();
    let main_purse = reader
        .get_account(account3)
        .expect("should have account")
        .main_purse();

    assert_eq!(
        result.get(&Key::Balance(main_purse.addr())),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(100)).expect("should convert U512 to CLValue")
        ))
    );

    // 2 keys above should be all that was overwritten
    assert_eq!(result.len(), 2);
}

#[test]
fn should_replace_one_validator() {
    let mut rng = TestRng::new();

    let validator1 = PublicKey::random(&mut rng);
    let validator2 = PublicKey::random(&mut rng);

    let mut reader = MockStateReader::new().with_validators(
        vec![(
            validator1.clone(),
            U512::from(101),
            ValidatorConfig {
                bonded_amount: U512::from(101),
                ..Default::default()
            },
        )],
        &mut rng,
    );

    // we'll be updating the validators set to only contain validator2
    let config = Config {
        accounts: vec![AccountConfig {
            public_key: validator2.clone(),
            balance: Some(U512::from(102)),
            validator: Some(ValidatorConfig {
                bonded_amount: U512::from(102),
                delegation_rate: None,
                delegators: None,
            }),
        }],
        only_listed_validators: true,
        ..Default::default()
    };

    let result = get_update(&mut reader, config);

    assert!(result.contains_key(&reader.get_seigniorage_recipients_key()));
    assert_eq!(
        result.get(&reader.get_total_supply_key()),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(305)).expect("should convert U512 to CLValue")
        ))
    );

    // check purse write for validator1
    let bid_purse = *reader
        .get_bids()
        .get(&validator1)
        .expect("should have bid")
        .bonding_purse();

    assert_eq!(
        result.get(&Key::Balance(bid_purse.addr())),
        Some(&StoredValue::from(
            CLValue::from_t(U512::zero()).expect("should convert U512 to CLValue")
        ))
    );

    // check bid overwrite
    let account1 = validator1.to_account_hash();
    let mut expected_bid_1 = Bid::unlocked(validator1, bid_purse, U512::zero(), Default::default());
    expected_bid_1.deactivate();
    assert_eq!(
        result.get(&Key::Bid(account1)),
        Some(&StoredValue::from(expected_bid_1))
    );

    // check writes for validator2
    let account2 = validator2.to_account_hash();

    // the new account should be created
    let account_write = result
        .get(&Key::Account(account2))
        .expect("should create account")
        .as_account()
        .expect("should be account")
        .clone();
    let main_purse_2 = account_write.main_purse();

    // check that the main purse for the new account has been created with the correct amount
    assert_eq!(
        result.get(&Key::URef(main_purse_2)),
        Some(&StoredValue::from(
            CLValue::from_t(()).expect("should convert unit to CLValue")
        ))
    );
    assert_eq!(
        result.get(&Key::Balance(main_purse_2.addr())),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(102)).expect("should convert U512 to CLValue")
        ))
    );

    let bid_write = result
        .get(&Key::Bid(account2))
        .expect("should create bid")
        .as_bid()
        .expect("should be bid")
        .clone();
    assert_eq!(bid_write.validator_public_key(), &validator2);
    assert_eq!(
        bid_write
            .total_staked_amount()
            .expect("should read total staked amount"),
        U512::from(102)
    );
    assert!(!bid_write.inactive());

    let bid_purse_2 = *bid_write.bonding_purse();

    // check that the bid purse for the new validator has been created with the correct amount
    assert_eq!(
        result.get(&Key::URef(bid_purse_2)),
        Some(&StoredValue::from(
            CLValue::from_t(()).expect("should convert unit to CLValue")
        ))
    );
    assert_eq!(
        result.get(&Key::Balance(bid_purse_2.addr())),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(102)).expect("should convert U512 to CLValue")
        ))
    );

    // 10 keys above should be all that was overwritten
    assert_eq!(result.len(), 10);
}

#[test]
fn should_add_one_validator() {
    let mut rng = TestRng::new();

    let validator1 = PublicKey::random(&mut rng);
    let validator2 = PublicKey::random(&mut rng);
    let validator3 = PublicKey::random(&mut rng);
    let validator4 = PublicKey::random(&mut rng);

    let mut reader = MockStateReader::new().with_validators(
        vec![
            (
                validator1,
                U512::from(101),
                ValidatorConfig {
                    bonded_amount: U512::from(101),
                    ..Default::default()
                },
            ),
            (
                validator2,
                U512::from(102),
                ValidatorConfig {
                    bonded_amount: U512::from(102),
                    ..Default::default()
                },
            ),
            (
                validator3,
                U512::from(103),
                ValidatorConfig {
                    bonded_amount: U512::from(103),
                    ..Default::default()
                },
            ),
        ],
        &mut rng,
    );

    // we'll be adding validator 4
    let config = Config {
        accounts: vec![AccountConfig {
            public_key: validator4.clone(),
            balance: Some(U512::from(100)),
            validator: Some(ValidatorConfig {
                bonded_amount: U512::from(104),
                delegation_rate: None,
                delegators: None,
            }),
        }],
        only_listed_validators: false,
        ..Default::default()
    };

    let result = get_update(&mut reader, config);

    assert!(result.contains_key(&reader.get_seigniorage_recipients_key()));
    assert_eq!(
        result.get(&reader.get_total_supply_key()),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(816)).expect("should convert U512 to CLValue")
        ))
    );

    // check writes for validator4
    let account4 = validator4.to_account_hash();

    // the new account should be created
    let account_write = result
        .get(&Key::Account(account4))
        .expect("should create account")
        .as_account()
        .expect("should be account")
        .clone();
    let main_purse_4 = account_write.main_purse();

    // check that the main purse for the new account has been created with the correct amount
    assert_eq!(
        result.get(&Key::URef(main_purse_4)),
        Some(&StoredValue::from(
            CLValue::from_t(()).expect("should convert unit to CLValue")
        ))
    );
    assert_eq!(
        result.get(&Key::Balance(main_purse_4.addr())),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(100)).expect("should convert U512 to CLValue")
        ))
    );

    let bid_write = result
        .get(&Key::Bid(account4))
        .expect("should create bid")
        .as_bid()
        .expect("should be bid")
        .clone();
    assert_eq!(bid_write.validator_public_key(), &validator4);
    assert_eq!(
        bid_write
            .total_staked_amount()
            .expect("should read total staked amount"),
        U512::from(104)
    );
    assert!(!bid_write.inactive());

    let bid_purse_4 = *bid_write.bonding_purse();

    // check that the bid purse for the new validator has been created with the correct amount
    assert_eq!(
        result.get(&Key::URef(bid_purse_4)),
        Some(&StoredValue::from(
            CLValue::from_t(()).expect("should convert unit to CLValue")
        ))
    );
    assert_eq!(
        result.get(&Key::Balance(bid_purse_4.addr())),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(104)).expect("should convert U512 to CLValue")
        ))
    );

    // 8 keys above should be all that was written
    assert_eq!(result.len(), 8);
}

#[test]
fn should_add_one_validator_with_delegators() {
    let mut rng = TestRng::new();

    let validator1 = PublicKey::random(&mut rng);
    let validator2 = PublicKey::random(&mut rng);
    let delegator1 = PublicKey::random(&mut rng);

    let mut reader = MockStateReader::new().with_validators(
        vec![(
            validator1,
            U512::from(101),
            ValidatorConfig {
                bonded_amount: U512::from(101),
                ..Default::default()
            },
        )],
        &mut rng,
    );

    // we'll be adding validator 2
    let config = Config {
        accounts: vec![AccountConfig {
            public_key: validator2.clone(),
            balance: Some(U512::from(100)),
            validator: Some(ValidatorConfig {
                bonded_amount: U512::from(102),
                delegation_rate: Some(5),
                delegators: Some(vec![DelegatorConfig {
                    public_key: delegator1.clone(),
                    delegated_amount: U512::from(13),
                }]),
            }),
        }],
        only_listed_validators: false,
        ..Default::default()
    };

    let result = get_update(&mut reader, config);

    assert!(result.contains_key(&reader.get_seigniorage_recipients_key()));
    assert_eq!(
        result.get(&reader.get_total_supply_key()),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(417)).expect("should convert U512 to CLValue")
        ))
    );

    // check writes for validator2
    let account2 = validator2.to_account_hash();

    // the new account should be created
    let account_write = result
        .get(&Key::Account(account2))
        .expect("should create account")
        .as_account()
        .expect("should be account")
        .clone();
    let main_purse_2 = account_write.main_purse();

    // check that the main purse for the new account has been created with the correct amount
    assert_eq!(
        result.get(&Key::URef(main_purse_2)),
        Some(&StoredValue::from(
            CLValue::from_t(()).expect("should convert unit to CLValue")
        ))
    );
    assert_eq!(
        result.get(&Key::Balance(main_purse_2.addr())),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(100)).expect("should convert U512 to CLValue")
        ))
    );

    let bid_write = result
        .get(&Key::Bid(account2))
        .expect("should create bid")
        .as_bid()
        .expect("should be bid")
        .clone();
    assert_eq!(bid_write.validator_public_key(), &validator2);
    assert_eq!(*bid_write.staked_amount(), U512::from(102));
    assert_eq!(
        bid_write
            .total_staked_amount()
            .expect("should read total staked amount"),
        U512::from(115)
    );
    assert!(!bid_write.inactive());

    let bid_purse_2 = *bid_write.bonding_purse();

    // check that the bid purse for the new validator has been created with the correct amount
    assert_eq!(
        result.get(&Key::URef(bid_purse_2)),
        Some(&StoredValue::from(
            CLValue::from_t(()).expect("should convert unit to CLValue")
        ))
    );
    assert_eq!(
        result.get(&Key::Balance(bid_purse_2.addr())),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(102)).expect("should convert U512 to CLValue")
        ))
    );

    let bid_delegator_purse = *bid_write
        .delegators()
        .get(&delegator1)
        .expect("should have delegator")
        .bonding_purse();

    // check that the bid purse for the new delegator has been created with the correct amount
    assert_eq!(
        result.get(&Key::URef(bid_delegator_purse)),
        Some(&StoredValue::from(
            CLValue::from_t(()).expect("should convert unit to CLValue")
        ))
    );
    assert_eq!(
        result.get(&Key::Balance(bid_delegator_purse.addr())),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(13)).expect("should convert U512 to CLValue")
        ))
    );

    // 10 keys above should be all that was written
    assert_eq!(result.len(), 10);
}

#[test]
fn should_replace_a_delegator() {
    let mut rng = TestRng::new();

    let validator1 = PublicKey::random(&mut rng);
    let delegator1 = PublicKey::random(&mut rng);
    let delegator2 = PublicKey::random(&mut rng);

    let mut reader = MockStateReader::new().with_validators(
        vec![(
            validator1.clone(),
            U512::from(101),
            ValidatorConfig {
                bonded_amount: U512::from(101),
                delegation_rate: Some(5),
                delegators: Some(vec![DelegatorConfig {
                    public_key: delegator1.clone(),
                    delegated_amount: U512::from(13),
                }]),
            },
        )],
        &mut rng,
    );

    // we'll be replacing the delegator
    let config = Config {
        accounts: vec![AccountConfig {
            public_key: validator1.clone(),
            balance: Some(U512::from(101)),
            validator: Some(ValidatorConfig {
                bonded_amount: U512::from(101),
                delegation_rate: None,
                delegators: Some(vec![DelegatorConfig {
                    public_key: delegator2.clone(),
                    delegated_amount: U512::from(14),
                }]),
            }),
        }],
        only_listed_validators: false,
        ..Default::default()
    };

    let result = get_update(&mut reader, config);

    assert!(result.contains_key(&reader.get_seigniorage_recipients_key()));
    assert_eq!(
        result.get(&reader.get_total_supply_key()),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(216)).expect("should convert U512 to CLValue")
        ))
    );

    let account1 = validator1.to_account_hash();

    let bid_write = result
        .get(&Key::Bid(account1))
        .expect("should create bid")
        .as_bid()
        .expect("should be bid")
        .clone();
    assert_eq!(bid_write.validator_public_key(), &validator1);
    assert_eq!(*bid_write.staked_amount(), U512::from(101));
    assert_eq!(
        bid_write
            .total_staked_amount()
            .expect("should read total staked amount"),
        U512::from(115)
    );
    assert!(!bid_write.inactive());

    let delegator1_bid_purse = *reader
        .get_bids()
        .get(&validator1)
        .expect("should have old bid")
        .delegators()
        .get(&delegator1)
        .expect("should have old delegator")
        .bonding_purse();

    let delegator2_bid_purse = *bid_write
        .delegators()
        .get(&delegator2)
        .expect("should have new delegator")
        .bonding_purse();

    // check that the old delegator's bid purse got zeroed
    assert_eq!(
        result.get(&Key::Balance(delegator1_bid_purse.addr())),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(0)).expect("should convert U512 to CLValue")
        ))
    );

    // check that the bid purse for the new delegator has been created with the correct amount
    assert_eq!(
        result.get(&Key::URef(delegator2_bid_purse)),
        Some(&StoredValue::from(
            CLValue::from_t(()).expect("should convert unit to CLValue")
        ))
    );
    assert_eq!(
        result.get(&Key::Balance(delegator2_bid_purse.addr())),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(14)).expect("should convert U512 to CLValue")
        ))
    );

    // 6 keys above should be all that was written
    assert_eq!(result.len(), 6);
}

#[test]
fn should_not_change_the_delegator() {
    let mut rng = TestRng::new();

    let validator1 = PublicKey::random(&mut rng);
    let delegator1 = PublicKey::random(&mut rng);

    let mut reader = MockStateReader::new().with_validators(
        vec![(
            validator1.clone(),
            U512::from(101),
            ValidatorConfig {
                bonded_amount: U512::from(101),
                delegation_rate: Some(5),
                delegators: Some(vec![DelegatorConfig {
                    public_key: delegator1,
                    delegated_amount: U512::from(13),
                }]),
            },
        )],
        &mut rng,
    );

    // we'll be changing the validator's stake
    let config = Config {
        accounts: vec![AccountConfig {
            public_key: validator1.clone(),
            balance: Some(U512::from(101)),
            validator: Some(ValidatorConfig {
                bonded_amount: U512::from(111),
                delegation_rate: None,
                delegators: None,
            }),
        }],
        only_listed_validators: false,
        ..Default::default()
    };

    let result = get_update(&mut reader, config);

    assert!(result.contains_key(&reader.get_seigniorage_recipients_key()));
    assert_eq!(
        result.get(&reader.get_total_supply_key()),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(225)).expect("should convert U512 to CLValue")
        ))
    );

    let account1 = validator1.to_account_hash();

    let bid_write = result
        .get(&Key::Bid(account1))
        .expect("should create bid")
        .as_bid()
        .expect("should be bid")
        .clone();
    assert_eq!(bid_write.validator_public_key(), &validator1);
    assert_eq!(*bid_write.staked_amount(), U512::from(111));
    assert_eq!(
        bid_write
            .total_staked_amount()
            .expect("should read total staked amount"),
        U512::from(124)
    );
    assert!(!bid_write.inactive());

    let validator_bid_purse = *bid_write.bonding_purse();

    // check that the validator's bid purse got updated
    assert_eq!(
        result.get(&Key::Balance(validator_bid_purse.addr())),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(111)).expect("should convert U512 to CLValue")
        ))
    );

    // 4 keys above should be all that was written
    assert_eq!(result.len(), 4);
}

#[test]
fn should_remove_the_delegator() {
    let mut rng = TestRng::new();

    let validator1 = PublicKey::random(&mut rng);
    let delegator1 = PublicKey::random(&mut rng);

    let mut reader = MockStateReader::new().with_validators(
        vec![(
            validator1.clone(),
            U512::from(101),
            ValidatorConfig {
                bonded_amount: U512::from(101),
                delegation_rate: Some(5),
                delegators: Some(vec![DelegatorConfig {
                    public_key: delegator1.clone(),
                    delegated_amount: U512::from(13),
                }]),
            },
        )],
        &mut rng,
    );

    // we'll be removing the delegator
    let config = Config {
        accounts: vec![AccountConfig {
            public_key: validator1.clone(),
            balance: Some(U512::from(101)),
            validator: Some(ValidatorConfig {
                bonded_amount: U512::from(111),
                delegation_rate: None,
                delegators: Some(vec![]),
            }),
        }],
        only_listed_validators: false,
        ..Default::default()
    };

    let result = get_update(&mut reader, config);

    assert!(result.contains_key(&reader.get_seigniorage_recipients_key()));
    assert_eq!(
        result.get(&reader.get_total_supply_key()),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(212)).expect("should convert U512 to CLValue")
        ))
    );

    let account1 = validator1.to_account_hash();

    let bid_write = result
        .get(&Key::Bid(account1))
        .expect("should create bid")
        .as_bid()
        .expect("should be bid")
        .clone();
    assert_eq!(bid_write.validator_public_key(), &validator1);
    assert_eq!(*bid_write.staked_amount(), U512::from(111));
    assert_eq!(
        bid_write
            .total_staked_amount()
            .expect("should read total staked amount"),
        U512::from(111)
    );
    assert!(!bid_write.inactive());

    let validator_bid_purse = *bid_write.bonding_purse();

    // check that the validator's bid purse got updated
    assert_eq!(
        result.get(&Key::Balance(validator_bid_purse.addr())),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(111)).expect("should convert U512 to CLValue")
        ))
    );

    let delegator1_bid_purse = *reader
        .get_bids()
        .get(&validator1)
        .expect("should have old bid")
        .delegators()
        .get(&delegator1)
        .expect("should have old delegator")
        .bonding_purse();

    // check that the old delegator's bid purse got zeroed
    assert_eq!(
        result.get(&Key::Balance(delegator1_bid_purse.addr())),
        Some(&StoredValue::from(
            CLValue::from_t(U512::from(0)).expect("should convert U512 to CLValue")
        ))
    );

    // 5 keys above should be all that was written
    assert_eq!(result.len(), 5);
}
