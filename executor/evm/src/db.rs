//! revm database adapter backed by Casper tracking copy reads.

use casper_storage::{
    data_access_layer::DataAccessLayer,
    eip2935,
    global_state::{error::Error as GlobalStateError, state::StateReader},
    TrackingCopy,
};
use casper_types::{evm, CLValue, EvmAddr, Key, StoredValue, U512};
use revm::{
    database_interface::Database,
    primitives::{Address, Bytes, StorageKey, StorageValue, B256, U256},
    state::{AccountInfo, Bytecode},
};

use crate::{account_state, tx, DbError};

pub(crate) struct CasperDb<'a, R, S>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    data_access_layer: &'a DataAccessLayer<S>,
    tracking_copy: &'a mut TrackingCopy<R>,
    wei_per_mote: u64,
}

impl<'a, R, S> CasperDb<'a, R, S>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    pub(crate) fn new(
        data_access_layer: &'a DataAccessLayer<S>,
        tracking_copy: &'a mut TrackingCopy<R>,
        wei_per_mote: u64,
    ) -> Self {
        Self {
            data_access_layer,
            tracking_copy,
            wei_per_mote,
        }
    }

    fn balance(&mut self, main_purse: casper_types::URef) -> Result<U256, DbError> {
        let key = Key::Balance(main_purse.addr());
        match self.tracking_copy.read(&key)? {
            Some(StoredValue::CLValue(cl_value)) => {
                cl_value_to_u256(key, cl_value, self.wei_per_mote)
            }
            Some(stored_value) => Err(DbError::TypeMismatch {
                key: Box::new(key),
                expected: "StoredValue::CLValue(U512)",
                found: stored_value.type_name(),
            }),
            None => Ok(U256::ZERO),
        }
    }

    fn block_hash_at_height(&self, height: u64) -> Result<B256, DbError> {
        let transaction = self
            .data_access_layer
            .block_store
            .checkout_ro()
            .map_err(|error| DbError::BlockHash { height, error })?;
        let maybe_header = transaction
            .read_block_header_at_height(height)
            .map_err(|error| DbError::BlockHash { height, error })?;
        Ok(maybe_header
            .map(|header| tx::to_revm_block_hash(header.block_hash()))
            .unwrap_or(B256::ZERO))
    }

    /// Executes the native EIP-2935 block hash lookup.
    pub(crate) fn eip2935_get(
        &self,
        input: &[u8],
        block_number: U256,
    ) -> Result<Option<B256>, DbError> {
        if input.len() != evm::HASH_LENGTH {
            return Ok(None);
        }

        let requested_height = U256::from_be_slice(input);
        if requested_height >= block_number
            || block_number - requested_height > U256::from(eip2935::HISTORY_BUFFER_LENGTH)
        {
            return Ok(None);
        }

        let Ok(requested_height) = u64::try_from(requested_height) else {
            return Ok(None);
        };
        self.block_hash_at_height(requested_height).map(Some)
    }
}

impl<R, S> Database for CasperDb<'_, R, S>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    type Error = DbError;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        let address = tx::from_revm_address(address);
        let Some(account) = account_state::read_account_metadata(self.tracking_copy, address)?
        else {
            return Ok(None);
        };
        let balance = self.balance(account.main_purse)?;
        Ok(Some(AccountInfo {
            balance,
            nonce: account.nonce,
            code_hash: tx::to_revm_hash(account.code_hash),
            account_id: None,
            code: None,
        }))
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        let code_hash = tx::from_revm_hash(code_hash);
        let key = Key::Evm(EvmAddr::ByteCode(code_hash));
        match self.tracking_copy.read(&key)? {
            Some(StoredValue::ByteCode(byte_code)) => {
                if !byte_code.kind().is_evm() {
                    return Err(DbError::TypeMismatch {
                        key: Box::new(key),
                        expected: "EVM bytecode kind",
                        found: byte_code.kind().to_string(),
                    });
                }
                Ok(Bytecode::new_raw(Bytes::from(byte_code.take_bytes())))
            }
            Some(stored_value) => Err(DbError::TypeMismatch {
                key: Box::new(key),
                expected: "StoredValue::ByteCode",
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
        let slot = tx::from_revm_storage_word(index);
        let key = Key::Evm(EvmAddr::Storage(evm::StorageAddr::new(address, slot)));
        match self.tracking_copy.read(&key)? {
            Some(StoredValue::CLValue(cl_value)) => cl_value
                .into_t::<casper_types::U256>()
                .map(tx::to_revm_storage_word)
                .map_err(|error| DbError::ValueDecode {
                    key: Box::new(key),
                    expected: "U256",
                    error: error.to_string(),
                }),
            Some(stored_value) => Err(DbError::TypeMismatch {
                key: Box::new(key),
                expected: "StoredValue::CLValue(U256)",
                found: stored_value.type_name(),
            }),
            None => Ok(U256::ZERO),
        }
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        self.block_hash_at_height(number)
    }
}

fn cl_value_to_u256(key: Key, cl_value: CLValue, wei_per_mote: u64) -> Result<U256, DbError> {
    let balance_motes = cl_value
        .into_t::<U512>()
        .map_err(|error| DbError::BalanceDecode {
            key: Box::new(key),
            error: error.to_string(),
        })?;

    let balance_wei = balance_motes
        .checked_mul(U512::from(wei_per_mote))
        .filter(|balance| balance.bits() <= 256)
        .ok_or_else(|| DbError::BalanceOverflow { key: Box::new(key) })?;

    let mut bytes = [0u8; 64];
    balance_wei.to_big_endian(&mut bytes);
    Ok(U256::from_be_slice(&bytes[32..]))
}
