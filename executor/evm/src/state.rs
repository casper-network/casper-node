//! Translation from revm state changes into Casper tracking copy writes.

use std::collections::{btree_map::Entry, BTreeMap};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BalanceLosses {
    pub(crate) evm_burn_wei: U512,
    pub(crate) rounding_loss_wei: U512,
}

pub(crate) fn apply<R>(
    tracking_copy: &mut TrackingCopy<R>,
    state: EvmState,
    wei_per_mote: u64,
) -> Result<BalanceLosses, Error>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    let (mut balance_motes_by_address, balance_losses) =
        resolve_balances(tracking_copy, &state, wei_per_mote)?;

    for (address, account) in state {
        let balance_motes = balance_motes_by_address.remove(&address).ok_or_else(|| {
            Error::State(format!(
                "missing resolved EVM balance for changed account {address:?}"
            ))
        })?;
        apply_account(tracking_copy, address, account, balance_motes)?;
    }
    Ok(balance_losses)
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
                StoredValue::ByteCode(ByteCode::new(ByteCodeKind::EvmOsaka, bytes.to_vec())),
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
        prune_existing_key(tracking_copy, key)?;
    }
    if !matches!(identity, Some(account_state::AccountIdentity::Account(_))) {
        prune_existing_key(tracking_copy, Key::Balance(main_purse.addr()))?;
    }
    for key in [
        account_key,
        Key::Evm(EvmAddr::Nonce(address)),
        Key::Evm(EvmAddr::CodeHash(address)),
    ] {
        prune_existing_key(tracking_copy, key)?;
    }
    Ok(())
}

fn prune_existing_key<R>(tracking_copy: &mut TrackingCopy<R>, key: Key) -> Result<(), Error>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    // A contract created and destroyed in one execution may never have had
    // persisted account records. Scratch state rejects pruning absent keys.
    if tracking_copy
        .read(&key)
        .map_err(|error| Error::State(error.to_string()))?
        .is_some()
    {
        tracking_copy.prune(key);
    }
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

fn read_balance<R>(tracking_copy: &mut TrackingCopy<R>, key: Key) -> Result<U512, Error>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    match tracking_copy
        .read(&key)
        .map_err(|error| Error::State(error.to_string()))?
    {
        Some(StoredValue::CLValue(cl_value)) => cl_value
            .into_t::<U512>()
            .map_err(|error| Error::State(format!("failed to decode balance at {key}: {error}"))),
        Some(stored_value) => Err(Error::State(format!(
            "unexpected balance at {key}: expected CLValue(U512), found {}",
            stored_value.type_name()
        ))),
        None => Ok(U512::zero()),
    }
}

fn resolve_balances<R>(
    tracking_copy: &mut TrackingCopy<R>,
    state: &EvmState,
    wei_per_mote: u64,
) -> Result<(AddressMap<U512>, BalanceLosses), Error>
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    if wei_per_mote == 0 {
        return Err(Error::InvalidWeiPerMote);
    }

    let wei_per_mote = U512::from(wei_per_mote);
    let mut balances = AddressMap::with_capacity_and_hasher(state.len(), Default::default());
    // Compare original Casper purse balances with final EVM balances to find
    // burns, then account separately for wei discarded by rounding. Both
    // components must stay in wei until their sum is converted to motes.
    // Key by purse so shared purses are counted once and updates follow the
    // same account iteration order as apply_account.
    let mut purse_balances = BTreeMap::<Key, (U512, U512)>::new();
    for (address, account) in state {
        let balance_wei = u256_to_u512(account.info.balance);
        let balance_motes = balance_wei / wei_per_mote;
        balances.insert(*address, balance_motes);

        let address = tx::from_revm_address(*address);
        let identity = account_state::read_account_identity(tracking_copy, address)?;
        let main_purse = existing_main_purse(tracking_copy, address, identity)?;
        let balance_key = Key::Balance(main_purse.addr());
        let (_, final_balance_wei) = match purse_balances.entry(balance_key) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let original_balance = read_balance(tracking_copy, balance_key)?
                    .checked_mul(wei_per_mote)
                    .ok_or_else(|| {
                        Error::State(format!("original EVM purse balance at {balance_key} overflowed U512 when scaled to wei"))
                    })?;
                entry.insert((original_balance, original_balance))
            }
        };

        if account.is_selfdestructed() {
            // prune_account leaves linked Casper main purses intact. Pruned
            // EVM-native purses retain no value, even if revm reports a balance.
            if !matches!(identity, Some(account_state::AccountIdentity::Account(_))) {
                *final_balance_wei = U512::zero();
            }
        } else {
            *final_balance_wei = balance_wei;
        }
    }

    let mut original_total_wei = U512::zero();
    let mut final_total_wei = U512::zero();
    let mut rounding_loss_wei = U512::zero();
    for (original_balance_wei, final_balance_wei) in purse_balances.values() {
        original_total_wei = original_total_wei
            .checked_add(*original_balance_wei)
            .ok_or_else(|| {
                Error::State("aggregate original EVM purse balance overflowed U512 wei".to_string())
            })?;
        final_total_wei = final_total_wei
            .checked_add(*final_balance_wei)
            .ok_or_else(|| {
                Error::State("aggregate final EVM purse balance overflowed U512 wei".to_string())
            })?;
        rounding_loss_wei = rounding_loss_wei
            .checked_add(*final_balance_wei % wei_per_mote)
            .ok_or_else(|| {
                Error::State("aggregate EVM rounding loss overflowed U512 wei".to_string())
            })?;
    }
    let evm_burn_wei = original_total_wei
        .checked_sub(final_total_wei)
        .ok_or_else(|| {
            Error::State(format!(
                "EVM purse balances increased from {original_total_wei} to {final_total_wei} wei"
            ))
        })?;
    let total_loss_wei = evm_burn_wei.checked_add(rounding_loss_wei).ok_or_else(|| {
        Error::State("aggregate EVM balance loss overflowed U512 wei".to_string())
    })?;
    if total_loss_wei % wei_per_mote != U512::zero() {
        return Err(Error::State(format!(
            "aggregate EVM balance loss {total_loss_wei} wei is not divisible by \
             {wei_per_mote} wei per mote"
        )));
    }

    Ok((
        balances,
        BalanceLosses {
            evm_burn_wei,
            rounding_loss_wei,
        },
    ))
}

