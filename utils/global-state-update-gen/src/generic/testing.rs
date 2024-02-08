use itertools::Itertools;
use std::collections::BTreeMap;

use rand::Rng;

use casper_types::{
    account::AccountHash,
    addressable_entity::{ActionThresholds, AssociatedKeys, MessageTopics, Weight},
    system::auction::{
        BidKind, BidsExt, Delegator, SeigniorageRecipient, SeigniorageRecipients,
        SeigniorageRecipientsSnapshot, UnbondingPurse, UnbondingPurses, ValidatorBid,
        WithdrawPurse, WithdrawPurses,
    },
    testing::TestRng,
    AccessRights, AddressableEntity, ByteCodeHash, CLValue, EntityKind, EntryPoints, EraId, Key,
    PackageHash, ProtocolVersion, PublicKey, StoredValue, URef, URefAddr, U512,
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
    accounts: BTreeMap<AccountHash, AddressableEntity>,
    purses: BTreeMap<URefAddr, U512>,
    total_supply: U512,
    seigniorage_recipients: SeigniorageRecipientsSnapshot,
    bids: Vec<BidKind>,
    withdraws: WithdrawPurses,
    unbonds: UnbondingPurses,
    protocol_version: ProtocolVersion,
}

impl MockStateReader {
    fn new() -> Self {
        Self {
            accounts: BTreeMap::new(),
            purses: BTreeMap::new(),
            total_supply: U512::zero(),
            seigniorage_recipients: SeigniorageRecipientsSnapshot::new(),
            bids: vec![],
            withdraws: WithdrawPurses::new(),
            unbonds: UnbondingPurses::new(),
            protocol_version: ProtocolVersion::V1_0_0,
        }
    }

    fn with_account<R: Rng>(
        mut self,
        account_hash: AccountHash,
        balance: U512,
        rng: &mut R,
    ) -> Self {
        let main_purse = URef::new(rng.gen(), AccessRights::READ_ADD_WRITE);
        let entity = AddressableEntity::new(
            PackageHash::new(rng.gen()),
            ByteCodeHash::new(rng.gen()),
            EntryPoints::new(),
            self.protocol_version,
            main_purse,
            AssociatedKeys::new(account_hash, Weight::new(1)),
            ActionThresholds::default(),
            MessageTopics::default(),
            EntityKind::Account(account_hash),
        );

        self.purses.insert(main_purse.addr(), balance);
        // If `insert` returns `Some()`, it means we used the same account hash twice, which is
        // a programmer error and the function will panic.
        assert!(self.accounts.insert(account_hash, entity).is_none());
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

                self.bids.push(BidKind::Delegator(Box::new(delegator)));
            }

            let validator_bid =
                ValidatorBid::unlocked(public_key.clone(), bonding_purse, stake, delegation_rate);

            self.bids.push(BidKind::Validator(Box::new(validator_bid)));
        }

        for era_id in 0..5 {
            self.seigniorage_recipients
                .insert(era_id.into(), recipients.clone());
        }

        self
    }

    /// Returns the bonding purse if the unbonder exists in `self.bids`.
    fn unbonder_bonding_purse(
        &self,
        validator_public_key: &PublicKey,
        unbonder_public_key: &PublicKey,
    ) -> Option<URef> {
        let bid = self.bids.validator_bid(validator_public_key)?;
        if unbonder_public_key == validator_public_key {
            return Some(*bid.bonding_purse());
        }

        Some(
            *self
                .bids
                .delegator_by_public_keys(validator_public_key, unbonder_public_key)?
                .bonding_purse(),
        )
    }

    /// Returns the bonding purse if the unbonder exists in `self.bids`, or creates a new account
    /// with a nominal stake with the given validator and returns the new unbonder's bonding purse.
    fn create_or_get_unbonder_bonding_purse<R: Rng>(
        &mut self,
        validator_public_key: &PublicKey,
        unbonder_public_key: &PublicKey,
        rng: &mut R,
    ) -> URef {
        if let Some(purse) = self.unbonder_bonding_purse(validator_public_key, unbonder_public_key)
        {
            return purse;
        }
        // // create the account if it doesn't exist
        // let account_hash = unbonder_public_key.to_account_hash();
        // if !self.accounts.contains_key(&account_hash) {
        //     *self = self.with_account(account_hash, U512::from(100), rng);
        // }

        let bonding_purse = URef::new(rng.gen(), AccessRights::READ_ADD_WRITE);
        let stake = U512::from(10);
        self.purses.insert(bonding_purse.addr(), stake);
        self.total_supply += stake;
        bonding_purse
    }

    /// Creates a `WithdrawPurse` for 1 mote.  If the validator or delegator don't exist in
    /// `self.bids`, a random bonding purse is assigned.
    fn with_withdraw<R: Rng>(
        mut self,
        validator_public_key: PublicKey,
        unbonder_public_key: PublicKey,
        era_of_creation: EraId,
        amount: U512,
        rng: &mut R,
    ) -> Self {
        let bonding_purse = self.create_or_get_unbonder_bonding_purse(
            &validator_public_key,
            &unbonder_public_key,
            rng,
        );

        let withdraw = WithdrawPurse::new(
            bonding_purse,
            validator_public_key,
            unbonder_public_key,
            era_of_creation,
            amount,
        );

        let withdraws = self
            .withdraws
            .entry(withdraw.validator_public_key().to_account_hash())
            .or_default();
        withdraws.push(withdraw);
        self
    }

    /// Creates an `UnbondingPurse` for 1 mote.  If the validator or delegator don't exist in
    /// `self.bids`, a random bonding purse is assigned.
    fn with_unbond<R: Rng>(
        mut self,
        validator_public_key: PublicKey,
        unbonder_public_key: PublicKey,
        amount: U512,
        rng: &mut R,
    ) -> Self {
        let purse_uref = self.create_or_get_unbonder_bonding_purse(
            &validator_public_key,
            &unbonder_public_key,
            rng,
        );

        let unbonding_purse = UnbondingPurse::new(
            purse_uref,
            validator_public_key,
            unbonder_public_key,
            EraId::new(10),
            amount,
            None,
        );

        let account_hash = unbonding_purse.validator_public_key().to_account_hash();
        match self.unbonds.get_mut(&account_hash) {
            None => {
                self.unbonds.insert(account_hash, vec![unbonding_purse]);
            }
            Some(existing_purses) => {
                if !existing_purses.contains(&unbonding_purse) {
                    existing_purses.push(unbonding_purse)
                }
            }
        }
        self
    }

    fn total_supply(&self) -> U512 {
        self.total_supply
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
            key => unimplemented!(
                "Querying a key of type {:?} is not handled",
                key.type_string()
            ),
        }
    }

    fn get_total_supply_key(&mut self) -> Key {
        Key::URef(TOTAL_SUPPLY_KEY)
    }

    fn get_seigniorage_recipients_key(&mut self) -> Key {
        Key::URef(SEIGNIORAGE_RECIPIENTS_KEY)
    }

    fn get_account(&mut self, account_hash: AccountHash) -> Option<AddressableEntity> {
        self.accounts.get(&account_hash).cloned()
    }

    fn get_bids(&mut self) -> Vec<BidKind> {
        self.bids.clone()
    }

    fn get_withdraws(&mut self) -> WithdrawPurses {
        self.withdraws.clone()
    }

    fn get_unbonds(&mut self) -> UnbondingPurses {
        self.unbonds.clone()
    }

    fn get_protocol_version(&mut self) -> ProtocolVersion {
        self.protocol_version
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
    let account2 = update.get_written_addressable_entity(account2);

    // should write decreased balance to the first purse
    update.assert_written_balance(account1.main_purse(), 700_000_000);

    // check that the main purse for the new account has been created with the correct amount
    update.assert_written_balance(account2.main_purse(), 300_000_000);
    update.assert_written_purse_is_unit(account2.main_purse());

    // total supply is written on every purse balance change, so we'll have a write to this key
    // even though the changes cancel each other out
    update.assert_total_supply(&mut reader, 1_000_000_001);

    // 7 keys should be written:
    // - balance of account 1
    // - account indirection for account 2
    // - the package for the addressable entity associated with account 2
    // - the addressable entity associated with account 2.
    // - main purse of account 2
    // - balance of account 2
    // - total supply
    assert_eq!(update.len(), 7);
}

