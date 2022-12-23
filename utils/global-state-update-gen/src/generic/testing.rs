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
#[cfg(test)]
use crate::utils::ValidatorInfo;

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
        // If `insert` returns `Some()`, it means we used the same account hash twice, which is
        // a programmer error and the function will panic.
        assert!(self.accounts.insert(account_hash, account).is_none());
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

impl ValidatorInfo {
    pub fn new(public_key: &PublicKey, weight: U512) -> Self {
        ValidatorInfo {
            public_key: public_key.clone(),
            weight,
        }
    }
}

#[test]
fn should_transfer_funds() {
    let mut rng = TestRng::new();

    let public_key1 = PublicKey::random(&mut rng);
    let public_key2 = PublicKey::random(&mut rng);
    let account1 = public_key1.to_account_hash();
    let account2 = public_key2.to_account_hash();

    let mut reader = MockStateReader::new().with_validators(
        vec![
            (
                public_key1,
                U512::from(1_000_000_000),
                ValidatorConfig {
                    bonded_amount: U512::from(1),
                    ..Default::default()
                },
            ),
            (
                public_key2,
                U512::zero(),
                ValidatorConfig {
                    bonded_amount: U512::zero(),
                    ..Default::default()
                },
            ),
        ],
        &mut rng,
    );

    let config = Config {
        transfers: vec![Transfer {
            from: account1,
            to: account2,
            amount: U512::from(300_000_000),
        }],
        ..Default::default()
    };

    let update = get_update(&mut reader, config);

    // check that the update contains the correct list of validators
    update.assert_validators_unchanged();

    // should write decreased balance to the first purse
    let account1 = reader.get_account(account1).expect("should have account");
    update.assert_written_balance(account1.main_purse(), 700_000_000);

    // should write increased balance to the second purse
    let account2 = reader.get_account(account2).expect("should have account");
    update.assert_written_balance(account2.main_purse(), 300_000_000);

    // total supply is written on every purse balance change, so we'll have a write to this key
    // even though the changes cancel each other out
    update.assert_total_supply(&mut reader, 1_000_000_001);

    // 3 keys should be written:
    // - balance of account 1
    // - balance of account 2
    // - total supply
    assert_eq!(update.len(), 3);
}

