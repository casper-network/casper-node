use casperlabs_types::{
    auction::{
        AuctionProvider, DataProvider, DelegationProvider, MintProvider, RuntimeProvider, StorageProvider, SystemProvider,
    },
    bytesrepr::{FromBytes, ToBytes},
    runtime_args,
    system_contract_errors::auction::Error,
    CLTyped, CLValue, Key, PublicKey, RuntimeArgs, URef, U512,
};

use super::Runtime;
use crate::components::contract_runtime::{
    core::execution, shared::stored_value::StoredValue, storage::global_state::StateReader,
};

impl<'a, R> StorageProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    type Error = Error;

    fn get_key(&mut self, name: &str) -> Option<Key> {
        self.context.named_keys_get(name).cloned()
    }

    fn read<T: FromBytes + CLTyped>(&mut self, uref: URef) -> Result<Option<T>, Self::Error> {
        match self.context.read_gs(&uref.into()) {
            Ok(Some(StoredValue::CLValue(cl_value))) => {
                Ok(Some(cl_value.into_t().map_err(|_| Error::Storage)?))
            }
            Ok(Some(_)) => Err(Error::Storage),
            Ok(None) => Ok(None),
            Err(execution::Error::BytesRepr(_)) => Err(Error::Serialization),
            Err(_) => Err(Error::Storage),
        }
    }

    fn write<T: ToBytes + CLTyped>(&mut self, uref: URef, value: T) -> Result<(), Self::Error> {
        let cl_value = CLValue::from_t(value).unwrap();
        self.context
            .write_gs(uref.into(), StoredValue::CLValue(cl_value))
            .map_err(|_| Error::Storage)
    }
}

// pub fn read_from<P, T>(provider: &mut P, name: &str) -> Result<T, Error>
// where
//     P: StorageProvider + ?Sized,
//     T: FromBytes + CLTyped,
//     Error: From<P::Error>,
// {
//     let key = provider.get_key(name).ok_or(Error::MissingKey)?;
//     let uref = key.into_uref().ok_or(Error::InvalidKeyVariant)?;
//     let value: T = provider.read(uref)?.ok_or(Error::MissingValue)?;
//     Ok(value)
// }

// pub fn write_to<P, T>(provider: &mut P, name: &str, value: T) -> Result<(), Error>
// where
//     P: StorageProvider + ?Sized,
//     T: ToBytes + CLTyped,
//     Error: From<P::Error>,
// {
//     let key = provider.get_key(name).ok_or(Error::MissingKey)?;
//     let uref = key.into_uref().ok_or(Error::InvalidKeyVariant)?;
//     provider.write(uref, value)?;
//     Ok(())
// }

impl<'a, R> DataProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    type Error = Error;
}
//     fn get_delegations_map(&mut self) -> Result<DelegationsMap, Self::Error> {
//         read_from(self, DELEGATIONS_MAP_KEY)
//     }
//     fn set_delegations_map(&mut self, delegations_map: DelegationsMap) -> Result<(), Self::Error> {
//         write_to(self, DELEGATIONS_MAP_KEY, delegations_map)
//     }
//     fn get_tally_map(&mut self) -> Result<TallyMap, Self::Error> {
//         read_from(self, TALLY_MAP_KEY)
//     }
//     fn set_tally_map(&mut self, tally_map: TallyMap) -> Result<(), Self::Error> {
//         write_to(self, TALLY_MAP_KEY, tally_map)
//     }
//     fn get_reward_per_stake_map(&mut self) -> Result<RewardPerStakeMap, Self::Error> {
//         read_from(self, REWARD_PER_STAKE_MAP_KEY)
//     }
//     fn set_reward_per_stake_map(
//         &mut self,
//         reward_per_stake_map: RewardPerStakeMap,
//     ) -> Result<(), Self::Error> {
//         write_to(self, REWARD_PER_STAKE_MAP_KEY, reward_per_stake_map)
//     }
//     fn get_total_delegator_stake_map(&mut self) -> Result<TotalDelegatorStakeMap, Self::Error> {
//         read_from(self, TOTAL_DELEGATOR_STAKE_MAP_KEY)
//     }
//     fn set_total_delegator_stake_map(
//         &mut self,
//         total_delegator_stake_map: TotalDelegatorStakeMap,
//     ) -> Result<(), Self::Error> {
//         write_to(
//             self,
//             TOTAL_DELEGATOR_STAKE_MAP_KEY,
//             total_delegator_stake_map,
//         )
//     }
//     fn get_delegator_reward_pool_map(&mut self) -> Result<DelegatorRewardPoolMap, Self::Error> {
//         read_from(self, DELEGATOR_REWARD_POOL_MAP)
//     }
//     fn set_delegator_reward_pool_map(
//         &mut self,
//         delegator_reward_pool_map: DelegatorRewardPoolMap,
//     ) -> Result<(), Self::Error> {
//         write_to(self, DELEGATOR_REWARD_POOL_MAP, delegator_reward_pool_map)
//     }
// }