fn validator_config(
    public_key: &PublicKey,
    balance: U512,
    staked: U512,
) -> (PublicKey, U512, ValidatorConfig) {
    (
        public_key.clone(),
        balance,
        ValidatorConfig {
            bonded_amount: staked,
            ..Default::default()
        },
    )
}

#[test]
fn should_change_one_validator() {
    let mut rng = TestRng::new();

    let validator1 = PublicKey::random(&mut rng);
    let validator1_staked = U512::from(1);
    let validator2 = PublicKey::random(&mut rng);
    let validator2_staked = U512::from(2);
    let validator3 = PublicKey::random(&mut rng);
    let validator3_staked = U512::from(3);

    let liquid = U512::from(5);

    let validators = vec![
        validator_config(&validator1, liquid, validator1_staked),
        validator_config(&validator2, liquid, validator2_staked),
        validator_config(&validator3, liquid, validator3_staked),
    ];
    let mut reader = MockStateReader::new().with_validators(validators, &mut rng);

    let mut total_supply: U512 =
        (liquid * 3) + validator1_staked + validator2_staked + validator3_staked;

    assert_eq!(
        reader.total_supply(),
        total_supply,
        "initial total supply mismatch"
    );

    let validator3_new_balance = liquid.saturating_add(1.into());
    let validator3_new_staked = validator3_staked.saturating_add(1.into());
    total_supply = total_supply.saturating_add(2.into());

    // we'll be increasing the stake and balance of validator 3
    let config = Config {
        accounts: vec![AccountConfig {
            public_key: validator3.clone(),
            balance: Some(validator3_new_balance),
            validator: Some(ValidatorConfig {
                bonded_amount: validator3_new_staked,
                delegation_rate: None,
                delegators: None,
            }),
        }],
        ..Default::default()
    };

    let update = get_update(&mut reader, config);

    // check that the update contains the correct list of validators
    update.assert_validators(&[
        ValidatorInfo::new(&validator1, validator1_staked),
        ValidatorInfo::new(&validator2, validator2_staked),
        ValidatorInfo::new(&validator3, validator3_new_staked),
    ]);

    update.assert_seigniorage_recipients_written(&mut reader);
    update.assert_total_supply(&mut reader, total_supply.as_u64());

    let account3_hash = validator3.to_account_hash();
    let account3 = reader
        .get_account(account3_hash)
        .expect("should have account");
    update.assert_written_balance(account3.main_purse(), validator3_new_balance.as_u64());

    let bids = reader.get_bids();
    println!("should_change_one_validator {:?}", bids);

    let old_bid3 = bids.validator_bid(&validator3).expect("should have bid");
    let bid_purse = *old_bid3.bonding_purse();
    update.assert_written_balance(bid_purse, validator3_new_staked.as_u64());

    // check bid overwrite
    let expected_bid = ValidatorBid::unlocked(
        validator3,
        bid_purse,
        validator3_new_staked,
        Default::default(),
    );
    update.assert_written_bid(account3_hash, BidKind::Validator(Box::new(expected_bid)));

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
        .validator_bid(&validator3)
        .expect("should have bid");
    let bid_purse = *old_bid3.bonding_purse();

    update.assert_written_balance(bid_purse, 104);

    // check bid overwrite
    let expected_bid =
        ValidatorBid::unlocked(validator3, bid_purse, U512::from(104), Default::default());
    update.assert_written_bid(account3_hash, BidKind::Validator(Box::new(expected_bid)));

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
        .validator_bid(&validator1)
        .expect("should have bid");
    let bid_purse = *old_bid1.bonding_purse();

    update.assert_written_balance(bid_purse, 0);

    // check bid overwrite
    let account1_hash = validator1.to_account_hash();
    let mut expected_bid_1 =
        ValidatorBid::unlocked(validator1, bid_purse, U512::zero(), Default::default());
    expected_bid_1.deactivate();
    update.assert_written_bid(account1_hash, BidKind::Validator(Box::new(expected_bid_1)));

    // check writes for validator2
    let account2_hash = validator2.to_account_hash();

    // the new account should be created
    let account2 = update.get_written_addressable_entity(account2_hash);

    // check that the main purse for the new account has been created with the correct amount
    update.assert_written_purse_is_unit(account2.main_purse());
    update.assert_written_balance(account2.main_purse(), 102);

    let bid_write = update.get_written_bid(account2_hash);
    assert_eq!(bid_write.validator_public_key(), validator2);
    let total_stake = update
        .get_total_stake(account2_hash)
        .expect("should have total stake");
    assert_eq!(total_stake, U512::from(102));
    assert!(!bid_write.inactive());

    // check that the bid purse for the new validator has been created with the correct amount
    update.assert_written_purse_is_unit(bid_write.bonding_purse());
    update.assert_written_balance(bid_write.bonding_purse(), 102);

    // 12 keys should be written:
    // - seigniorage recipients
    // - total supply
    // - bid for validator 1
    // - bonding purse balance for validator 1
    // - account indirection for validator 2
    // - the package for the addressable entity associated with validator 2
    // - the addressable entity associated with validator 2.
    // - main purse for account for validator 2
    // - main purse balance for account for validator 2
    // - bid for validator 2
    // - bonding purse for validator 2
    // - bonding purse balance for validator 2
    assert_eq!(update.len(), 12);
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
        .validator_bid(&validator1)
        .expect("should have bid");
    let bid_purse = *old_bid1.bonding_purse();

    // bid purse balance should be unchanged
    update.assert_key_absent(&Key::Balance(bid_purse.addr()));

    // should write an unbonding purse
    update.assert_unbonding_purse(bid_purse, &validator1, &validator1, 101);

    // check bid overwrite
    let account1_hash = validator1.to_account_hash();
    let mut expected_bid_1 =
        ValidatorBid::unlocked(validator1, bid_purse, U512::zero(), Default::default());
    expected_bid_1.deactivate();
    update.assert_written_bid(account1_hash, BidKind::Validator(Box::new(expected_bid_1)));

    // check writes for validator2
    let account2_hash = validator2.to_account_hash();

    // the new account should be created
    let account2 = update.get_written_addressable_entity(account2_hash);

    // check that the main purse for the new account has been created with the correct amount
    update.assert_written_purse_is_unit(account2.main_purse());
    update.assert_written_balance(account2.main_purse(), 102);

    let bid_write = update.get_written_bid(account2_hash);
    assert_eq!(bid_write.validator_public_key(), validator2);
    let total_stake = update
        .get_total_stake(account2_hash)
        .expect("should have total stake");
    assert_eq!(total_stake, U512::from(102));
    assert!(!bid_write.inactive());

    // check that the bid purse for the new validator has been created with the correct amount
    update.assert_written_purse_is_unit(bid_write.bonding_purse());
    update.assert_written_balance(bid_write.bonding_purse(), 102);

    // 12 keys should be written:
    // - seigniorage recipients
    // - total supply
    // - bid for validator 1
    // - unbonding purse for validator 1
    // - account indirection for validator 2
    // - the package for the addressable entity associated with validator 2
    // - the addressable entity associated with validator 2.
    // - main purse for account for validator 2
    // - main purse balance for account for validator 2
    // - bid for validator 2
    // - bonding purse for validator 2
    // - bonding purse balance for validator 2
    assert_eq!(update.len(), 12);
}