#[test]
fn should_create_account_when_transferring_funds() {
    let mut rng = TestRng::new();

    let public_key1 = PublicKey::random(&mut rng);
    let public_key2 = PublicKey::random(&mut rng);
    let account1 = public_key1.to_account_hash();
    let account2 = public_key2.to_account_hash();

    let mut reader = MockStateReader::new().with_validators(
        vec![(
            public_key1,
            U512::from(1_000_000_000),
            ValidatorConfig {
                bonded_amount: U512::from(1),
                ..Default::default()
            },
        )],
        &mut rng,
    );

    let config = Config {
        transfers: vec![Transfer {
            from: account1,
            to: account2,
            amount: U512::from(300_000_000),
        }],
        ..Default::default()
    };

    let update = get_update(&mut reader, config);

    // check that the update contains the correct list of validators
    update.assert_validators_unchanged();

    let account1 = reader.get_account(account1).expect("should have account");
    // account2 shouldn't exist in the reader itself, only the update should be creating it
    assert!(reader.get_account(account2).is_none());
    let account2 = update.get_written_account(account2);

    // should write decreased balance to the first purse
    update.assert_written_balance(account1.main_purse(), 700_000_000);

    // check that the main purse for the new account has been created with the correct amount
    update.assert_written_balance(account2.main_purse(), 300_000_000);
    update.assert_written_purse_is_unit(account2.main_purse());

    // total supply is written on every purse balance change, so we'll have a write to this key
    // even though the changes cancel each other out
    update.assert_total_supply(&mut reader, 1_000_000_001);

    // 5 keys should be written:
    // - balance of account 1
    // - account 2
    // - main purse of account 2
    // - balance of account 2
    // - total supply
    assert_eq!(update.len(), 5);
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
                validator1.clone(),
                U512::from(101),
                ValidatorConfig {
                    bonded_amount: U512::from(101),
                    ..Default::default()
                },
            ),
            (
                validator2.clone(),
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

    let update = get_update(&mut reader, config);

    // check that the update contains the correct list of validators
    update.assert_validators(&[
        ValidatorInfo::new(&validator1, U512::from(101)),
        ValidatorInfo::new(&validator2, U512::from(102)),
        ValidatorInfo::new(&validator3, U512::from(104)),
    ]);

    update.assert_seigniorage_recipients_written(&mut reader);
    update.assert_total_supply(&mut reader, 610);

    let account3_hash = validator3.to_account_hash();
    let account3 = reader
        .get_account(account3_hash)
        .expect("should have account");
    update.assert_written_balance(account3.main_purse(), 100);

    let old_bid3 = reader
        .get_bids()
        .get(&validator3)
        .cloned()
        .expect("should have bid");
    let bid_purse = *old_bid3.bonding_purse();
    update.assert_written_balance(bid_purse, 104);

    // check bid overwrite
    let expected_bid = Bid::unlocked(validator3, bid_purse, U512::from(104), Default::default());
    update.assert_written_bid(account3_hash, expected_bid);

    // 5 keys should be written:
    // - seigniorage recipients
    // - total supply
    // - balance of bid purse of validator 3
    // - balance of main purse of validator 3
    // - bid of validator 3
    assert_eq!(update.len(), 5);
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
                validator1.clone(),
                U512::from(101),
                ValidatorConfig {
                    bonded_amount: U512::from(101),
                    ..Default::default()
                },
            ),
            (
                validator2.clone(),
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

    let update = get_update(&mut reader, config);

    // check that the update contains the correct list of validators
    update.assert_validators(&[
        ValidatorInfo::new(&validator1, U512::from(101)),
        ValidatorInfo::new(&validator2, U512::from(102)),
        ValidatorInfo::new(&validator3, U512::from(104)),
    ]);

    update.assert_seigniorage_recipients_written(&mut reader);
    update.assert_total_supply(&mut reader, 613);

    // check purse writes
    let account3_hash = validator3.to_account_hash();
    let old_bid3 = reader
        .get_bids()
        .get(&validator3)
        .cloned()
        .expect("should have bid");
    let bid_purse = *old_bid3.bonding_purse();

    update.assert_written_balance(bid_purse, 104);

    // check bid overwrite
    let expected_bid = Bid::unlocked(validator3, bid_purse, U512::from(104), Default::default());
    update.assert_written_bid(account3_hash, expected_bid);

    // 4 keys should be written:
    // - seigniorage recipients
    // - total supply
    // - bid purse balance for validator 3
    // - bid for validator 3
    assert_eq!(update.len(), 4);
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

    let update = get_update(&mut reader, config);

    // check that the update contains the correct list of validators
    update.assert_validators_unchanged();

    update.assert_total_supply(&mut reader, 609);

    // check purse writes
    let account3_hash = validator3.to_account_hash();
    let account3 = reader
        .get_account(account3_hash)
        .expect("should have account");

    update.assert_written_balance(account3.main_purse(), 100);

    // 2 keys should be written:
    // - total supply
    // - balance for main purse of validator 3
    assert_eq!(update.len(), 2);
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
        slash_instead_of_unbonding: true,
        ..Default::default()
    };

    let update = get_update(&mut reader, config);

    // check that the update contains the correct list of validators
    update.assert_validators(&[ValidatorInfo::new(&validator2, U512::from(102))]);

    update.assert_seigniorage_recipients_written(&mut reader);
    update.assert_total_supply(&mut reader, 305);

    // check purse write for validator1
    let old_bid1 = reader
        .get_bids()
        .get(&validator1)
        .cloned()
        .expect("should have bid");
    let bid_purse = *old_bid1.bonding_purse();

    update.assert_written_balance(bid_purse, 0);

    // check bid overwrite
    let account1_hash = validator1.to_account_hash();
    let mut expected_bid_1 = Bid::unlocked(validator1, bid_purse, U512::zero(), Default::default());
    expected_bid_1.deactivate();
    update.assert_written_bid(account1_hash, expected_bid_1);

    // check writes for validator2
    let account2_hash = validator2.to_account_hash();

    // the new account should be created
    let account2 = update.get_written_account(account2_hash);

    // check that the main purse for the new account has been created with the correct amount
    update.assert_written_purse_is_unit(account2.main_purse());
    update.assert_written_balance(account2.main_purse(), 102);

    let bid_write = update.get_written_bid(account2_hash);
    assert_eq!(bid_write.validator_public_key(), &validator2);
    assert_eq!(
        bid_write
            .total_staked_amount()
            .expect("should read total staked amount"),
        U512::from(102)
    );
    assert!(!bid_write.inactive());

    // check that the bid purse for the new validator has been created with the correct amount
    update.assert_written_purse_is_unit(*bid_write.bonding_purse());
    update.assert_written_balance(*bid_write.bonding_purse(), 102);

    // 10 keys should be written:
    // - seigniorage recipients
    // - total supply
    // - bid for validator 1
    // - bonding purse balance for validator 1
    // - account for validator 2
    // - main purse for account for validator 2
    // - main purse balance for account for validator 2
    // - bid for validator 2
    // - bonding purse for validator 2
    // - bonding purse balance for validator 2
    assert_eq!(update.len(), 10);
}