impl<'a, R> DelegationProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    type Error = Error;

}
//     fn delegate(
//         &mut self,
//         delegator_account_hash: casperlabs_types::account::AccountHash,
//         source_purse: URef,
//         validator_account_hash: casperlabs_types::account::AccountHash,
//         delegation_amount: U512,
//     ) -> Result<(URef, U512), Self::Error> {
//         todo!()
//     }
//     fn undelegate(
//         &mut self,
//         delegator_account_hash: casperlabs_types::account::AccountHash,
//         validator_account_hash: casperlabs_types::account::AccountHash,
//         quantity: U512,
//     ) -> Result<U512, Self::Error> {
//         todo!()
//     }
//     fn distribute_to_delegators(
//         &mut self,
//         validator_account_hash: casperlabs_types::account::AccountHash,
//         purse: URef,
//     ) -> Result<(), Self::Error> {
//         todo!()
//     }
//     fn delegation_reward(
//         &mut self,
//         validator_account_hash: casperlabs_types::account::AccountHash,
//         delegator_account_hash: casperlabs_types::account::AccountHash,
//     ) -> Result<U512, Self::Error> {
//         todo!()
//     }
//     fn withdraw_reward(
//         &mut self,
//         validator_account_hash: casperlabs_types::account::AccountHash,
//         delegator_account_hash: casperlabs_types::account::AccountHash,
//         purse: URef,
//     ) -> Result<U512, Self::Error> {
//         todo!()
//     }

// }

impl<'a, R> SystemProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    type Error = Error;

    fn create_purse(&mut self) -> URef {
        Runtime::create_purse(self).unwrap()
    }

    fn get_balance(&mut self, purse: URef) -> Result<Option<U512>, Self::Error> {
        Runtime::get_balance(self, purse).map_err(|_| Error::GetBalance)
    }

    fn transfer_from_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Result<(), Self::Error> {
        let mint_contract_hash = self.get_mint_contract();
        self.mint_transfer(mint_contract_hash, source, target, amount)
            .map_err(|_| Error::Transfer)
    }
}

impl<'a, R> MintProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    type Error = Error;

    fn bond(
        &mut self,
        public_key: PublicKey,
        amount: U512,
        purse: URef,
    ) -> Result<(URef, U512), Self::Error> {
        const ARG_AMOUNT: &str = "amount";
        const ARG_PURSE: &str = "purse";
        const ARG_PUBLIC_KEY: &str = "public_key";

        let args_values: RuntimeArgs = runtime_args! {
            ARG_AMOUNT => amount,
            ARG_PURSE => purse,
            ARG_PUBLIC_KEY => public_key,
        };

        let mint_contract_hash = self.get_mint_contract();

        let result = self
            .call_contract(mint_contract_hash, "bond", args_values)
            .map_err(|_| Error::Bonding)?;
        Ok(result.into_t().map_err(|_| Error::Bonding)?)
    }

    fn unbond(&mut self, public_key: PublicKey, amount: U512) -> Result<(URef, U512), Self::Error> {
        const ARG_AMOUNT: &str = "amount";
        const ARG_PUBLIC_KEY: &str = "public_key";

        let args_values: RuntimeArgs = runtime_args! {
            ARG_AMOUNT => amount,
            ARG_PUBLIC_KEY => public_key,
        };

        let mint_contract_hash = self.get_mint_contract();

        let result = self
            .call_contract(mint_contract_hash, "unbond", args_values)
            .map_err(|_| Error::Unbonding)?;
        Ok(result.into_t().map_err(|_| Error::Unbonding)?)
    }
}

impl<'a, R> RuntimeProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
    fn get_caller(&self) -> casperlabs_types::account::AccountHash {
        self.context.get_caller()
    }
}

impl<'a, R> AuctionProvider for Runtime<'a, R>
where
    R: StateReader<Key, StoredValue>,
    R::Error: Into<execution::Error>,
{
}