#[cfg(test)]
mod tests {
    use casper_storage::global_state::state::{
        lmdb::{make_temporary_global_state, LmdbGlobalStateView},
        StateProvider,
    };
    use revm::state::AccountInfo;

    use super::*;

    fn tracking_copy(balances: &[u64]) -> (TrackingCopy<LmdbGlobalStateView>, impl Send) {
        let (global_state, state_root_hash, tempdir) = make_temporary_global_state([]);
        let reader = global_state.checkout(state_root_hash).unwrap().unwrap();
        let mut tracking_copy = TrackingCopy::new(reader, 5, false);
        for (index, balance) in balances.iter().enumerate() {
            let mut address = [0u8; 20];
            address[19] = u8::try_from(index).unwrap();
            let purse = evm::deterministic_purse(evm::Address::new(address));
            tracking_copy.write(
                Key::Balance(purse.addr()),
                StoredValue::CLValue(CLValue::from_t(U512::from(*balance)).unwrap()),
            );
        }
        (tracking_copy, tempdir)
    }

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
    fn should_account_for_rounding_across_all_purses() {
        let (mut tracking_copy, _tempdir) = tracking_copy(&[1, 0, 0]);
        let state = state_with_balances(&[8, 1, 1]);

        let (balances, balance_losses) = resolve_balances(&mut tracking_copy, &state, 10)
            .expect("aggregate rounding should resolve");

        assert!(balances.values().all(U512::is_zero));
        assert_eq!(balance_losses.evm_burn_wei, U512::zero());
        assert_eq!(balance_losses.rounding_loss_wei, U512::from(10u64));
    }

    #[test]
    fn should_account_for_burn_and_non_divisible_remainder() {
        let (mut tracking_copy, _tempdir) = tracking_copy(&[1]);
        let state = state_with_balances(&[1]);

        let (balances, balance_losses) = resolve_balances(&mut tracking_copy, &state, 10)
            .expect("burn and rounding should resolve together");

        assert!(balances.values().all(U512::is_zero));
        assert_eq!(balance_losses.evm_burn_wei, U512::from(9u64));
        assert_eq!(balance_losses.rounding_loss_wei, U512::one());
    }

    #[test]
    fn should_account_for_whole_mote_burn_without_remainder() {
        let (mut tracking_copy, _tempdir) = tracking_copy(&[3]);
        let state = state_with_balances(&[0]);

        let (_, balance_losses) = resolve_balances(&mut tracking_copy, &state, 10)
            .expect("whole-mote burn should resolve");

        assert_eq!(balance_losses.evm_burn_wei, U512::from(30u64));
        assert_eq!(balance_losses.rounding_loss_wei, U512::zero());
    }

    #[test]
    fn should_count_shared_purse_once() {
        let (mut tracking_copy, _tempdir) = tracking_copy(&[1]);
        let state = state_with_balances(&[0, 0]);
        let purse = evm::deterministic_purse(evm::Address::ZERO);
        for address in state.keys() {
            account_state::write_account_identity(
                &mut tracking_copy,
                tx::from_revm_address(*address),
                Key::URef(purse),
            )
            .unwrap();
        }

        let balance_losses =
            apply(&mut tracking_copy, state, 10).expect("shared purse should resolve once");

        assert_eq!(balance_losses.evm_burn_wei, U512::from(10u64));
        assert_eq!(balance_losses.rounding_loss_wei, U512::zero());
    }

    #[test]
    fn should_reject_whole_and_fractional_balance_increase_before_writing_state() {
        for final_balance_wei in [1, 10] {
            let (mut tracking_copy, _tempdir) = tracking_copy(&[0]);
            let state = state_with_balances(&[final_balance_wei]);
            let (original_writes, original_prunes, _) = tracking_copy.fork2().destructure();

            assert!(matches!(
                apply(&mut tracking_copy, state, 10),
                Err(Error::State(message))
                    if message == format!("EVM purse balances increased from 0 to {final_balance_wei} wei")
            ));
            let (writes, prunes, _) = tracking_copy.destructure();
            assert_eq!(writes, original_writes);
            assert_eq!(prunes, original_prunes);
        }
    }
}