#[test]
fn should_add_one_validator() {
    let mut rng = TestRng::new();

    let mut validators = BTreeMap::new();
    for index in 1..4 {
        let balance = index * 10;
        validators.insert(
            PublicKey::random(&mut rng),
            (U512::from(balance), U512::from(index)),
        );
    }

    let initial_validators = validators
        .iter()
        .map(|(k, (b, s))| {
            (
                k.clone(),
                *b,
                ValidatorConfig {
                    bonded_amount: *s,
                    ..Default::default()
                },
            )
        })
        .collect();

    let initial_supply: u64 = validators.iter().map(|(_, (b, s))| (b + s).as_u64()).sum();

    let mut reader = MockStateReader::new().with_validators(initial_validators, &mut rng);

    assert_eq!(
        reader.total_supply().as_u64(),
        initial_supply,
        "initial supply should equal"
    );

    let validator4 = PublicKey::random(&mut rng);
    let v4_balance = U512::from(40);
    let v4_stake = U512::from(4);
    validators.insert(validator4.clone(), (v4_balance, v4_stake));
    let config = Config {
        accounts: vec![AccountConfig {
            public_key: validator4.clone(),
            balance: Some(v4_balance),
            validator: Some(ValidatorConfig {
                bonded_amount: v4_stake,
                delegation_rate: None,
                delegators: None,
            }),
        }],
        only_listed_validators: false,
        ..Default::default()
    };

    let update = get_update(&mut reader, config);
    let expected_supply: u64 = validators.iter().map(|(_, (b, s))| (b + s).as_u64()).sum();
    assert_eq!(
        initial_supply + (v4_stake + v4_balance).as_u64(),
        expected_supply,
        "should match"
    );

    update.assert_total_supply(&mut reader, expected_supply);

    let expected_staking = validators
        .iter()
        .map(|(k, (_, s))| ValidatorInfo::new(k, *s))
        .collect_vec();

    // check that the update contains the correct list of validators
    update.assert_validators(&expected_staking);
    update.assert_seigniorage_recipients_written(&mut reader);

    // check writes for validator4
    let account4_hash = validator4.to_account_hash();
    // the new account should be created
    let account4 = update.get_written_addressable_entity(account4_hash);
    let total_stake = update
        .get_total_stake(account4_hash)
        .expect("should have total stake");
    assert_eq!(total_stake, v4_stake);
    // check that the main purse for the new account has been created with the correct amount
    update.assert_written_purse_is_unit(account4.main_purse());
    update.assert_written_balance(account4.main_purse(), v4_balance.as_u64());
    // check that the bid purse for the new validator has been created with the correct amount
    let bid4 = update.get_written_bid(account4_hash);
    assert_eq!(bid4.validator_public_key(), validator4);
    update.assert_written_balance(bid4.bonding_purse(), v4_stake.as_u64());
    update.assert_written_purse_is_unit(bid4.bonding_purse());

    assert!(!bid4.inactive());

    // 8 keys should be written:
    // - seigniorage recipients snapshot
    // - total supply
    // - account indirection for validator 4
    // - package for the addressable entity associated with validator 4
    // - the addressable entity record associated with validator 4
    // - main purse for account for validator 4
    // - main purse balance for account for validator 4
    // - bid for validator 4
    // - bonding purse for validator 4
    // - bonding purse balance for validator 4
    assert_eq!(update.len(), 10);
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
    let account2 = update.get_written_addressable_entity(account2_hash);

    // check that the main purse for the new account has been created with the correct amount
    update.assert_written_purse_is_unit(account2.main_purse());
    update.assert_written_balance(account2.main_purse(), 100);

    let bid2 = update.get_written_bid(account2_hash);
    assert_eq!(bid2.validator_public_key(), validator2);
    assert_eq!(bid2.staked_amount(), U512::from(102));
    let total_staked = update
        .get_total_stake(account2_hash)
        .expect("should have total stake");
    assert_eq!(total_staked, U512::from(115));
    assert!(!bid2.inactive());

    // check that the bid purse for the new validator has been created with the correct amount
    update.assert_written_purse_is_unit(bid2.bonding_purse());
    update.assert_written_balance(bid2.bonding_purse(), 102);

    if let BidKind::Validator(validator_bid) = bid2 {
        let bid_delegator_purse = *update
            .delegator(&validator_bid, &delegator1)
            .expect("should have delegator")
            .bonding_purse();
        // check that the bid purse for the new delegator has been created with the correct amount
        update.assert_written_purse_is_unit(bid_delegator_purse);
        update.assert_written_balance(bid_delegator_purse, 13);
    }

    // 13 keys should be written:
    // - seigniorage recipients
    // - total supply
    // - account indirection for validator 2
    // - main purse for account for validator 2
    // - main purse balance for account for validator 2
    // - package for the addressable entity associated with validator 2
    // - the addressable entity record associated with validator 2
    // - bid for validator 2
    // - bonding purse for validator 2
    // - bonding purse balance for validator2
    // - bid for delegator
    // - bonding purse for delegator
    // - bonding purse balance for delegator
    assert_eq!(update.len(), 13);
}

#[test]
fn should_replace_a_delegator() {
    let mut rng = TestRng::new();

    let validator1 = PublicKey::random(&mut rng);
    let v1_stake = 1;
    let v1_balance = 100;
    let v1_updated_balance = 100;
    let v1_updated_stake = 4;
    let delegator1 = PublicKey::random(&mut rng);
    let d1_stake = 2;
    let delegator2 = PublicKey::random(&mut rng);
    let d2_stake = 3;

    let mut reader = MockStateReader::new().with_validators(
        vec![(
            validator1.clone(),
            U512::from(v1_balance),
            ValidatorConfig {
                bonded_amount: U512::from(v1_stake),
                delegation_rate: Some(5),
                delegators: Some(vec![DelegatorConfig {
                    public_key: delegator1.clone(),
                    delegated_amount: U512::from(d1_stake),
                }]),
            },
        )],
        &mut rng,
    );

    // we'll be replacing the delegator
    let config = Config {
        accounts: vec![AccountConfig {
            public_key: validator1.clone(),
            balance: Some(U512::from(v1_updated_balance)),
            validator: Some(ValidatorConfig {
                bonded_amount: U512::from(v1_updated_stake),
                delegation_rate: None,
                delegators: Some(vec![DelegatorConfig {
                    public_key: delegator2.clone(),
                    delegated_amount: U512::from(d2_stake),
                }]),
            }),
        }],
        only_listed_validators: false,
        slash_instead_of_unbonding: true,
        ..Default::default()
    };

    let update = get_update(&mut reader, config);

    // check that the update contains the correct list of validators
    update.assert_validators(&[ValidatorInfo::new(
        &validator1,
        U512::from(v1_updated_stake + d2_stake),
    )]);

    update.assert_seigniorage_recipients_written(&mut reader);
    update.assert_total_supply(
        &mut reader,
        v1_updated_balance + v1_updated_stake + d2_stake,
    );

    let account1_hash = validator1.to_account_hash();

    let bid1 = update.get_written_bid(account1_hash);
    assert_eq!(bid1.validator_public_key(), validator1);
    assert_eq!(bid1.staked_amount(), U512::from(v1_updated_stake));
    let total_stake = update
        .get_total_stake(account1_hash)
        .expect("should have total stake");
    assert_eq!(total_stake, U512::from(v1_updated_stake + d2_stake));
    assert!(!bid1.inactive());

    let initial_bids = reader.get_bids();

    let validator_bid = initial_bids
        .validator_bid(&validator1)
        .expect("should have old bid");
    let delegator1_bid_purse = *initial_bids
        .delegator_by_public_keys(&validator1, &delegator1)
        .expect("should have old delegator")
        .bonding_purse();

    let delegator2_bid_purse = *update
        .delegator(&validator_bid, &delegator2)
        .expect("should have new delegator")
        .bonding_purse();

    // check that the old delegator's bid purse got zeroed
    update.assert_written_balance(delegator1_bid_purse, 0);

    // check that the bid purse for the new delegator has been created with the correct amount
    update.assert_written_purse_is_unit(delegator2_bid_purse);
    update.assert_written_balance(delegator2_bid_purse, d2_stake);

    // 9 keys should be written:
    // - seigniorage recipients
    // - total supply
    // - main purse for validator 1
    // - 3 bids, 3 balances
    assert_eq!(update.len(), 9);
}

