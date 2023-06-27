use casper_engine_test_support::LmdbWasmTestBuilder;
use casper_types::{
    account::{Account, AccountHash},
    system::{
        auction::{Bids, UnbondingPurses, WithdrawPurses, SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY},
        mint::TOTAL_SUPPLY_KEY,
    },
    Key, StoredValue,
};

pub trait StateReader {
    fn query(&mut self, key: Key) -> Option<StoredValue>;

    fn get_total_supply_key(&mut self) -> Key;

    fn get_seigniorage_recipients_key(&mut self) -> Key;

    fn get_account(&mut self, account_hash: AccountHash) -> Option<Account>;

    fn get_bids(&mut self) -> Bids;

    fn get_withdraws(&mut self) -> WithdrawPurses;

    fn get_unbonds(&mut self) -> UnbondingPurses;
}

impl<'a, T> StateReader for &'a mut T
where
    T: StateReader,
{
    fn query(&mut self, key: Key) -> Option<StoredValue> {
        T::query(self, key)
    }

    fn get_total_supply_key(&mut self) -> Key {
        T::get_total_supply_key(self)
    }

    fn get_seigniorage_recipients_key(&mut self) -> Key {
        T::get_seigniorage_recipients_key(self)
    }

    fn get_account(&mut self, account_hash: AccountHash) -> Option<Account> {
        T::get_account(self, account_hash)
    }

    fn get_bids(&mut self) -> Bids {
        T::get_bids(self)
    }

    fn get_withdraws(&mut self) -> WithdrawPurses {
        T::get_withdraws(self)
    }

    fn get_unbonds(&mut self) -> UnbondingPurses {
        T::get_unbonds(self)
    }
}

impl StateReader for LmdbWasmTestBuilder {
    fn query(&mut self, key: Key) -> Option<StoredValue> {
        LmdbWasmTestBuilder::query(self, None, key, &[]).ok()
    }

    fn get_total_supply_key(&mut self) -> Key {
        // Find the hash of the mint contract.
        let mint_contract_hash = self.get_system_mint_hash();

        *self
            .get_contract(mint_contract_hash)
            .expect("mint should exist")
            .named_keys()
            .get(TOTAL_SUPPLY_KEY)
            .expect("total_supply should exist in mint named keys")
    }

    fn get_seigniorage_recipients_key(&mut self) -> Key {
        // Find the hash of the auction contract.
        let auction_contract_hash = self.get_system_auction_hash();

        *self
            .get_contract(auction_contract_hash)
            .expect("auction should exist")
            .named_keys()
            .get(SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY)
            .expect("seigniorage_recipients_snapshot should exist in auction named keys")
    }

    fn get_account(&mut self, account_hash: AccountHash) -> Option<Account> {
        LmdbWasmTestBuilder::get_account(self, account_hash)
    }

    fn get_bids(&mut self) -> Bids {
        LmdbWasmTestBuilder::get_bids(self)
    }

    fn get_withdraws(&mut self) -> WithdrawPurses {
        LmdbWasmTestBuilder::get_withdraw_purses(self)
    }

    fn get_unbonds(&mut self) -> UnbondingPurses {
        LmdbWasmTestBuilder::get_unbonds(self)
    }
}
