use alloc::{collections::BTreeSet, vec::Vec};

use crate::{
    account::AccountHash,
    bytesrepr::{FromBytes, ToBytes},
    system::{
        auction::{Bid, EraId, EraInfo, Error, UnbondingPurse},
        mint, CallStackElement,
    },
    CLTyped, Key, KeyTag, URef, BLAKE2B_DIGEST_LENGTH, U512,
};

/// Provider of runtime host functionality.
pub trait RuntimeProvider {
    /// This method should return the caller of the current context.
    fn get_caller(&self) -> AccountHash;

    /// This method should return the immediate caller of the current call.
    fn get_immediate_caller(&self) -> Option<&CallStackElement>;

    /// Gets named key under a `name`.
    fn named_keys_get(&self, name: &str) -> Option<Key>;

    /// Gets keys in a given keyspace
    fn get_keys(&mut self, key_tag: &KeyTag) -> Result<BTreeSet<Key>, Error>;

    /// Returns a 32-byte BLAKE2b digest
    fn blake2b<T: AsRef<[u8]>>(&self, data: T) -> [u8; BLAKE2B_DIGEST_LENGTH];
}

/// Provides functionality of a contract storage.
pub trait StorageProvider {
    /// Reads data from [`URef`].
    fn read<T: FromBytes + CLTyped>(&mut self, uref: URef) -> Result<Option<T>, Error>;

    /// Writes data to [`URef].
    fn write<T: ToBytes + CLTyped>(&mut self, uref: URef, value: T) -> Result<(), Error>;

    /// Reads [`Bid`] at account hash derived from given public key
    fn read_bid(&mut self, account_hash: &AccountHash) -> Result<Option<Bid>, Error>;

    /// Writes given [`Bid`] at account hash derived from given public key
    fn write_bid(&mut self, account_hash: AccountHash, bid: Bid) -> Result<(), Error>;

    /// Reads collection of [`UnbondingPurse`]s at account hash derived from given public key
    fn read_withdraw(&mut self, account_hash: &AccountHash) -> Result<Vec<UnbondingPurse>, Error>;

    /// Writes given [`UnbondingPurse`]s at account hash derived from given public key
    fn write_withdraw(
        &mut self,
        account_hash: AccountHash,
        unbonding_purses: Vec<UnbondingPurse>,
    ) -> Result<(), Error>;

    /// Records era info at the given era id.
    fn record_era_info(&mut self, era_id: EraId, era_info: EraInfo) -> Result<(), Error>;
}

/// Provides an access to mint.
pub trait MintProvider {
    /// Returns successfully unbonded stake to origin account.
    fn unbond(&mut self, unbonding_purse: &UnbondingPurse) -> Result<(), Error>;

    /// Allows optimized auction and mint interaction.
    /// Intended to be used only by system contracts to manage staked purses.
    fn mint_transfer_direct(
        &mut self,
        to: Option<AccountHash>,
        source: URef,
        target: URef,
        amount: U512,
        id: Option<u64>,
    ) -> Result<Result<(), mint::Error>, Error>;

    /// Creates new purse.
    fn create_purse(&mut self) -> Result<URef, Error>;

    /// Gets purse balance.
    fn get_balance(&mut self, purse: URef) -> Result<Option<U512>, Error>;

    /// Reads the base round reward.
    fn read_base_round_reward(&mut self) -> Result<U512, Error>;

    /// Mints new token with given `initial_balance` balance. Returns new purse on success,
    /// otherwise an error.
    fn mint(&mut self, amount: U512) -> Result<URef, Error>;

    /// Reduce total supply by `amount`. Returns unit on success, otherwise
    /// an error.
    fn reduce_total_supply(&mut self, amount: U512) -> Result<(), Error>;
}

/// Provider of an account related functionality.
pub trait AccountProvider {
    /// Get currently executing account's purse.
    fn get_main_purse(&self) -> Result<URef, Error>;
}
