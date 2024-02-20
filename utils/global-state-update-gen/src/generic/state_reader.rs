use casper_engine_test_support::LmdbWasmTestBuilder;
use casper_types::{
    account::AccountHash,
    contracts::ContractHash,
    system::{
        auction::{BidKind, UnbondingPurses, WithdrawPurses, SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY},
        mint::TOTAL_SUPPLY_KEY,
    },
    AddressableEntity, Key, ProtocolVersion, StoredValue,
};

pub trait StateReader {
    fn query(&mut self, key: Key) -> Option<StoredValue>;

    fn get_total_supply_key(&mut self) -> Key;

    fn get_seigniorage_recipients_key(&mut self) -> Key;

    fn get_account(&mut self, account_hash: AccountHash) -> Option<AddressableEntity>;

    fn get_bids(&mut self) -> Vec<BidKind>;

    fn get_withdraws(&mut self) -> WithdrawPurses;

    fn get_unbonds(&mut self) -> UnbondingPurses;

    fn get_protocol_version(&mut self) -> ProtocolVersion;
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

    fn get_account(&mut self, account_hash: AccountHash) -> Option<AddressableEntity> {
        T::get_account(self, account_hash)
    }

    fn get_bids(&mut self) -> Vec<BidKind> {
        T::get_bids(self)
    }

    fn get_withdraws(&mut self) -> WithdrawPurses {
        T::get_withdraws(self)
    }

    fn get_unbonds(&mut self) -> UnbondingPurses {
        T::get_unbonds(self)
    }

    fn get_protocol_version(&mut self) -> ProtocolVersion {
        T::get_protocol_version(self)
    }
}

impl StateReader for LmdbWasmTestBuilder {
    fn query(&mut self, key: Key) -> Option<StoredValue> {
        LmdbWasmTestBuilder::query(self, None, key, &[]).ok()
    }

    fn get_total_supply_key(&mut self) -> Key {
        // Find the hash of the mint contract.
        let mint_contract_hash = self.get_system_mint_hash();

        if let Some(entity) = self.get_entity_with_named_keys_by_entity_hash(mint_contract_hash) {
            entity
                .named_keys()
                .get(TOTAL_SUPPLY_KEY)
                .copied()
                .expect("total_supply should exist in mint named keys")
        } else {
            let mint_legacy_contract_hash: ContractHash =
                ContractHash::new(mint_contract_hash.value());

            self.get_legacy_contract(mint_legacy_contract_hash)
                .expect("mint should exist")
                .named_keys()
                .get(TOTAL_SUPPLY_KEY)
                .copied()
                .expect("total_supply should exist in mint named keys")
        }
    }

    fn get_seigniorage_recipients_key(&mut self) -> Key {
        // Find the hash of the auction contract.
        let auction_contract_hash = self.get_system_auction_hash();

        if let Some(entity) = self.get_entity_with_named_keys_by_entity_hash(auction_contract_hash)
        {
            entity
                .named_keys()
                .get(SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY)
                .copied()
                .expect("seigniorage_recipients_snapshot should exist in auction named keys")
        } else {
            let auction_legacy_contract_hash = ContractHash::new(auction_contract_hash.value());

            self.get_legacy_contract(auction_legacy_contract_hash)
                .expect("auction should exist")
                .named_keys()
                .get(SEIGNIORAGE_RECIPIENTS_SNAPSHOT_KEY)
                .copied()
                .expect("seigniorage_recipients_snapshot should exist in auction named keys")
        }
    }

    fn get_account(&mut self, account_hash: AccountHash) -> Option<AddressableEntity> {
        LmdbWasmTestBuilder::get_entity_by_account_hash(self, account_hash)
    }

    fn get_bids(&mut self) -> Vec<BidKind> {
        LmdbWasmTestBuilder::get_bids(self)
    }

    fn get_withdraws(&mut self) -> WithdrawPurses {
        LmdbWasmTestBuilder::get_withdraw_purses(self)
    }

    fn get_unbonds(&mut self) -> UnbondingPurses {
        LmdbWasmTestBuilder::get_unbonds(self)
    }

    fn get_protocol_version(&mut self) -> ProtocolVersion {
        let mint_contract_hash = self.get_system_mint_hash();

        let mint_legacy_contract_hash: ContractHash = ContractHash::new(mint_contract_hash.value());

        if let Some(entity) = self.get_addressable_entity(mint_contract_hash) {
            entity.protocol_version()
        } else {
            self.get_legacy_contract(mint_legacy_contract_hash)
                .expect("mint should exist")
                .protocol_version()
        }
    }
}