#[test]
fn should_replace_one_validator_with_unbonding() {
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
                ..Default::default()
            }),
        }],
        only_listed_validators: true,
        slash_instead_of_unbonding: false,
        ..Default::default()
    };

    let update = get_update(&mut reader, config);

    // check that the update contains the correct list of validators
    update.assert_validators(&[ValidatorInfo::new(&validator2, U512::from(102))]);

    update.assert_seigniorage_recipients_written(&mut reader);
    update.assert_total_supply(&mut reader, 406);

    // check purse write for validator1
    let old_bid1 = reader
        .get_bids()
        .get(&validator1)
        .cloned()
        .expect("should have bid");
    let bid_purse = *old_bid1.bonding_purse();

    // bid purse balance should be unchanged
    update.assert_key_absent(&Key::Balance(bid_purse.addr()));

    // should write an unbonding purse
    update.assert_unbonding_purse(bid_purse, &validator1, &validator1, 101);

    // check bid overwrite
    let account1_hash = validator1.to_account_hash();
    let mut expected_bid_1 = Bid::unlocked(validator1, bid_purse, U512::zero(), Default::default());
    expected_bid_1.deactivate();
    update.assert_written_bid(account1_hash, expected_bid_1);

    // check writes for validator2
    let account2_hash = validator2.to_account_hash();

    // the new account should be created
    let account2 = update.get_written_account(account2_hash);

    // check that the main purse for the new account has been created with the correct amount
    update.assert_written_purse_is_unit(account2.main_purse());
    update.assert_written_balance(account2.main_purse(), 102);

    let bid_write = update.get_written_bid(account2_hash);
    assert_eq!(bid_write.validator_public_key(), &validator2);
    assert_eq!(
        bid_write
            .total_staked_amount()
            .expect("should read total staked amount"),
        U512::from(102)
    );
    assert!(!bid_write.inactive());

    // check that the bid purse for the new validator has been created with the correct amount
    update.assert_written_purse_is_unit(*bid_write.bonding_purse());
    update.assert_written_balance(*bid_write.bonding_purse(), 102);

    // 10 keys should be written:
    // - seigniorage recipients
    // - total supply
    // - bid for validator 1
    // - unbonding purse for validator 1
    // - account for validator 2
    // - main purse for account for validator 2
    // - main purse balance for account for validator 2
    // - bid for validator 2
    // - bonding purse for validator 2
    // - bonding purse balance for validator 2
    assert_eq!(update.len(), 10);
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
                validator1.clone(),
                U512::from(101),
                ValidatorConfig {
                    bonded_amount: U512::from(101),
                    ..Default::default()
                },
            ),
            (
                validator2.clone(),
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

    let update = get_update(&mut reader, config);

    // check that the update contains the correct list of validators
    update.assert_validators(&[
        ValidatorInfo::new(&validator1, U512::from(101)),
        ValidatorInfo::new(&validator2, U512::from(102)),
        ValidatorInfo::new(&validator3, U512::from(103)),
        ValidatorInfo::new(&validator4, U512::from(104)),
    ]);

    update.assert_seigniorage_recipients_written(&mut reader);
    update.assert_total_supply(&mut reader, 816);

    // check writes for validator4
    let account4_hash = validator4.to_account_hash();

    // the new account should be created
    let account4 = update.get_written_account(account4_hash);

    // check that the main purse for the new account has been created with the correct amount
    update.assert_written_purse_is_unit(account4.main_purse());
    update.assert_written_balance(account4.main_purse(), 100);

    let bid4 = update.get_written_bid(account4_hash);
    assert_eq!(bid4.validator_public_key(), &validator4);
    assert_eq!(
        bid4.total_staked_amount()
            .expect("should read total staked amount"),
        U512::from(104)
    );
    assert!(!bid4.inactive());

    // check that the bid purse for the new validator has been created with the correct amount
    update.assert_written_purse_is_unit(*bid4.bonding_purse());
    update.assert_written_balance(*bid4.bonding_purse(), 104);

    // 8 keys should be written:
    // - seigniorage recipients snapshot
    // - total supply
    // - account for validator 4
    // - main purse for account for validator 4
    // - main purse balance for account for validator 4
    // - bid for validator 4
    // - bonding purse for validator 4
    // - bonding purse balance for validator 4
    assert_eq!(update.len(), 8);
}