#[test]
fn should_replace_a_delegator_with_unbonding() {
    let mut rng = TestRng::new();

    let validator1 = PublicKey::random(&mut rng);
    let (v1_stake, v1_balance) = (1, 100);
    let (v1_updated_stake, v1_updated_balance) = (4, 200);
    let delegator1 = PublicKey::random(&mut rng);
    let d1_stake = 2;
    let delegator2 = PublicKey::random(&mut rng);
    let d2_stake = 3;

    let mut reader = MockStateReader::new().with_validators(
        vec![(
            validator1.clone(),
            U512::from(v1_balance),
            ValidatorConfig {
                bonded_amount: U512::from(v1_stake),
                delegation_rate: Some(5),
                delegators: Some(vec![DelegatorConfig {
                    public_key: delegator1.clone(),
                    delegated_amount: U512::from(d1_stake),
                }]),
            },
        )],
        &mut rng,
    );

    // we'll be replacing the delegator
    let config = Config {
        accounts: vec![AccountConfig {
            public_key: validator1.clone(),
            balance: Some(U512::from(v1_updated_balance)),
            validator: Some(ValidatorConfig {
                bonded_amount: U512::from(v1_updated_stake),
                delegation_rate: None,
                delegators: Some(vec![DelegatorConfig {
                    public_key: delegator2.clone(),
                    delegated_amount: U512::from(d2_stake),
                }]),
            }),
        }],
        only_listed_validators: false,
        slash_instead_of_unbonding: false,
        ..Default::default()
    };

    let update = get_update(&mut reader, config);

    // check that the update contains the correct list of validators
    update.assert_validators(&[ValidatorInfo::new(
        &validator1,
        U512::from(v1_updated_stake + d2_stake),
    )]);

    update.assert_seigniorage_recipients_written(&mut reader);
    update.assert_total_supply(
        &mut reader,
        v1_updated_balance + v1_updated_stake + d1_stake + d2_stake,
    );

    let account1_hash = validator1.to_account_hash();

    let bid1 = update.get_written_bid(account1_hash);
    assert_eq!(bid1.validator_public_key(), validator1);
    assert_eq!(bid1.staked_amount(), U512::from(v1_updated_stake));
    let total_stake = update
        .get_total_stake(account1_hash)
        .expect("should have total stake");
    assert_eq!(total_stake, U512::from(v1_updated_stake + d2_stake));
    assert!(!bid1.inactive());

    let initial_bids = reader.get_bids();

    let validator_bid = initial_bids
        .validator_bid(&validator1)
        .expect("should have old bid");
    let delegator1_bid_purse = *initial_bids
        .delegator_by_public_keys(&validator1, &delegator1)
        .expect("should have old delegator")
        .bonding_purse();

    let delegator2_bid_purse = *update
        .delegator(&validator_bid, &delegator2)
        .expect("should have new delegator")
        .bonding_purse();

    // check that the old delegator's bid purse hasn't been updated
    update.assert_key_absent(&Key::Balance(delegator1_bid_purse.addr()));

    // check that the old delegator has been unbonded
    update.assert_unbonding_purse(delegator1_bid_purse, &validator1, &delegator1, d1_stake);

    // check that the bid purse for the new delegator has been created with the correct amount
    update.assert_written_purse_is_unit(delegator2_bid_purse);
    update.assert_written_balance(delegator2_bid_purse, d2_stake);

    // 10 keys should be written:
    // - seigniorage recipients
    // - total supply
    // - main purse for validator 1
    // - 3 bids, 3 balances, 1 unbond
    assert_eq!(update.len(), 10);
}

#[test]
fn should_not_change_the_delegator() {
    let mut rng = TestRng::new();

    let validator1 = PublicKey::random(&mut rng);
    let v1_balance = 100;
    let v1_stake = 1;
    let delegator1 = PublicKey::random(&mut rng);
    let d1_stake = 2;
    let v1_updated_stake = 3;
    let v1_updated_balance = 200;

    let mut reader = MockStateReader::new().with_validators(
        vec![(
            validator1.clone(),
            U512::from(v1_balance),
            ValidatorConfig {
                bonded_amount: U512::from(v1_stake),
                delegation_rate: Some(5),
                delegators: Some(vec![DelegatorConfig {
                    public_key: delegator1,
                    delegated_amount: U512::from(d1_stake),
                }]),
            },
        )],
        &mut rng,
    );

    // we'll be changing the validator's stake
    let config = Config {
        accounts: vec![AccountConfig {
            public_key: validator1.clone(),
            balance: Some(U512::from(v1_updated_balance)),
            validator: Some(ValidatorConfig {
                bonded_amount: U512::from(v1_updated_stake),
                delegation_rate: None,
                delegators: None,
            }),
        }],
        only_listed_validators: false,
        ..Default::default()
    };

    let update = get_update(&mut reader, config);

    // check that the update contains the correct list of validators
    update.assert_validators(&[ValidatorInfo::new(
        &validator1,
        U512::from(d1_stake + v1_updated_stake),
    )]);

    update.assert_seigniorage_recipients_written(&mut reader);
    update.assert_total_supply(
        &mut reader,
        v1_updated_balance + d1_stake + v1_updated_stake,
    );

    let account1_hash = validator1.to_account_hash();

    let bid1 = update.get_written_bid(account1_hash);
    assert_eq!(bid1.validator_public_key(), validator1);
    assert_eq!(bid1.staked_amount(), U512::from(v1_updated_stake));
    let total_stake = update
        .get_total_stake(account1_hash)
        .expect("should have total stake");
    assert_eq!(total_stake, U512::from(v1_updated_stake));
    assert!(!bid1.inactive());

    // check that the validator's bid purse got updated
    update.assert_written_balance(bid1.bonding_purse(), v1_updated_stake);

    // 5 keys should be written:
    // - seigniorage recipients
    // - total supply
    // - bid for validator 1
    // - bid for delegator 1
    // - bonding purse balance for validator 1
    assert_eq!(update.len(), 5);
}

