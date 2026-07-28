//! Translation from revm state changes into Casper tracking copy writes.

use casper_storage::{
    global_state::{error::Error as GlobalStateError, state::StateReader},
    KeyPrefix, TrackingCopy,
};
use casper_types::{evm, ByteCode, ByteCodeKind, CLValue, EvmAddr, Key, StoredValue, U512};
use revm::{
    primitives::{Address, AddressMap, U256},
    state::{Account, EvmState},
};

use crate::{account_state, tx, Error};

pub(crate) fn apply<R>(
    tracking_copy: &mut TrackingCopy<R>,
    state: EvmState,
    wei_per_mote: u64,
) -> Result<U512, Error>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    let (mut balance_motes_by_address, dust_motes) = resolve_balances(&state, wei_per_mote)?;

    for (address, account) in state {
        let balance_motes = balance_motes_by_address.remove(&address).ok_or_else(|| {
            Error::State(format!(
                "missing resolved EVM balance for changed account {address:?}"
            ))
        })?;
        apply_account(tracking_copy, address, account, balance_motes)?;
    }
    Ok(dust_motes)
}

pub(crate) struct DisabledFeeTransfers {
    pub caller: Address,
    pub caller_reimbursement: U256,
    pub beneficiary: Address,
    pub beneficiary_reward: U256,
}

pub(crate) fn remove_disabled_fee_transfers(
    state: &mut EvmState,
    transfers: DisabledFeeTransfers,
) -> Result<(), Error> {
    subtract_balance(state, transfers.caller, transfers.caller_reimbursement)?;
    subtract_balance(state, transfers.beneficiary, transfers.beneficiary_reward)
}

fn subtract_balance(state: &mut EvmState, address: Address, amount: U256) -> Result<(), Error> {
    if amount.is_zero() {
        return Ok(());
    }
    let account = state.get_mut(&address).ok_or_else(|| {
        Error::State(format!(
            "missing EVM account {address:?} while removing disabled fee transfer"
        ))
    })?;
    account.info.balance = account.info.balance.checked_sub(amount).ok_or_else(|| {
        Error::State(format!(
            "EVM account {address:?} balance underflow while removing disabled fee transfer"
        ))
    })?;
    Ok(())
}

fn apply_account<R>(
    tracking_copy: &mut TrackingCopy<R>,
    address: Address,
    account: Account,
    balance_motes: U512,
) -> Result<(), Error>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    // Check how to deal with Key::Balance after selfdestruct
    let address = tx::from_revm_address(address);
    let account_key = Key::Evm(EvmAddr::Account(address));

    if account.is_selfdestructed() {
        // Selfdestruct removes EVM metadata and storage, but linked Casper
        // accounts remain Casper accounts. Only EVM-native purse balances are
        // pruned below.
        let identity = account_state::read_account_identity(tracking_copy, address)?;
        let main_purse = existing_main_purse(tracking_copy, address, identity)?;
        prune_account(tracking_copy, address, account_key, identity, main_purse)?;
        return Ok(());
    }

    if let Some(code) = account.info.code.as_ref() {
        let bytes = code.original_byte_slice();
        if !bytes.is_empty() {
            tracking_copy.write(
                Key::Evm(EvmAddr::ByteCode(tx::from_revm_hash(
                    account.info.code_hash,
                ))),
                StoredValue::ByteCode(ByteCode::new(ByteCodeKind::EvmPrague, bytes.to_vec())),
            );
        }
    }

    let identity = account_state::read_account_identity(tracking_copy, address)?;
    let main_purse = existing_main_purse(tracking_copy, address, identity)?;
    let code_hash = tx::from_revm_hash(account.info.code_hash);

    // Executor never creates a `Key::Account` bridge. Runtime applies that
    // policy before execution. For accounts without such a bridge, ensure revm
    // state changes have an EVM-native purse identity to attach balances to.
    if !matches!(identity, Some(account_state::AccountIdentity::Account(_))) {
        account_state::write_account_identity(tracking_copy, address, Key::URef(main_purse))?;
    }
    account_state::write_nonce(tracking_copy, address, account.info.nonce)?;
    account_state::write_code_hash(tracking_copy, address, code_hash)?;
    write_balance(tracking_copy, main_purse, balance_motes)?;

    for (slot, value) in account.changed_storage_slots() {
        let key = Key::Evm(EvmAddr::Storage(evm::StorageAddr::new(
            address,
            tx::from_revm_storage_word(*slot),
        )));
        // Storage slots are plain CLValue(U256) under their split storage key.
        // Zero writes prune the slot, matching Ethereum's empty-storage model.
        if value.present_value.is_zero() {
            tracking_copy.prune(key);
        } else {
            let storage_value = CLValue::from_t(tx::from_revm_storage_word(value.present_value))
                .map_err(|error| Error::State(error.to_string()))?;
            tracking_copy.write(key, StoredValue::CLValue(storage_value));
        }
    }

    Ok(())
}

