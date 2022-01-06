#![no_std]

extern crate alloc;

use alloc::{vec, vec::Vec};
use casper_contract::{
    contract_api::{self, runtime, storage, system},
    ext_ffi,
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    account::AccountHash,
    api_error, bytesrepr,
    bytesrepr::{FromBytes, ToBytes},
    ApiError, BlockTime, CLTyped, CLValue, Key, URef, U512,
};
use core::{cmp::Ordering, mem::MaybeUninit};
use num::rational::Ratio;

const ARG_TARGET: &str = "target";
const ARG_ID: &str = "id";
const ARG_TIME_INTERVAL: &str = "time_interval";
const ARG_AVAILABLE_AMOUNT: &str = "available_amount";
const ARG_DISTRIBUTIONS_PER_INTERVAL: &str = "distributions_per_interval";
const REMAINING_AMOUNT: &str = "remaining_amount";
const AVAILABLE_AMOUNT: &str = "available_amount";
const TIME_INTERVAL: &str = "time_interval";
const DISTRIBUTIONS_PER_INTERVAL: &str = "distributions_per_interval";
const LAST_DISTRIBUTION_TIME: &str = "last_distribution_time";
const FAUCET_PURSE: &str = "faucet_purse";
const INSTALLER: &str = "installer";

#[repr(u16)]
enum FaucetError {
    InvalidAccount = 1,
    MissingInstaller = 2,
    InvalidInstaller = 3,
    InstallerDoesNotFundItself = 4,
    InvalidAmount = 5,
    MissingDistributionTime = 6,
    InvalidDistributionTime = 7,
    MissingAvailableAmount = 8,
    InvalidAvailableAmount = 9,
    MissingTimeInterval = 10,
    InvalidTimeInterval = 11,
    MissingId = 12,
    InvalidId = 13,
    FailedToTransfer = 14,
    FailedToGetArgBytes = 15,
    FailedToConstructReturnData = 16,
    MissingFaucetPurse = 17,
    InvalidFaucetPurse = 18,
    MissingRemainingAmount = 19,
    InvalidRemainingAmount = 20,
    MissingDistributionsPerInterval = 21,
    InvalidDistributionsPerInterval = 22,
    UnexpectedKeyVariant = 23,
}

impl From<FaucetError> for ApiError {
    fn from(e: FaucetError) -> Self {
        ApiError::User(e as u16)
    }
}

#[no_mangle]
pub fn init() {
    let installer = get_account_hash_with_user_errors(
        INSTALLER,
        FaucetError::MissingInstaller,
        FaucetError::InvalidInstaller,
    );

    if installer != runtime::get_caller() {
        runtime::revert(FaucetError::InvalidAccount);
    }

    runtime::put_key(TIME_INTERVAL, storage::new_uref(7_200_000u64).into());
    runtime::put_key(LAST_DISTRIBUTION_TIME, storage::new_uref(0u64).into());
    runtime::put_key(AVAILABLE_AMOUNT, storage::new_uref(U512::zero()).into());
    runtime::put_key(REMAINING_AMOUNT, storage::new_uref(U512::zero()).into());
    // 20 blocks x 25 deploys = 500
    runtime::put_key(DISTRIBUTIONS_PER_INTERVAL, storage::new_uref(500u64).into());

    let purse = system::create_purse();

    runtime::put_key(FAUCET_PURSE, purse.into());
    runtime::ret(
        CLValue::from_t(purse).unwrap_or_revert_with(FaucetError::FailedToConstructReturnData),
    )
}