#[test]
fn should_add_one_validator_with_delegators() {
    let mut rng = TestRng::new();

    let validator1 = PublicKey::random(&mut rng);
    let validator2 = PublicKey::random(&mut rng);
    let delegator1 = PublicKey::random(&mut rng);

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

    let update = get_update(&mut reader, config);

    // check that the update contains the correct list of validators
    update.assert_validators(&[
        ValidatorInfo::new(&validator1, U512::from(101)),
        ValidatorInfo::new(&validator2, U512::from(115)),
    ]);

    update.assert_seigniorage_recipients_written(&mut reader);
    update.assert_total_supply(&mut reader, 417);

    // check writes for validator2
    let account2_hash = validator2.to_account_hash();

    // the new account should be created
    let account2 = update.get_written_account(account2_hash);

    // check that the main purse for the new account has been created with the correct amount
    update.assert_written_purse_is_unit(account2.main_purse());
    update.assert_written_balance(account2.main_purse(), 100);

    let bid2 = update.get_written_bid(account2_hash);
    assert_eq!(bid2.validator_public_key(), &validator2);
    assert_eq!(*bid2.staked_amount(), U512::from(102));
    assert_eq!(
        bid2.total_staked_amount()
            .expect("should read total staked amount"),
        U512::from(115)
    );
    assert!(!bid2.inactive());

    // check that the bid purse for the new validator has been created with the correct amount
    update.assert_written_purse_is_unit(*bid2.bonding_purse());
    update.assert_written_balance(*bid2.bonding_purse(), 102);

    let bid_delegator_purse = *bid2
        .delegators()
        .get(&delegator1)
        .expect("should have delegator")
        .bonding_purse();

    // check that the bid purse for the new delegator has been created with the correct amount
    update.assert_written_purse_is_unit(bid_delegator_purse);
    update.assert_written_balance(bid_delegator_purse, 13);

    // 10 keys should be written:
    // - seigniorage recipients
    // - total supply
    // - account for validator 2
    // - main purse for account for validator 2
    // - main purse balance for account for validator 2
    // - bid for validator 2
    // - bonding purse for validator 2
    // - bonding purse balance for validator2
    // - bonding purse for delegator
    // - bonding purse balance for delegator
    assert_eq!(update.len(), 10);
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
        slash_instead_of_unbonding: true,
        ..Default::default()
    };

    let update = get_update(&mut reader, config);

    // check that the update contains the correct list of validators
    update.assert_validators(&[ValidatorInfo::new(&validator1, U512::from(115))]);

    update.assert_seigniorage_recipients_written(&mut reader);
    update.assert_total_supply(&mut reader, 216);

    let account1_hash = validator1.to_account_hash();

    let bid1 = update.get_written_bid(account1_hash);
    assert_eq!(bid1.validator_public_key(), &validator1);
    assert_eq!(*bid1.staked_amount(), U512::from(101));
    assert_eq!(
        bid1.total_staked_amount()
            .expect("should read total staked amount"),
        U512::from(115)
    );
    assert!(!bid1.inactive());

    let old_bid1 = reader
        .get_bids()
        .get(&validator1)
        .cloned()
        .expect("should have old bid");
    let delegator1_bid_purse = *old_bid1
        .delegators()
        .get(&delegator1)
        .expect("should have old delegator")
        .bonding_purse();

    let delegator2_bid_purse = *bid1
        .delegators()
        .get(&delegator2)
        .expect("should have new delegator")
        .bonding_purse();

    // check that the old delegator's bid purse got zeroed
    update.assert_written_balance(delegator1_bid_purse, 0);

    // check that the bid purse for the new delegator has been created with the correct amount
    update.assert_written_purse_is_unit(delegator2_bid_purse);
    update.assert_written_balance(delegator2_bid_purse, 14);

    // 6 keys should be written:
    // - seigniorage recipients
    // - total supply
    // - bid for validator 1
    // - bonding purse balance for old delegator
    // - bonding purse for new delegator
    // - bonding purse balance for new delegator
    assert_eq!(update.len(), 6);
}

