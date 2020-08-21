use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};

use casperlabs_types::{
    account::AccountHash,
    proof_of_stake::{MintProvider, ProofOfStake, RuntimeProvider, Stakes, StakesProvider},
    system_contract_errors::pos::Error,
    ApiError, BlockTime, Key, Phase, TransferredTo, URef, U512,
};

use crate::components::contract_runtime::{
    core::{execution, runtime::Runtime},
    shared::stored_value::StoredValue,
    storage::global_state::StateReader,
};

// TODO: Update MintProvider to better handle errors
impl<'a, R> MintProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn transfer_purse_to_account(
        &mut self,
        source: URef,
        target: AccountHash,
        amount: U512,
    ) -> Result<TransferredTo, ApiError> {
        self.transfer_from_purse_to_account(source, target, amount)
            .expect("should transfer from purse to account")
    }

    fn transfer_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Result<(), ()> {
        let mint_contract_key = self.get_mint_contract();
        if self
            .mint_transfer(mint_contract_key, source, target, amount)
            .is_ok()
        {
            Ok(())
        } else {
            Err(())
        }
    }

    fn balance(&mut self, purse: URef) -> Option<U512> {
        self.get_balance(purse).expect("should get balance")
    }
}

// TODO: Update RuntimeProvider to better handle errors
impl<'a, R> RuntimeProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn get_key(&self, name: &str) -> Option<Key> {
        self.context.named_keys_get(name).cloned()
    }

    fn put_key(&mut self, name: &str, key: Key) {
        self.context
            .put_key(name.to_string(), key)
            .expect("should put key")
    }

    fn remove_key(&mut self, name: &str) {
        self.context.remove_key(name).expect("should remove key")
    }

    fn get_phase(&self) -> Phase {
        self.context.phase()
    }

    fn get_block_time(&self) -> BlockTime {
        self.context.get_blocktime()
    }

    fn get_caller(&self) -> AccountHash {
        self.context.get_caller()
    }
}

impl<'a, R> StakesProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn read(&self) -> Result<Stakes, Error> {
        let mut stakes = BTreeMap::new();
        for (name, _) in self.context.named_keys().iter() {
            let mut split_name = name.split('_');
            if Some("v") != split_name.next() {
                continue;
            }
            let hex_key = split_name
                .next()
                .ok_or(Error::StakesKeyDeserializationFailed)?;
            if hex_key.len() != 64 {
                return Err(Error::StakesKeyDeserializationFailed);
            }
            let mut key_bytes = [0u8; 32];
            let _bytes_written = base16::decode_slice(hex_key, &mut key_bytes)
                .map_err(|_| Error::StakesKeyDeserializationFailed)?;
            debug_assert!(_bytes_written == key_bytes.len());
            let pub_key = AccountHash::new(key_bytes);
            let balance = split_name
                .next()
                .and_then(|b| U512::from_dec_str(b).ok())
                .ok_or(Error::StakesDeserializationFailed)?;
            stakes.insert(pub_key, balance);
        }
        if stakes.is_empty() {
            return Err(Error::StakesNotFound);
        }
        Ok(Stakes(stakes))
    }

    fn write(&mut self, stakes: &Stakes) {
        // Encode the stakes as a set of uref names.
        let mut new_urefs: BTreeSet<String> = stakes
            .0
            .iter()
            .map(|(pub_key, balance)| {
                let key_bytes = pub_key.value();
                let mut hex_key = String::with_capacity(64);
                for byte in &key_bytes[..32] {
                    write!(hex_key, "{:02x}", byte).expect("Writing to a string cannot fail");
                }
                let mut uref = String::new();
                uref.write_fmt(format_args!("v_{}_{}", hex_key, balance))
                    .expect("Writing to a string cannot fail");
                uref
            })
            .collect();
        // Remove and add urefs to update the contract's known urefs accordingly.
        let mut removes = Vec::new();
        for (name, _) in self.context.named_keys().iter() {
            if name.starts_with("v_") && !new_urefs.remove(name) {
                removes.push(name.to_owned())
            }
        }
        for name in removes.iter() {
            self.context.remove_key(name).expect("should remove key")
        }
        for name in new_urefs {
            self.context
                .put_key(name, Key::Hash([0; 32]))
                .expect("should put key")
        }
    }
}

impl<'a, R> ProofOfStake for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
}