#[test]
fn should_remove_the_delegator() {
    let mut rng = TestRng::new();

    let validator1 = PublicKey::random(&mut rng);
    let delegator1 = PublicKey::random(&mut rng);

    let v_balance = U512::from(10);
    let v_stake = U512::from(1);
    let d_stake = U512::from(2);
    let initial_supply = v_balance + v_stake + d_stake;

    let mut reader = MockStateReader::new().with_validators(
        vec![(
            validator1.clone(),
            v_balance,
            ValidatorConfig {
                bonded_amount: v_stake,
                delegation_rate: Some(5),
                delegators: Some(vec![DelegatorConfig {
                    public_key: delegator1.clone(),
                    delegated_amount: d_stake,
                }]),
            },
        )],
        &mut rng,
    );

    assert_eq!(
        reader.total_supply(),
        initial_supply,
        "should match initial supply"
    );

    /* validator and delegator bids should be present */
    let original_bids = reader.get_bids();
    let validator_bid = original_bids
        .validator_bid(&validator1)
        .expect("should have old bid");
    let validator_initial_stake = reader
        .purses
        .get(&validator_bid.bonding_purse().addr())
        .expect("should have validator initial stake");
    assert_eq!(
        *validator_initial_stake, v_stake,
        "validator initial balance should match"
    );
    let delegator_bid = original_bids
        .delegator_by_public_keys(&validator1, &delegator1)
        .expect("should have delegator");
    let delegator_initial_stake = reader
        .purses
        .get(&delegator_bid.bonding_purse().addr())
        .expect("should have delegator initial stake");
    assert_eq!(
        *delegator_initial_stake, d_stake,
        "delegator initial balance should match"
    );

    let v_updated_balance = U512::from(20);
    let v_updated_stake = U512::from(2);
    let updated_supply = v_updated_balance + v_updated_stake;

    /* make various changes to the bid, including removal of delegator */
    let config = Config {
        accounts: vec![AccountConfig {
            public_key: validator1.clone(),
            balance: Some(v_updated_balance),
            validator: Some(ValidatorConfig {
                bonded_amount: v_updated_stake,
                delegation_rate: None,
                delegators: Some(vec![]),
            }),
        }],
        only_listed_validators: false,
        slash_instead_of_unbonding: true,
        ..Default::default()
    };

    let update = get_update(&mut reader, config);

    /* check high level details */
    let expected_validator_set = &[ValidatorInfo::new(&validator1, v_updated_stake)];
    update.assert_validators(expected_validator_set);
    update.assert_seigniorage_recipients_written(&mut reader);
    update.assert_total_supply(&mut reader, updated_supply.as_u64());

    /* check validator bid details */
    let account1_hash = validator1.to_account_hash();
    let updated_validator_bid = update.get_written_bid(account1_hash);
    update.assert_written_balance(
        updated_validator_bid.bonding_purse(),
        v_updated_stake.as_u64(),
    );
    assert_eq!(updated_validator_bid.validator_public_key(), validator1);
    assert_eq!(updated_validator_bid.staked_amount(), v_updated_stake);
    let total_staked = update
        .get_total_stake(account1_hash)
        .expect("should have total stake");
    assert_eq!(total_staked, v_updated_stake);
    assert!(!updated_validator_bid.inactive());
    // The delegator's bonding purse should be 0'd
    update.assert_written_balance(*delegator_bid.bonding_purse(), 0);

    // 7 keys should be written:
    // - seigniorage recipients
    // - total supply
    // - main purse for validator 1
    // - bonding purse balance for validator 1
    // - bonding purse balance for delegator 1
    // - unbonding for delegator 1
    // - bid for validator 1
    assert_eq!(update.len(), 7);
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

    let expected = U512::from(111);
    let bid1 = update.get_written_bid(account1_hash);
    assert_eq!(bid1.validator_public_key(), validator1);
    assert_eq!(bid1.staked_amount(), expected);

    let total_stake = update
        .get_total_stake(account1_hash)
        .expect("should have total stake");

    assert_eq!(total_stake, expected);
    assert!(!bid1.inactive());

    // check that the validator's bid purse got updated
    update.assert_written_balance(bid1.bonding_purse(), 111);

    let old_bids1 = reader.get_bids();
    let _ = old_bids1
        .validator_bid(&validator1)
        .expect("should have validator1");

    let delegator1_bid = old_bids1
        .delegator_by_public_keys(&validator1, &delegator1)
        .expect("should have delegator1");

    let delegator1_bid_purse = *delegator1_bid.bonding_purse();

    // check that the old delegator's bid purse hasn't been updated
    update.assert_key_absent(&Key::Balance(delegator1_bid_purse.addr()));

    // check that the unbonding purse got created
    update.assert_unbonding_purse(delegator1_bid_purse, &validator1, &delegator1, 13);

    // 6 keys should be written:
    // - seigniorage recipients
    // - total supply
    // - bid for validator 1
    // - bid for delegator 1
    // - bonding purse balance for validator 1
    // - unbonding purse for delegator
    assert_eq!(update.len(), 6);
}

#[test]
fn should_slash_a_validator_and_delegator_with_enqueued_withdraws() {
    let mut rng = TestRng::new();

    let validator1 = PublicKey::random(&mut rng);
    let validator2 = PublicKey::random(&mut rng);
    let delegator1 = PublicKey::random(&mut rng);
    let delegator2 = PublicKey::random(&mut rng);
    let past_delegator1 = PublicKey::random(&mut rng);
    let past_delegator2 = PublicKey::random(&mut rng);

    let amount = U512::one();
    let era_id = EraId::new(1);

    let validator1_config = ValidatorConfig {
        bonded_amount: amount,
        delegation_rate: Some(5),
        delegators: Some(vec![DelegatorConfig {
            public_key: delegator1.clone(),
            delegated_amount: amount,
        }]),
    };

    let mut reader = MockStateReader::new()
        .with_validators(
            vec![
                (validator1.clone(), amount, validator1_config.clone()),
                (
                    validator2.clone(),
                    amount,
                    ValidatorConfig {
                        bonded_amount: amount,
                        delegation_rate: Some(5),
                        delegators: Some(vec![DelegatorConfig {
                            public_key: delegator2.clone(),
                            delegated_amount: amount,
                        }]),
                    },
                ),
            ],
            &mut rng,
        )
        .with_withdraw(
            validator1.clone(),
            validator1.clone(),
            era_id,
            amount,
            &mut rng,
        )
        .with_withdraw(validator1.clone(), delegator1, era_id, amount, &mut rng)
        .with_withdraw(
            validator1.clone(),
            past_delegator1,
            era_id,
            amount,
            &mut rng,
        )
        .with_withdraw(
            validator2.clone(),
            validator2.clone(),
            era_id,
            amount,
            &mut rng,
        )
        .with_withdraw(
            validator2.clone(),
            delegator2.clone(),
            era_id,
            amount,
            &mut rng,
        )
        .with_withdraw(
            validator2.clone(),
            past_delegator2,
            era_id,
            amount,
            &mut rng,
        );

    // we'll be removing validator2
    let config = Config {
        accounts: vec![AccountConfig {
            public_key: validator1.clone(),
            balance: Some(amount),
            validator: Some(validator1_config),
        }],
        only_listed_validators: true,
        slash_instead_of_unbonding: true,
        ..Default::default()
    };

    let update = get_update(&mut reader, config);

    // check that the update contains the correct list of validators
    update.assert_validators(&[ValidatorInfo::new(&validator1, amount * 2)]);

    update.assert_seigniorage_recipients_written(&mut reader);

    // check validator2 slashed
    let old_bids2 = reader.get_bids();
    let old_bid2 = old_bids2
        .validator_bid(&validator2)
        .expect("should have validator2");

    update.assert_written_balance(*old_bid2.bonding_purse(), 0);
    let delegator2 = old_bids2
        .delegator_by_public_keys(&validator2, &delegator2)
        .expect("should have delegator 2");

    // check delegator2 slashed
    update.assert_written_balance(*delegator2.bonding_purse(), 0);
    // check past_delegator2 slashed
    update.assert_written_balance(
        *reader
            .withdraws
            .get(&validator2.to_account_hash())
            .expect("should have withdraws for validator2")
            .last()
            .expect("should have withdraw purses")
            .bonding_purse(),
        0,
    );

    // check validator1 and its delegators not slashed
    for withdraw in reader
        .withdraws
        .get(&validator1.to_account_hash())
        .expect("should have withdraws for validator2")
    {
        update.assert_key_absent(&Key::Balance(withdraw.bonding_purse().addr()));
    }

    // check the withdraws under validator 2 are cleared
    update.assert_withdraws_empty(&validator2);

    // check the withdraws under validator 1 are unchanged
    update.assert_key_absent(&Key::Withdraw(validator1.to_account_hash()));

    // 8 keys should be written:
    // - seigniorage recipients
    // - total supply
    // - 3 balances, 2 bids, 1 withdraw
    assert_eq!(update.len(), 8);
}