#[test]
fn should_replace_a_delegator_with_unbonding() {
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
        slash_instead_of_unbonding: false,
        ..Default::default()
    };

    let update = get_update(&mut reader, config);

    // check that the update contains the correct list of validators
    update.assert_validators(&[ValidatorInfo::new(&validator1, U512::from(115))]);

    update.assert_seigniorage_recipients_written(&mut reader);
    update.assert_total_supply(&mut reader, 229);

    let account1_hash = validator1.to_account_hash();

    let bid1 = update.get_written_bid(account1_hash);
    assert_eq!(bid1.validator_public_key(), &validator1);
    assert_eq!(*bid1.staked_amount(), U512::from(101));
    assert_eq!(
        bid1.total_staked_amount()
            .expect("should read total staked amount"),
        U512::from(115)
    );
    assert!(!bid1.inactive());

    let old_bid1 = reader
        .get_bids()
        .get(&validator1)
        .cloned()
        .expect("should have old bid");
    let delegator1_bid_purse = *old_bid1
        .delegators()
        .get(&delegator1)
        .expect("should have old delegator")
        .bonding_purse();

    let delegator2_bid_purse = *bid1
        .delegators()
        .get(&delegator2)
        .expect("should have new delegator")
        .bonding_purse();

    // check that the old delegator's bid purse hasn't been updated
    update.assert_key_absent(&Key::Balance(delegator1_bid_purse.addr()));

    // check that the old delegator has been unbonded
    update.assert_unbonding_purse(delegator1_bid_purse, &validator1, &delegator1, 13);

    // check that the bid purse for the new delegator has been created with the correct amount
    update.assert_written_purse_is_unit(delegator2_bid_purse);
    update.assert_written_balance(delegator2_bid_purse, 14);

    // 6 keys should be written:
    // - seigniorage recipients
    // - total supply
    // - bid for validator 1
    // - unbonding purse for old delegator
    // - bonding purse for new delegator
    // - bonding purse balance for new delegator
    assert_eq!(update.len(), 6);
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

    let update = get_update(&mut reader, config);

    // check that the update contains the correct list of validators
    update.assert_validators(&[ValidatorInfo::new(&validator1, U512::from(124))]);

    update.assert_seigniorage_recipients_written(&mut reader);
    update.assert_total_supply(&mut reader, 225);

    let account1_hash = validator1.to_account_hash();

    let bid1 = update.get_written_bid(account1_hash);
    assert_eq!(bid1.validator_public_key(), &validator1);
    assert_eq!(*bid1.staked_amount(), U512::from(111));
    assert_eq!(
        bid1.total_staked_amount()
            .expect("should read total staked amount"),
        U512::from(124)
    );
    assert!(!bid1.inactive());

    // check that the validator's bid purse got updated
    update.assert_written_balance(*bid1.bonding_purse(), 111);

    // 4 keys should be written:
    // - seigniorage recipients
    // - total supply
    // - bid for validator 1
    // - bonding purse balance for validator 1
    assert_eq!(update.len(), 4);
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
        slash_instead_of_unbonding: true,
        ..Default::default()
    };

    let update = get_update(&mut reader, config);

    // check that the update contains the correct list of validators
    update.assert_validators(&[ValidatorInfo::new(&validator1, U512::from(111))]);

    update.assert_seigniorage_recipients_written(&mut reader);
    update.assert_total_supply(&mut reader, 212);

    let account1_hash = validator1.to_account_hash();

    let bid1 = update.get_written_bid(account1_hash);
    assert_eq!(bid1.validator_public_key(), &validator1);
    assert_eq!(*bid1.staked_amount(), U512::from(111));
    assert_eq!(
        bid1.total_staked_amount()
            .expect("should read total staked amount"),
        U512::from(111)
    );
    assert!(!bid1.inactive());

    // check that the validator's bid purse got updated
    update.assert_written_balance(*bid1.bonding_purse(), 111);

    let old_bid1 = reader
        .get_bids()
        .get(&validator1)
        .cloned()
        .expect("should have old bid");
    let delegator1_bid_purse = *old_bid1
        .delegators()
        .get(&delegator1)
        .expect("should have old delegator")
        .bonding_purse();

    // check that the old delegator's bid purse got zeroed
    update.assert_written_balance(delegator1_bid_purse, 0);

    // 5 keys should be written:
    // - seigniorage recipients
    // - total supply
    // - bid for validator 1
    // - bonding purse balance for validator 1
    // - bonding purse balance for delegator
    assert_eq!(update.len(), 5);
}