#[no_mangle]
pub fn set_variables() {
    let installer = get_account_hash_with_user_errors(
        INSTALLER,
        FaucetError::MissingInstaller,
        FaucetError::InvalidInstaller,
    );

    if installer != runtime::get_caller() {
        runtime::revert(FaucetError::InvalidAccount);
    }

    let new_time_interval: u64 = runtime::get_named_arg(ARG_TIME_INTERVAL);
    let time_interval_uref = get_uref_with_user_errors(
        TIME_INTERVAL,
        FaucetError::MissingTimeInterval,
        FaucetError::InvalidTimeInterval,
    );
    storage::write(time_interval_uref, new_time_interval);

    let new_available_amount: U512 = runtime::get_named_arg(ARG_AVAILABLE_AMOUNT);
    let available_amount_uref = get_uref_with_user_errors(
        AVAILABLE_AMOUNT,
        FaucetError::MissingAvailableAmount,
        FaucetError::InvalidAvailableAmount,
    );
    storage::write(available_amount_uref, new_available_amount);

    let remaining_amount_uref = get_uref_with_user_errors(
        REMAINING_AMOUNT,
        FaucetError::MissingRemainingAmount,
        FaucetError::InvalidRemainingAmount,
    );
    storage::write(remaining_amount_uref, new_available_amount);

    let new_distributions_per_interval: u64 =
        runtime::get_named_arg(ARG_DISTRIBUTIONS_PER_INTERVAL);
    let distributions_per_interval_uref = get_uref_with_user_errors(
        DISTRIBUTIONS_PER_INTERVAL,
        FaucetError::MissingDistributionsPerInterval,
        FaucetError::InvalidDistributionsPerInterval,
    );
    storage::write(
        distributions_per_interval_uref,
        new_distributions_per_interval,
    );
}

/// Executes token transfer to supplied account hash.
/// Revert status codes:
/// 1 - requested transfer to already funded account hash.
#[no_mangle]
pub fn delegate() {
    let id = get_optional_named_arg_with_user_errors(
        ARG_ID,
        FaucetError::MissingId,
        FaucetError::InvalidId,
    );

    let caller = runtime::get_caller();
    let installer = get_account_hash_with_user_errors(
        INSTALLER,
        FaucetError::MissingInstaller,
        FaucetError::InvalidInstaller,
    );

    let last_distribution_time_uref = get_uref_with_user_errors(
        LAST_DISTRIBUTION_TIME,
        FaucetError::MissingDistributionTime,
        FaucetError::InvalidDistributionTime,
    );

    let last_distribution_time: u64 = read_with_user_errors(
        last_distribution_time_uref,
        FaucetError::MissingDistributionTime,
        FaucetError::InvalidDistributionTime,
    );

    let time_interval_uref = get_uref_with_user_errors(
        TIME_INTERVAL,
        FaucetError::MissingTimeInterval,
        FaucetError::InvalidTimeInterval,
    );

    let time_interval: u64 = read_with_user_errors(
        time_interval_uref,
        FaucetError::MissingTimeInterval,
        FaucetError::InvalidTimeInterval,
    );

    let blocktime = runtime::get_blocktime();

    if blocktime > BlockTime::new(last_distribution_time + time_interval) {
        reset_remaining_amount();
    }

    let available_amount: U512 = get_available_amount();

    if caller == installer {
        // this is the installer / faucet account creating a NEW account
        // or topping off an existing account
        let target: AccountHash = runtime::get_named_arg(ARG_TARGET);
        transfer(target, available_amount, id);
    } else {
        transfer(caller, available_amount, id);
    }

    set_last_distribution_time(blocktime);
}

fn transfer(target: AccountHash, amount: U512, id: Option<u64>) -> U512 {
    let faucet_purse = get_uref_with_user_errors(
        FAUCET_PURSE,
        FaucetError::MissingFaucetPurse,
        FaucetError::InvalidFaucetPurse,
    );

    system::transfer_from_purse_to_account(faucet_purse, target, amount, id)
        .unwrap_or_revert_with(FaucetError::FailedToTransfer);
    decrease_remaining_amount(amount)
}