#[test]
fn should_slash_a_validator_and_delegator_with_enqueued_unbonds() {
    let mut rng = TestRng::new();

    let (v1_balance, v2_balance) = (100u64, 200u64);
    let (v1_stake, v2_stake, d1_stake, d2_stake) = (1u64, 2u64, 3u64, 4u64);
    let (pd1_stake, pd2_stake) = (10u64, 11u64);

    let validator1 = PublicKey::random(&mut rng);
    let validator2 = PublicKey::random(&mut rng);
    let delegator1 = PublicKey::random(&mut rng);
    let delegator2 = PublicKey::random(&mut rng);

    let past_delegator1 = PublicKey::random(&mut rng);
    let past_delegator2 = PublicKey::random(&mut rng);

    let validator1_config = ValidatorConfig {
        bonded_amount: U512::from(v1_stake),
        delegation_rate: Some(5),
        delegators: Some(vec![DelegatorConfig {
            public_key: delegator1.clone(),
            delegated_amount: U512::from(d1_stake),
        }]),
    };

    let mut reader = MockStateReader::new()
        .with_validators(
            vec![
                (
                    validator1.clone(),
                    v1_balance.into(),
                    validator1_config.clone(),
                ),
                (
                    validator2.clone(),
                    v2_balance.into(),
                    ValidatorConfig {
                        bonded_amount: U512::from(v2_stake),
                        delegation_rate: Some(5),
                        delegators: Some(vec![DelegatorConfig {
                            public_key: delegator2.clone(),
                            delegated_amount: U512::from(d2_stake),
                        }]),
                    },
                ),
            ],
            &mut rng,
        )
        .with_unbond(
            validator1.clone(),
            validator1.clone(),
            v1_stake.into(),
            &mut rng,
        )
        .with_unbond(validator1.clone(), delegator1, d1_stake.into(), &mut rng)
        .with_unbond(
            validator1.clone(),
            past_delegator1,
            pd1_stake.into(),
            &mut rng,
        )
        .with_unbond(
            validator2.clone(),
            validator2.clone(),
            v2_stake.into(),
            &mut rng,
        )
        .with_unbond(
            validator2.clone(),
            delegator2.clone(),
            d2_stake.into(),
            &mut rng,
        )
        .with_unbond(
            validator2.clone(),
            past_delegator2,
            pd2_stake.into(),
            &mut rng,
        );

    // we'll be removing validator2
    let config = Config {
        accounts: vec![AccountConfig {
            public_key: validator1.clone(),
            balance: Some(v1_stake.into()),
            validator: Some(validator1_config),
        }],
        only_listed_validators: true,
        slash_instead_of_unbonding: true,
        ..Default::default()
    };

    let update = get_update(&mut reader, config);

    // check that the update contains the correct list of validators
    update.assert_validators(&[ValidatorInfo::new(
        &validator1,
        U512::from(v1_stake + d1_stake),
    )]);

    update.assert_seigniorage_recipients_written(&mut reader);

    let old_bids = reader.get_bids();
    // check validator2 slashed
    let old_bid2 = old_bids
        .validator_bid(&validator2)
        .expect("should have bid");
    update.assert_written_balance(*old_bid2.bonding_purse(), 0);

    let delegator = old_bids
        .delegator_by_public_keys(&validator2, &delegator2)
        .expect("should have delegator");

    // check delegator2 slashed
    update.assert_written_balance(*delegator.bonding_purse(), 0);
    // check past_delegator2 slashed
    update.assert_written_balance(
        *reader
            .unbonds
            .get(&validator2.to_account_hash())
            .expect("should have unbonds for validator2")
            .last()
            .expect("should have unbond purses")
            .bonding_purse(),
        0,
    );

    // check validator1 and its delegators not slashed
    for unbond in reader
        .unbonds
        .get(&validator1.to_account_hash())
        .expect("should have unbonds for validator2")
    {
        update.assert_key_absent(&Key::Balance(unbond.bonding_purse().addr()));
    }

    // check the unbonds under validator 2 are cleared
    update.assert_unbonds_empty(&validator2);

    // check the withdraws under validator 1 are unchanged
    update.assert_key_absent(&Key::Unbond(validator1.to_account_hash()));

    // 9 keys should be written:
    // - seigniorage recipients
    // - total supply
    // - 4 balances, 2 bids,
    // - 1 unbond
    assert_eq!(update.len(), 9);
}

#[test]
fn should_handle_unbonding_to_oneself_correctly() {
    let rng = &mut TestRng::new();

    let old_validator = PublicKey::random(rng);
    let new_validator = PublicKey::random(rng);

    const OLD_BALANCE: u64 = 31;
    const NEW_BALANCE: u64 = 73;
    const OLD_STAKE: u64 = 97;
    const NEW_STAKE: u64 = 103;

    let mut reader = MockStateReader::new()
        .with_account(old_validator.to_account_hash(), OLD_BALANCE.into(), rng)
        .with_validators(
            vec![(
                old_validator.clone(),
                U512::from(OLD_BALANCE),
                ValidatorConfig {
                    bonded_amount: U512::from(OLD_STAKE),
                    ..Default::default()
                },
            )],
            rng,
        )
        // One token is being unbonded to the validator:
        .with_unbond(
            old_validator.clone(),
            old_validator.clone(),
            OLD_STAKE.into(),
            rng,
        );

    // We'll be updating the validators set to only contain new_validator:
    let config = Config {
        accounts: vec![AccountConfig {
            public_key: new_validator.clone(),
            balance: Some(U512::from(NEW_BALANCE)),
            validator: Some(ValidatorConfig {
                bonded_amount: U512::from(NEW_STAKE),
                ..Default::default()
            }),
        }],
        only_listed_validators: true,
        slash_instead_of_unbonding: false,
        ..Default::default()
    };

    let update = get_update(&mut reader, config);

    // Check that the update contains the correct list of validators
    update.assert_validators(&[ValidatorInfo::new(&new_validator, U512::from(NEW_STAKE))]);

    update.assert_seigniorage_recipients_written(&mut reader);
    update.assert_total_supply(
        &mut reader,
        OLD_BALANCE + OLD_STAKE + NEW_BALANCE + NEW_STAKE,
    );

    // Check purse write for validator1
    let bid_purse = *reader
        .get_bids()
        .validator_bid(&old_validator)
        .expect("should have bid")
        .bonding_purse();

    // Bid purse balance should be unchanged
    update.assert_key_absent(&Key::Balance(bid_purse.addr()));

    // Check bid overwrite
    let account1_hash = old_validator.to_account_hash();
    let mut expected_bid_1 =
        ValidatorBid::unlocked(old_validator, bid_purse, U512::zero(), Default::default());
    expected_bid_1.deactivate();
    update.assert_written_bid(account1_hash, BidKind::Validator(Box::new(expected_bid_1)));

    // Check writes for validator2
    let account2_hash = new_validator.to_account_hash();

    // The new account should be created
    let account2 = update.get_written_addressable_entity(account2_hash);

    // Check that the main purse for the new account has been created with the correct amount
    update.assert_written_purse_is_unit(account2.main_purse());
    update.assert_written_balance(account2.main_purse(), NEW_BALANCE);

    let bid_write = update.get_written_bid(account2_hash);
    assert_eq!(bid_write.validator_public_key(), new_validator);
    let total = update
        .get_total_stake(account2_hash)
        .expect("should read total staked amount");
    assert_eq!(total, U512::from(NEW_STAKE));
    assert!(!bid_write.inactive());

    // Check that the bid purse for the new validator has been created with the correct amount
    update.assert_written_purse_is_unit(bid_write.bonding_purse());
    update.assert_written_balance(bid_write.bonding_purse(), NEW_STAKE);

    // 11 keys should be written:
    // - seigniorage recipients
    // - total supply
    // - bid for old validator
    // - account for new validator
    // - main purse for account for new validator
    // - main purse balance for account for new validator
    // - addressable entity for new validator
    // - package for the newly created addressable entity
    // - bid for new validator
    // - bonding purse for new validator
    // - bonding purse balance for new validator
    assert_eq!(update.len(), 11);
}

