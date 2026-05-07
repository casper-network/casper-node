//! revm database adapter backed by Casper tracking copy reads.

use casper_storage::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    TrackingCopy,
};
use casper_types::{evm, CLValue, Key, StoredValue, U512};
use revm::{
    database_interface::Database,
    primitives::{Address, Bytes, StorageKey, StorageValue, B256, U256},
    state::{AccountInfo, Bytecode},
};

use crate::{tx, BlockHashProvider, DbError};

pub(crate) struct CasperDb<'a, R, B>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
    B: BlockHashProvider + ?Sized,
{
    tracking_copy: &'a mut TrackingCopy<R>,
    block_hash_provider: &'a B,
}

impl<'a, R, B> CasperDb<'a, R, B>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
    B: BlockHashProvider + ?Sized,
{
    pub(crate) fn new(tracking_copy: &'a mut TrackingCopy<R>, block_hash_provider: &'a B) -> Self {
        Self {
            tracking_copy,
            block_hash_provider,
        }
    }

    fn balance(&mut self, main_purse: casper_types::URef) -> Result<U256, DbError> {
        let key = Key::Balance(main_purse.addr());
        match self.tracking_copy.read(&key)? {
            Some(StoredValue::CLValue(cl_value)) => cl_value_to_u256(key, cl_value),
            Some(stored_value) => Err(DbError::TypeMismatch {
                key: Box::new(key),
                expected: "StoredValue::CLValue(U512)",
                found: stored_value.type_name(),
            }),
            None => Ok(U256::ZERO),
        }
    }
}

impl<R, B> Database for CasperDb<'_, R, B>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
    B: BlockHashProvider + ?Sized,
{
    type Error = DbError;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        let address = tx::from_revm_address(address);
        let key = Key::EvmAccount(address);
        match self.tracking_copy.read(&key)? {
            Some(StoredValue::EvmAccount(account)) => {
                let balance = self.balance(account.main_purse())?;
                Ok(Some(AccountInfo {
                    balance,
                    nonce: account.nonce(),
                    code_hash: tx::to_revm_hash(account.code_hash()),
                    account_id: None,
                    code: None,
                }))
            }
            Some(stored_value) => Err(DbError::TypeMismatch {
                key: Box::new(key),
                expected: "StoredValue::EvmAccount",
                found: stored_value.type_name(),
            }),
            None => Ok(None),
        }
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        let code_hash = tx::from_revm_hash(code_hash);
        let key = Key::EvmByteCode(code_hash);
        match self.tracking_copy.read(&key)? {
            Some(StoredValue::EvmByteCode(byte_code)) => {
                Ok(Bytecode::new_raw(Bytes::from(byte_code.into_bytes())))
            }
            Some(stored_value) => Err(DbError::TypeMismatch {
                key: Box::new(key),
                expected: "StoredValue::EvmByteCode",
                found: stored_value.type_name(),
            }),
            None => Ok(Bytecode::default()),
        }
    }

    fn storage(
        &mut self,
        address: Address,
        index: StorageKey,
    ) -> Result<StorageValue, Self::Error> {
        let address = tx::from_revm_address(address);
        let slot = tx::from_revm_u256(index);
        let key = Key::EvmStorage(evm::StorageAddr::new(address, slot));
        match self.tracking_copy.read(&key)? {
            Some(StoredValue::EvmStorage(value)) => Ok(tx::to_revm_hash_word(value.value())),
            Some(stored_value) => Err(DbError::TypeMismatch {
                key: Box::new(key),
                expected: "StoredValue::EvmStorage",
                found: stored_value.type_name(),
            }),
            None => Ok(U256::ZERO),
        }
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        let maybe_block_hash = self
            .block_hash_provider
            .block_hash(number)
            .map_err(|error| DbError::BlockHash {
                height: number,
                error,
            })?;
        Ok(maybe_block_hash
            .map(tx::to_revm_block_hash)
            .unwrap_or(B256::ZERO))
    }
}

fn cl_value_to_u256(key: Key, cl_value: CLValue) -> Result<U256, DbError> {
    let balance = cl_value
        .into_t::<U512>()
        .map_err(|error| DbError::BalanceDecode {
            key: Box::new(key),
            error: error.to_string(),
        })?;

    if balance.bits() > 256 {
        return Err(DbError::BalanceOverflow { key: Box::new(key) });
    }

    let mut bytes = [0u8; 64];
    balance.to_big_endian(&mut bytes);
    Ok(U256::from_be_slice(&bytes[32..]))
}