fn get_available_amount() -> U512 {
    let available_amount_uref = get_uref_with_user_errors(
        AVAILABLE_AMOUNT,
        FaucetError::MissingAvailableAmount,
        FaucetError::InvalidAvailableAmount,
    );

    let available_amount: U512 = read_with_user_errors(
        available_amount_uref,
        FaucetError::MissingAvailableAmount,
        FaucetError::InvalidAvailableAmount,
    );

    let remaining_amount_uref = get_uref_with_user_errors(
        REMAINING_AMOUNT,
        FaucetError::MissingRemainingAmount,
        FaucetError::InvalidRemainingAmount,
    );

    let remaining_amount: U512 = read_with_user_errors(
        remaining_amount_uref,
        FaucetError::MissingRemainingAmount,
        FaucetError::InvalidRemainingAmount,
    );

    let distributions_per_interval_uref = get_uref_with_user_errors(
        DISTRIBUTIONS_PER_INTERVAL,
        FaucetError::MissingDistributionsPerInterval,
        FaucetError::InvalidDistributionsPerInterval,
    );

    let distributions_per_interval: u64 = read_with_user_errors(
        distributions_per_interval_uref,
        FaucetError::MissingDistributionsPerInterval,
        FaucetError::InvalidDistributionsPerInterval,
    );

    let distribution_amount =
        Ratio::new(available_amount, U512::from(distributions_per_interval)).to_integer();

    match remaining_amount.cmp(&distribution_amount) {
        Ordering::Equal | Ordering::Greater => distribution_amount,
        Ordering::Less => remaining_amount,
    }
}

fn reset_remaining_amount() {
    let available_amount_uref = get_uref_with_user_errors(
        AVAILABLE_AMOUNT,
        FaucetError::MissingAvailableAmount,
        FaucetError::InvalidAvailableAmount,
    );

    let available_amount: U512 = read_with_user_errors(
        available_amount_uref,
        FaucetError::MissingAvailableAmount,
        FaucetError::InvalidAvailableAmount,
    );

    let remaining_amount_uref = get_uref_with_user_errors(
        REMAINING_AMOUNT,
        FaucetError::MissingRemainingAmount,
        FaucetError::InvalidRemainingAmount,
    );

    storage::write(remaining_amount_uref, available_amount);
}

fn decrease_remaining_amount(amount: U512) -> U512 {
    let remaining_amount_uref = get_uref_with_user_errors(
        REMAINING_AMOUNT,
        FaucetError::MissingRemainingAmount,
        FaucetError::InvalidRemainingAmount,
    );

    let remaining_amount: U512 = read_with_user_errors(
        remaining_amount_uref,
        FaucetError::MissingRemainingAmount,
        FaucetError::InvalidRemainingAmount,
    );

    let new_remaining_amount = remaining_amount.saturating_sub(amount);
    storage::write(remaining_amount_uref, new_remaining_amount);

    new_remaining_amount
}

fn set_last_distribution_time(t: BlockTime) {
    let last_distribution_time_uref = get_uref_with_user_errors(
        LAST_DISTRIBUTION_TIME,
        FaucetError::MissingDistributionTime,
        FaucetError::InvalidDistributionTime,
    );

    storage::write::<u64>(last_distribution_time_uref, t.into());
}

fn get_named_arg_size(name: &str) -> Option<usize> {
    let mut arg_size: usize = 0;
    let ret = unsafe {
        ext_ffi::casper_get_named_arg_size(
            name.as_bytes().as_ptr(),
            name.len(),
            &mut arg_size as *mut usize,
        )
    };
    match api_error::result_from(ret) {
        Ok(_) => Some(arg_size),
        Err(ApiError::MissingArgument) => None,
        Err(e) => runtime::revert(e),
    }
}

fn get_optional_named_arg_with_user_errors<T: FromBytes>(
    name: &str,
    missing: FaucetError,
    invalid: FaucetError,
) -> Option<T> {
    match get_named_arg_with_user_errors(name, missing, invalid) {
        Ok(val) => val,
        Err(err @ FaucetError::InvalidId) => runtime::revert(err),
        Err(_) => None,
    }
}

fn get_named_arg_with_user_errors<T: FromBytes>(
    name: &str,
    missing: FaucetError,
    invalid: FaucetError,
) -> Result<T, FaucetError> {
    let arg_size = get_named_arg_size(name).ok_or(missing)?;
    let arg_bytes = if arg_size > 0 {
        let res = {
            let data_non_null_ptr = contract_api::alloc_bytes(arg_size);
            let ret = unsafe {
                ext_ffi::casper_get_named_arg(
                    name.as_bytes().as_ptr(),
                    name.len(),
                    data_non_null_ptr.as_ptr(),
                    arg_size,
                )
            };
            let data =
                unsafe { Vec::from_raw_parts(data_non_null_ptr.as_ptr(), arg_size, arg_size) };
            api_error::result_from(ret).map(|_| data)
        };
        // Assumed to be safe as `get_named_arg_size` checks the argument already
        res.unwrap_or_revert_with(FaucetError::FailedToGetArgBytes)
    } else {
        // Avoids allocation with 0 bytes and a call to get_named_arg
        Vec::new()
    };

    bytesrepr::deserialize(arg_bytes).map_err(|_| invalid)
}