#[test]
fn should_handle_unbonding_to_a_delegator_correctly() {
    let rng = &mut TestRng::new();

    let old_validator = PublicKey::random(rng);
    let new_validator = PublicKey::random(rng);
    let delegator = PublicKey::random(rng);

    const OLD_BALANCE: u64 = 100;
    const NEW_BALANCE: u64 = 200;
    const DELEGATOR_BALANCE: u64 = 50;
    const OLD_STAKE: u64 = 1;
    const DELEGATOR_STAKE: u64 = 2;
    const NEW_STAKE: u64 = 3;

    let mut reader = MockStateReader::new()
        .with_account(delegator.to_account_hash(), DELEGATOR_BALANCE.into(), rng)
        .with_account(old_validator.to_account_hash(), OLD_BALANCE.into(), rng)
        .with_validators(
            vec![(
                old_validator.clone(),
                U512::from(OLD_BALANCE),
                ValidatorConfig {
                    bonded_amount: U512::from(OLD_STAKE),
                    delegators: Some(vec![DelegatorConfig {
                        public_key: delegator.clone(),
                        delegated_amount: DELEGATOR_STAKE.into(),
                    }]),
                    ..Default::default()
                },
            )],
            rng,
        )
        // One token is being unbonded to the validator:
        .with_unbond(
            old_validator.clone(),
            old_validator.clone(),
            OLD_STAKE.into(),
            rng,
        )
        // One token is being unbonded to the delegator:
        .with_unbond(
            old_validator.clone(),
            delegator.clone(),
            OLD_STAKE.into(),
            rng,
        );

    // We'll be updating the validators set to only contain new_validator:
    let config = Config {
        accounts: vec![AccountConfig {
            public_key: new_validator.clone(),
            balance: Some(U512::from(NEW_BALANCE)),
            validator: Some(ValidatorConfig {
                bonded_amount: U512::from(NEW_STAKE),
                ..Default::default()
            }),
        }],
        only_listed_validators: true,
        slash_instead_of_unbonding: false,
        ..Default::default()
    };

    let update = get_update(&mut reader, config);

    // Check that the update contains the correct list of validators
    update.assert_validators(&[ValidatorInfo::new(&new_validator, U512::from(NEW_STAKE))]);

    update.assert_seigniorage_recipients_written(&mut reader);
    update.assert_total_supply(
        &mut reader,
        OLD_BALANCE + OLD_STAKE + NEW_BALANCE + NEW_STAKE + DELEGATOR_BALANCE + DELEGATOR_STAKE,
    );

    let unbonding_purses = reader
        .get_unbonds()
        .get(&old_validator.to_account_hash())
        .cloned()
        .expect("should have unbond purses");
    let validator_purse = unbonding_purses
        .iter()
        .find(|&purse| purse.unbonder_public_key() == &old_validator)
        .map(|purse| *purse.bonding_purse())
        .expect("A bonding purse for the validator");
    let _ = unbonding_purses
        .iter()
        .find(|&purse| purse.unbonder_public_key() == &delegator)
        .map(|purse| *purse.bonding_purse())
        .expect("A bonding purse for the delegator");

    // Bid purse balance should be unchanged
    update.assert_key_absent(&Key::Balance(validator_purse.addr()));

    // Check bid overwrite
    let account1_hash = old_validator.to_account_hash();
    let mut expected_bid_1 = ValidatorBid::unlocked(
        old_validator,
        validator_purse,
        U512::zero(),
        Default::default(),
    );
    expected_bid_1.deactivate();
    update.assert_written_bid(account1_hash, BidKind::Validator(Box::new(expected_bid_1)));

    // Check writes for validator2
    let account2_hash = new_validator.to_account_hash();

    // The new account should be created
    let account2 = update.get_written_addressable_entity(account2_hash);

    // Check that the main purse for the new account has been created with the correct amount
    update.assert_written_purse_is_unit(account2.main_purse());
    update.assert_written_balance(account2.main_purse(), NEW_BALANCE);

    let bid_write = update.get_written_bid(account2_hash);
    assert_eq!(bid_write.validator_public_key(), new_validator);
    let total = update
        .get_total_stake(account2_hash)
        .expect("should read total staked amount");
    assert_eq!(total, U512::from(NEW_STAKE));
    assert!(!bid_write.inactive());

    // Check that the bid purse for the new validator has been created with the correct amount
    update.assert_written_purse_is_unit(bid_write.bonding_purse());
    update.assert_written_balance(bid_write.bonding_purse(), NEW_STAKE);

    // 13 keys should be written:
    // - seigniorage recipients
    // - total supply
    // - bid for old validator
    // - bid for delegator
    // - unbonding purse for old validator
    // - account for new validator
    // - main purse for account for new validator
    // - main purse balance for account for new validator
    // - addressable entity for new validator
    // - package for the newly created addressable entity
    // - bid for new validator
    // - bonding purse for new validator
    // - bonding purse balance for new validator
    assert_eq!(update.len(), 13);
}

#[test]
fn should_handle_legacy_unbonding_to_oneself_correctly() {
    let rng = &mut TestRng::new();

    let old_validator = PublicKey::random(rng);
    let new_validator = PublicKey::random(rng);

    const OLD_BALANCE: u64 = 100;
    const NEW_BALANCE: u64 = 200;
    const OLD_STAKE: u64 = 1;
    const NEW_STAKE: u64 = 2;

    let mut reader = MockStateReader::new()
        .with_account(old_validator.to_account_hash(), OLD_BALANCE.into(), rng)
        .with_validators(
            vec![(
                old_validator.clone(),
                U512::from(OLD_BALANCE),
                ValidatorConfig {
                    bonded_amount: U512::from(OLD_STAKE),
                    ..Default::default()
                },
            )],
            rng,
        )
        // Two tokens are being unbonded to the validator, one legacy, the other not:
        .with_unbond(
            old_validator.clone(),
            old_validator.clone(),
            OLD_STAKE.into(),
            rng,
        )
        .with_withdraw(
            old_validator.clone(),
            old_validator.clone(),
            EraId::new(1),
            OLD_STAKE.into(),
            rng,
        );

    // We'll be updating the validators set to only contain new_validator:
    let config = Config {
        accounts: vec![AccountConfig {
            public_key: new_validator.clone(),
            balance: Some(U512::from(NEW_BALANCE)),
            validator: Some(ValidatorConfig {
                bonded_amount: U512::from(NEW_STAKE),
                ..Default::default()
            }),
        }],
        only_listed_validators: true,
        slash_instead_of_unbonding: false,
        ..Default::default()
    };

    let update = get_update(&mut reader, config);

    // Check that the update contains the correct list of validators
    update.assert_validators(&[ValidatorInfo::new(&new_validator, U512::from(NEW_STAKE))]);

    update.assert_seigniorage_recipients_written(&mut reader);
    update.assert_total_supply(
        &mut reader,
        OLD_BALANCE + OLD_STAKE + NEW_BALANCE + NEW_STAKE,
    );

    // Check purse write for validator1
    let bid_purse = *reader
        .get_bids()
        .validator_bid(&old_validator)
        .expect("should have bid")
        .bonding_purse();

    // Bid purse balance should be unchanged
    update.assert_key_absent(&Key::Balance(bid_purse.addr()));

    // Check bid overwrite
    let account1_hash = old_validator.to_account_hash();
    let mut expected_bid_1 =
        ValidatorBid::unlocked(old_validator, bid_purse, U512::zero(), Default::default());
    expected_bid_1.deactivate();
    update.assert_written_bid(account1_hash, BidKind::Validator(Box::new(expected_bid_1)));

    // Check writes for validator2
    let account2_hash = new_validator.to_account_hash();

    // The new account should be created
    let account2 = update.get_written_addressable_entity(account2_hash);

    // Check that the main purse for the new account has been created with the correct amount
    update.assert_written_purse_is_unit(account2.main_purse());
    update.assert_written_balance(account2.main_purse(), NEW_BALANCE);

    let bid_write = update.get_written_bid(account2_hash);
    assert_eq!(bid_write.validator_public_key(), new_validator);
    let total = update
        .get_total_stake(account2_hash)
        .expect("should read total staked amount");
    assert_eq!(total, U512::from(NEW_STAKE));
    assert!(!bid_write.inactive());

    // Check that the bid purse for the new validator has been created with the correct amount
    update.assert_written_purse_is_unit(bid_write.bonding_purse());
    update.assert_written_balance(bid_write.bonding_purse(), NEW_STAKE);

    // 11 keys should be written:
    // - seigniorage recipients
    // - total supply
    // - bid for old validator
    // - account for new validator
    // - main purse for account for new validator
    // - main purse balance for account for new validator
    // - addressable entity for new validator
    // - package for the newly created addressable entity
    // - bid for new validator
    // - bonding purse for new validator
    // - bonding purse balance for new validator
    assert_eq!(update.len(), 11);
}