#[test]
fn should_remove_the_delegator_with_unbonding() {
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
        slash_instead_of_unbonding: false,
        ..Default::default()
    };

    let update = get_update(&mut reader, config);

    // check that the update contains the correct list of validators
    update.assert_validators(&[ValidatorInfo::new(&validator1, U512::from(111))]);

    update.assert_seigniorage_recipients_written(&mut reader);
    update.assert_total_supply(&mut reader, 225);

    let account1_hash = validator1.to_account_hash();

    let bid1 = update.get_written_bid(account1_hash);
    assert_eq!(bid1.validator_public_key(), &validator1);
    assert_eq!(*bid1.staked_amount(), U512::from(111));
    assert_eq!(
        bid1.total_staked_amount()
            .expect("should read total staked amount"),
        U512::from(111)
    );
    assert!(!bid1.inactive());

    // check that the validator's bid purse got updated
    update.assert_written_balance(*bid1.bonding_purse(), 111);

    let old_bid1 = reader
        .get_bids()
        .get(&validator1)
        .cloned()
        .expect("should have old bid");
    let delegator1_bid_purse = *old_bid1
        .delegators()
        .get(&delegator1)
        .expect("should have old delegator")
        .bonding_purse();

    // check that the old delegator's bid purse hasn't been updated
    update.assert_key_absent(&Key::Balance(delegator1_bid_purse.addr()));

    // check that the unbonding purse got created
    update.assert_unbonding_purse(delegator1_bid_purse, &validator1, &delegator1, 13);

    // 5 keys should be written:
    // - seigniorage recipients
    // - total supply
    // - bid for validator 1
    // - bonding purse balance for validator 1
    // - unbonding purse for delegator
    assert_eq!(update.len(), 5);
}