fn get_account_hash_with_user_errors(
    name: &str,
    missing: FaucetError,
    invalid: FaucetError,
) -> AccountHash {
    let key = get_key_with_user_errors(name, missing, invalid);
    key.into_account()
        .unwrap_or_revert_with(FaucetError::UnexpectedKeyVariant)
}

fn get_uref_with_user_errors(name: &str, missing: FaucetError, invalid: FaucetError) -> URef {
    let key = get_key_with_user_errors(name, missing, invalid);
    key.into_uref()
        .unwrap_or_revert_with(FaucetError::UnexpectedKeyVariant)
}

fn get_key_with_user_errors(name: &str, missing: FaucetError, invalid: FaucetError) -> Key {
    let (name_ptr, name_size, _bytes) = to_ptr(name);
    let mut key_bytes = vec![0u8; Key::max_serialized_length()];
    let mut total_bytes: usize = 0;
    let ret = unsafe {
        ext_ffi::casper_get_key(
            name_ptr,
            name_size,
            key_bytes.as_mut_ptr(),
            key_bytes.len(),
            &mut total_bytes as *mut usize,
        )
    };
    match api_error::result_from(ret) {
        Ok(_) => {}
        Err(ApiError::MissingKey) => runtime::revert(missing),
        Err(e) => runtime::revert(e),
    }
    key_bytes.truncate(total_bytes);

    bytesrepr::deserialize(key_bytes).unwrap_or_revert_with(invalid)
}

fn read_with_user_errors<T: CLTyped + FromBytes>(
    uref: URef,
    missing: FaucetError,
    invalid: FaucetError,
) -> T {
    let key: Key = uref.into();
    let (key_ptr, key_size, _bytes) = to_ptr(key);

    let value_size = {
        let mut value_size = MaybeUninit::uninit();
        let ret = unsafe { ext_ffi::casper_read_value(key_ptr, key_size, value_size.as_mut_ptr()) };
        match api_error::result_from(ret) {
            Ok(_) => unsafe { value_size.assume_init() },
            Err(ApiError::ValueNotFound) => runtime::revert(missing),
            Err(e) => runtime::revert(e),
        }
    };

    let value_bytes = read_host_buffer(value_size).unwrap_or_revert();

    bytesrepr::deserialize(value_bytes).unwrap_or_revert_with(invalid)
}

fn read_host_buffer_into(dest: &mut [u8]) -> Result<usize, ApiError> {
    let mut bytes_written = MaybeUninit::uninit();
    let ret = unsafe {
        ext_ffi::casper_read_host_buffer(dest.as_mut_ptr(), dest.len(), bytes_written.as_mut_ptr())
    };
    // NOTE: When rewriting below expression as `result_from(ret).map(|_| unsafe { ... })`, and the
    // caller ignores the return value, execution of the contract becomes unstable and ultimately
    // leads to `Unreachable` error.
    api_error::result_from(ret)?;
    Ok(unsafe { bytes_written.assume_init() })
}

fn read_host_buffer(size: usize) -> Result<Vec<u8>, ApiError> {
    let mut dest: Vec<u8> = if size == 0 {
        Vec::new()
    } else {
        let bytes_non_null_ptr = contract_api::alloc_bytes(size);
        unsafe { Vec::from_raw_parts(bytes_non_null_ptr.as_ptr(), size, size) }
    };
    read_host_buffer_into(&mut dest)?;
    Ok(dest)
}

fn to_ptr<T: ToBytes>(t: T) -> (*const u8, usize, Vec<u8>) {
    let bytes = t.into_bytes().unwrap_or_revert();
    let ptr = bytes.as_ptr();
    let size = bytes.len();
    (ptr, size, bytes)
}
