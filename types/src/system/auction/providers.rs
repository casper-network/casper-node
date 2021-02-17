use crate::{
    account::AccountHash,
    bytesrepr::{FromBytes, ToBytes},
    system::auction::{EraId, EraInfo, Error},
    CLTyped, Key, TransferredTo, URef, BLAKE2B_DIGEST_LENGTH, U512,
};

/// Provider of runtime host functionality.
pub trait RuntimeProvider {
    /// This method should return the caller of the current context.
    fn get_caller(&self) -> AccountHash;

    /// Gets named key under a `name`.
    fn get_key(&self, name: &str) -> Option<Key>;

    /// Returns a 32-byte BLAKE2b digest
    fn blake2b<T: AsRef<[u8]>>(&self, data: T) -> [u8; BLAKE2B_DIGEST_LENGTH];
}

/// Provides functionality of a contract storage.
pub trait StorageProvider {
    /// Reads data from [`URef`].
    fn read<T: FromBytes + CLTyped>(&mut self, uref: URef) -> Result<Option<T>, Error>;

    /// Writes data to [`URef].
    fn write<T: ToBytes + CLTyped>(&mut self, uref: URef, value: T) -> Result<(), Error>;
}

/// Provides functionality of a system module.
pub trait SystemProvider {
    /// Creates new purse.
    fn create_purse(&mut self) -> Result<URef, Error>;

    /// Gets purse balance.
    fn get_balance(&mut self, purse: URef) -> Result<Option<U512>, Error>;

    /// Transfers specified `amount` of tokens from `source` purse into a `target` purse.
    fn transfer_from_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Result<(), Error>;

    /// Records era info at the given era id.
    fn record_era_info(&mut self, era_id: EraId, era_info: EraInfo) -> Result<(), Error>;
}

/// Provides an access to mint.
pub trait MintProvider {
    /// Transfers `amount` from `source` purse to a `target` account.
    fn transfer_purse_to_account(
        &mut self,
        source: URef,
        target: AccountHash,
        amount: U512,
    ) -> Result<TransferredTo, Error>;

    /// Transfers `amount` from `source` purse to a `target` purse.
    fn transfer_purse_to_purse(
        &mut self,
        source: URef,
        target: URef,
        amount: U512,
    ) -> Result<(), Error>;

    /// Checks balance of a `purse`. Returns `None` if given purse does not exist.
    fn balance(&mut self, purse: URef) -> Result<Option<U512>, Error>;

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