fn prune_account<R>(
    tracking_copy: &mut TrackingCopy<R>,
    address: evm::Address,
    account_key: Key,
    identity: Option<account_state::AccountIdentity>,
    main_purse: casper_types::URef,
) -> Result<(), Error>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    let storage_keys = tracking_copy
        .get_keys_by_prefix(&KeyPrefix::EvmStorageByAddress(address))
        .map_err(|error| Error::State(error.to_string()))?;
    for key in storage_keys {
        tracking_copy.prune(key);
    }
    if !matches!(identity, Some(account_state::AccountIdentity::Account(_))) {
        tracking_copy.prune(Key::Balance(main_purse.addr()));
    }
    tracking_copy.prune(account_key);
    tracking_copy.prune(Key::Evm(EvmAddr::Nonce(address)));
    tracking_copy.prune(Key::Evm(EvmAddr::CodeHash(address)));
    Ok(())
}

fn existing_main_purse<R>(
    tracking_copy: &mut TrackingCopy<R>,
    address: evm::Address,
    identity: Option<account_state::AccountIdentity>,
) -> Result<casper_types::URef, Error>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    match identity {
        // Linked accounts use their Casper main purse. If the linked account is
        // unexpectedly missing, fall back to the deterministic purse so pruning
        // stays local to EVM state rather than deleting unrelated balances.
        Some(account_state::AccountIdentity::Account(account_hash)) => Ok(
            account_state::account_main_purse(tracking_copy, account_hash)?
                .unwrap_or_else(|| evm::deterministic_purse(address)),
        ),
        // EVM-native accounts and contracts keep balances under the identity
        // purse chosen by runtime/native-transfer initialization.
        Some(account_state::AccountIdentity::Purse(main_purse)) => Ok(main_purse),
        None => Ok(evm::deterministic_purse(address)),
    }
}

fn write_balance<R>(
    tracking_copy: &mut TrackingCopy<R>,
    main_purse: casper_types::URef,
    balance_motes: U512,
) -> Result<(), Error>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    let cl_value =
        CLValue::from_t(balance_motes).map_err(|error| Error::State(error.to_string()))?;
    tracking_copy.write(
        Key::Balance(main_purse.addr()),
        StoredValue::CLValue(cl_value),
    );
    Ok(())
}

fn u256_to_u512(value: U256) -> U512 {
    let bytes = value.to_be_bytes::<32>();
    U512::from_big_endian(&bytes)
}

fn resolve_balances(
    state: &EvmState,
    wei_per_mote: u64,
) -> Result<(AddressMap<U512>, U512), Error> {
    if wei_per_mote == 0 {
        return Err(Error::InvalidWeiPerMote);
    }

    let wei_per_mote = U512::from(wei_per_mote);
    let mut balances = AddressMap::with_capacity_and_hasher(state.len(), Default::default());
    let mut aggregate_remainder_wei = U512::zero();
    for (address, account) in state {
        let balance_wei = u256_to_u512(account.info.balance);
        balances.insert(*address, balance_wei / wei_per_mote);
        aggregate_remainder_wei = aggregate_remainder_wei
            .checked_add(balance_wei % wei_per_mote)
            .ok_or_else(|| {
                Error::State("aggregate EVM balance remainder overflowed U512".to_string())
            })?;
    }

    if aggregate_remainder_wei % wei_per_mote != U512::zero() {
        return Err(Error::State(format!(
            "aggregate EVM balance remainder {aggregate_remainder_wei} wei is not divisible by \
             {wei_per_mote} wei per mote"
        )));
    }

    Ok((balances, aggregate_remainder_wei / wei_per_mote))
}

#[cfg(test)]
mod tests {
    use revm::state::AccountInfo;

    use super::*;

    fn state_with_balances(balances: &[u64]) -> EvmState {
        balances
            .iter()
            .enumerate()
            .map(|(index, balance)| {
                let mut address = [0u8; 20];
                address[19] = u8::try_from(index).unwrap();
                (
                    Address::from(address),
                    AccountInfo {
                        balance: U256::from(*balance),
                        ..Default::default()
                    }
                    .into(),
                )
            })
            .collect()
    }

    #[test]
    fn should_sum_all_remainders_before_converting_to_dust_motes() {
        let state = state_with_balances(&[8, 1, 1]);

        let (balances, dust_motes) =
            resolve_balances(&state, 10).expect("aggregate remainder should resolve");

        assert!(balances.values().all(U512::is_zero));
        assert_eq!(dust_motes, U512::one());
    }

    #[test]
    fn should_reject_non_divisible_aggregate_remainder() {
        let state = state_with_balances(&[1]);

        assert!(matches!(
            resolve_balances(&state, 10),
            Err(Error::State(message))
                if message.contains("aggregate EVM balance remainder 1 wei is not divisible")
        ));
    }
}
