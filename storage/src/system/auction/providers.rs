use std::collections::BTreeSet;

use casper_types::{
    account::AccountHash,
    bytesrepr::{FromBytes, ToBytes},
    system::{
        auction::{BidAddr, BidKind, EraInfo, Error, UnbondingPurse},
        mint,
    },
    CLTyped, Key, KeyTag, URef, BLAKE2B_DIGEST_LENGTH, U512,
};

/// Provider of runtime host functionality.
pub trait RuntimeProvider {
    /// This method should return the caller of the current context.
    fn get_caller(&self) -> AccountHash;

    /// Checks if account_hash matches the active session's account.
    fn is_allowed_session_caller(&self, account_hash: &AccountHash) -> bool;

    /// Gets named key under a `name`.
    fn named_keys_get(&self, name: &str) -> Option<Key>;

    /// Gets keys in a given keyspace
    fn get_keys(&mut self, key_tag: &KeyTag) -> Result<BTreeSet<Key>, Error>;

    /// Gets keys by prefix.
    fn get_keys_by_prefix(&mut self, prefix: &[u8]) -> Result<Vec<Key>, Error>;

    /// Returns the current number of delegators for this validator.
    fn delegator_count(&mut self, bid_addr: &BidAddr) -> Result<usize, Error>;

    /// Returns a 32-byte BLAKE2b digest
    fn blake2b<T: AsRef<[u8]>>(&self, data: T) -> [u8; BLAKE2B_DIGEST_LENGTH];

    /// Returns vesting schedule period.
    fn vesting_schedule_period_millis(&self) -> u64;

    /// Check if auction bids are allowed.
    fn allow_auction_bids(&self) -> bool;

    /// Check if auction should compute rewards.
    fn should_compute_rewards(&self) -> bool;
}

/// Provides functionality of a contract storage.
pub trait StorageProvider {
    /// Reads data from [`URef`].
    fn read<T: FromBytes + CLTyped>(&mut self, uref: URef) -> Result<Option<T>, Error>;

    /// Writes data to [`URef].
    fn write<T: ToBytes + CLTyped>(&mut self, uref: URef, value: T) -> Result<(), Error>;

    /// Reads [`casper_types::system::auction::Bid`] at account hash derived from given public key
    fn read_bid(&mut self, key: &Key) -> Result<Option<BidKind>, Error>;

    /// Writes given [`BidKind`] at given key.
    fn write_bid(&mut self, key: Key, bid_kind: BidKind) -> Result<(), Error>;

    /// Reads collection of [`UnbondingPurse`]s at account hash derived from given public key
    fn read_unbonds(&mut self, account_hash: &AccountHash) -> Result<Vec<UnbondingPurse>, Error>;

    /// Writes given [`UnbondingPurse`]s at account hash derived from given public key
    fn write_unbonds(
        &mut self,
        account_hash: AccountHash,
        unbonding_purses: Vec<UnbondingPurse>,
    ) -> Result<(), Error>;

    /// Records era info.
    fn record_era_info(&mut self, era_info: EraInfo) -> Result<(), Error>;

    /// Prunes a given bid at [`BidAddr`].
    fn prune_bid(&mut self, bid_addr: BidAddr);
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

    /// Mint `amount` new token into `existing_purse`.
    /// Returns unit on success, otherwise an error.
    fn mint_into_existing_purse(&mut self, amount: U512, existing_purse: URef)
        -> Result<(), Error>;

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