#[test]
fn should_handle_legacy_unbonding_to_a_delegator_correctly() {
    let rng = &mut TestRng::new();

    let v1_public_key = PublicKey::random(rng);
    let v2_public_key = PublicKey::random(rng);
    let d1_public_key = PublicKey::random(rng);

    const V1_INITIAL_BALANCE: u64 = 100;
    const V1_INITIAL_STAKE: u64 = 1;
    const V2_INITIAL_BALANCE: u64 = 200;
    const V2_INITIAL_STAKE: u64 = 3;

    const D1_INITIAL_BALANCE: u64 = 20;
    const D1_INITIAL_STAKE: u64 = 2;

    const WITHDRAW_ERA: EraId = EraId::new(0);

    let mut reader = MockStateReader::new()
        .with_account(
            d1_public_key.to_account_hash(),
            D1_INITIAL_BALANCE.into(),
            rng,
        )
        .with_account(
            v1_public_key.to_account_hash(),
            V1_INITIAL_BALANCE.into(),
            rng,
        )
        .with_validators(
            vec![(
                v1_public_key.clone(),
                U512::from(V1_INITIAL_BALANCE),
                ValidatorConfig {
                    bonded_amount: U512::from(V1_INITIAL_STAKE),
                    delegators: Some(vec![DelegatorConfig {
                        public_key: d1_public_key.clone(),
                        delegated_amount: D1_INITIAL_STAKE.into(),
                    }]),
                    ..Default::default()
                },
            )],
            rng,
        )
        .with_withdraw(
            v1_public_key.clone(),
            v1_public_key.clone(),
            WITHDRAW_ERA,
            U512::from(V1_INITIAL_STAKE),
            rng,
        )
        // Two tokens are being unbonded to the validator, one legacy, the other not:
        .with_unbond(
            v1_public_key.clone(),
            v1_public_key.clone(),
            U512::from(V1_INITIAL_STAKE),
            rng,
        )
        // Two tokens are being unbonded to the delegator, one legacy, the other not:
        .with_withdraw(
            v1_public_key.clone(),
            d1_public_key.clone(),
            WITHDRAW_ERA,
            U512::from(D1_INITIAL_STAKE),
            rng,
        )
        .with_unbond(
            v1_public_key.clone(),
            d1_public_key,
            U512::from(D1_INITIAL_STAKE),
            rng,
        );

    assert_eq!(
        reader.total_supply.as_u64(),
        V1_INITIAL_BALANCE + V1_INITIAL_STAKE + D1_INITIAL_BALANCE + D1_INITIAL_STAKE,
        "should equal"
    );

    // We'll be updating the validators set to only contain new_validator:
    let config = Config {
        accounts: vec![AccountConfig {
            public_key: v2_public_key.clone(),
            balance: Some(U512::from(V2_INITIAL_BALANCE)),
            validator: Some(ValidatorConfig {
                bonded_amount: U512::from(V2_INITIAL_STAKE),
                ..Default::default()
            }),
        }],
        only_listed_validators: true,
        slash_instead_of_unbonding: false,
        ..Default::default()
    };

    let update = get_update(&mut reader, config);

    update.assert_total_supply(
        &mut reader,
        V1_INITIAL_BALANCE
            + V1_INITIAL_STAKE
            + D1_INITIAL_BALANCE
            + D1_INITIAL_STAKE
            + V2_INITIAL_BALANCE
            + V2_INITIAL_STAKE,
    );

    // Check that the update contains the correct list of validators
    update.assert_validators(&[ValidatorInfo::new(
        &v2_public_key,
        U512::from(V2_INITIAL_STAKE),
    )]);

    let unbonding_purses = reader
        .get_unbonds()
        .get(&v1_public_key.to_account_hash())
        .cloned()
        .expect("should have unbond purses");
    let validator_purse = unbonding_purses
        .iter()
        .find(|&purse| purse.unbonder_public_key() == &v1_public_key)
        .map(|purse| *purse.bonding_purse())
        .expect("A bonding purse for the validator");

    // Bid purse balance should be unchanged
    update.assert_key_absent(&Key::Balance(validator_purse.addr()));

    // Check bid overwrite
    let account1_hash = v1_public_key.to_account_hash();
    let mut expected_bid_1 = ValidatorBid::unlocked(
        v1_public_key,
        validator_purse,
        U512::zero(),
        Default::default(),
    );
    expected_bid_1.deactivate();
    update.assert_written_bid(account1_hash, BidKind::Validator(Box::new(expected_bid_1)));

    // Check writes for validator2
    let account2_hash = v2_public_key.to_account_hash();

    // The new account should be created
    let account2 = update.get_written_addressable_entity(account2_hash);

    // Check that the main purse for the new account has been created with the correct amount
    update.assert_written_purse_is_unit(account2.main_purse());
    update.assert_written_balance(account2.main_purse(), V2_INITIAL_BALANCE);

    let bid_write = update.get_written_bid(account2_hash);
    assert_eq!(bid_write.validator_public_key(), v2_public_key);
    let total = update
        .get_total_stake(account2_hash)
        .expect("should read total staked amount");
    assert_eq!(total, U512::from(V2_INITIAL_STAKE));
    assert!(!bid_write.inactive());

    // Check that the bid purse for the new validator has been created with the correct amount
    update.assert_written_purse_is_unit(bid_write.bonding_purse());
    update.assert_written_balance(bid_write.bonding_purse(), V2_INITIAL_STAKE);

    // 12 keys should be written:
    // - seigniorage recipients
    // - total supply
    // - bid for old validator
    // - unbonding purse for old validator
    // - account for new validator
    // - main purse for account for new validator
    // - main purse balance for account for new validator
    // - addressable entity for new validator
    // - package for the newly created addressable entity
    // - bid for new validator
    // - bonding purse for new validator
    // - bonding purse balance for new validator
    assert_eq!(update.len(), 12);
}
