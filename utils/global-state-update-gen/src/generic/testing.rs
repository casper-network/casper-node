use std::collections::BTreeMap;

use rand::{thread_rng, Rng};

use casper_types::{
    account::{Account, AccountHash},
    system::auction::{Bids, SeigniorageRecipientsSnapshot, UnbondingPurses},
    AccessRights, CLValue, Key, StoredValue, URef, URefAddr, U512,
};

use super::{
    config::{Config, Transfer},
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
}

impl MockStateReader {
    fn new() -> Self {
        Self {
            accounts: BTreeMap::new(),
            purses: BTreeMap::new(),
            total_supply: U512::zero(),
            seigniorage_recipients: SeigniorageRecipientsSnapshot::new(),
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
        Bids::new()
    }

    fn get_unbonds(&mut self) -> UnbondingPurses {
        UnbondingPurses::new()
    }
}

#[test]
fn should_transfer_funds() {
    let mut rng = thread_rng();

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
    let mut rng = thread_rng();

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
