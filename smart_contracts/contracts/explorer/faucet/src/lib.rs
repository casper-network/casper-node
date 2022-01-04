#![no_std]

extern crate alloc;

use alloc::{format, vec::Vec};
use casper_contract::{
    contract_api::{self, runtime, storage, system},
    ext_ffi,
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    account::AccountHash, api_error, bytesrepr, bytesrepr::FromBytes, ApiError, BlockTime, CLValue,
    U512,
};
use core::cmp::Ordering;
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
const LAST_DISTRIBUTION: &str = "last_distribution";
const FAUCET_PURSE: &str = "faucet_purse";
const INSTALLER: &str = "installer";

#[repr(u16)]
enum FaucetError {
    InvalidAccount = 1,
    InstallerMissing = 2,
    InstallerInvalid = 3,
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
}

impl From<FaucetError> for ApiError {
    fn from(e: FaucetError) -> Self {
        ApiError::User(e as u16)
    }
}
/// Executes token transfer to supplied account hash.
/// Revert status codes:
/// 1 - requested transfer to already funded account hash.
#[no_mangle]
pub fn delegate() {
    // if available_amount.is_zero() {
    //     // or revert with a "faucet exhausted" error.
    //     runtime::revert(999);
    // }

    // let id: Option<u64> = get_optional_named_arg_with_user_errors(
    //     ARG_ID,
    //     CustomError::MissingId,
    //     CustomError::InvalidId,
    // );

    let id: Option<u64> = runtime::get_named_arg(ARG_ID);

    let caller = runtime::get_caller();
    let installer = runtime::get_key(INSTALLER)
        .unwrap_or_revert_with(FaucetError::InstallerMissing)
        .into_account()
        .unwrap_or_revert_with(FaucetError::InstallerInvalid);

    let last_distribution_time: u64 = storage::read(
        runtime::get_key(LAST_DISTRIBUTION)
            .unwrap_or_revert_with(FaucetError::MissingDistributionTime)
            .into_uref()
            .unwrap_or_revert_with(FaucetError::InvalidDistributionTime),
    )
    .unwrap_or_revert_with(FaucetError::MissingDistributionTime)
    .unwrap_or_revert_with(FaucetError::InvalidDistributionTime);

    let time_interval: u64 = storage::read(
        runtime::get_key(TIME_INTERVAL)
            .unwrap_or_revert_with(FaucetError::MissingTimeInterval)
            .into_uref()
            .unwrap_or_revert_with(FaucetError::InvalidTimeInterval),
    )
    .unwrap_or_revert_with(FaucetError::MissingTimeInterval)
    .unwrap_or_revert_with(FaucetError::InvalidTimeInterval);

    let blocktime = runtime::get_blocktime();

    if blocktime > BlockTime::new(last_distribution_time + time_interval) {
        reset_remaining_amount();
    }

    let available_amount: U512 = get_available_amount();

    // let amount = {
    if caller == installer {
        // this is the installer / faucet account creating a NEW account
        // or topping off an existing account
        let target: AccountHash = runtime::get_named_arg(ARG_TARGET);
        transfer(target, available_amount, id);
    } else {
        // this is an unknown existing account asking for top off
        transfer(caller, available_amount, id);
    }

    set_last_distribution_time(blocktime);
    // };
    // return amount granted as a courtesy to the caller
    //
    // runtime::ret(
    //     CLValue::from_t(amount)
    //         .unwrap_or_revert_with(ApiError::User(CustomError::InvalidAmount as u16)),
    // )
}

fn transfer(target: AccountHash, amount: U512, id: Option<u64>) -> U512 {
    let faucet_purse = runtime::get_key(FAUCET_PURSE)
        .unwrap_or_revert_with(FaucetError::MissingFaucetPurse)
        .into_uref()
        .unwrap_or_revert_with(FaucetError::InvalidFaucetPurse);
    system::transfer_from_purse_to_account(faucet_purse, target, amount, id)
        .unwrap_or_revert_with(FaucetError::FailedToTransfer);
    decrease_remaining_amount(amount)
}

fn get_available_amount() -> U512 {
    let available_amount_uref = runtime::get_key(AVAILABLE_AMOUNT)
        .unwrap_or_revert_with(FaucetError::MissingAvailableAmount)
        .into_uref()
        .unwrap_or_revert_with(FaucetError::MissingAvailableAmount);

    let available_amount: U512 = storage::read(available_amount_uref)
        .unwrap_or_revert_with(FaucetError::MissingAvailableAmount)
        .unwrap_or_revert_with(FaucetError::InvalidAvailableAmount);

    let remaining_amount_key = runtime::get_key(REMAINING_AMOUNT)
        .unwrap_or_revert_with(FaucetError::MissingRemainingAmount);

    let remaining_amount: U512 = storage::read(
        remaining_amount_key
            .into_uref()
            .unwrap_or_revert_with(FaucetError::MissingRemainingAmount),
    )
    .unwrap_or_revert_with(FaucetError::MissingRemainingAmount)
    .unwrap_or_revert_with(FaucetError::InvalidRemainingAmount);

    let distributions_per_interval_uref = runtime::get_key(DISTRIBUTIONS_PER_INTERVAL)
        .unwrap_or_revert_with(FaucetError::MissingDistributionsPerInterval)
        .into_uref()
        .unwrap_or_revert_with(FaucetError::InvalidDistributionsPerInterval);

    let distributions_per_interval: u64 = storage::read(distributions_per_interval_uref)
        .unwrap_or_revert_with(FaucetError::MissingDistributionsPerInterval)
        .unwrap_or_revert_with(FaucetError::InvalidDistributionsPerInterval);

    let distribution_amount =
        Ratio::new(available_amount, U512::from(distributions_per_interval)).to_integer();

    match remaining_amount.cmp(&distribution_amount) {
        Ordering::Equal | Ordering::Greater => distribution_amount,
        Ordering::Less => remaining_amount,
    }
}

fn reset_remaining_amount() {
    let available_amount_uref = runtime::get_key(AVAILABLE_AMOUNT)
        .unwrap_or_revert_with(FaucetError::MissingAvailableAmount)
        .into_uref()
        .unwrap_or_revert_with(FaucetError::MissingAvailableAmount);

    let available_amount: U512 = storage::read(available_amount_uref)
        .unwrap_or_revert_with(FaucetError::MissingAvailableAmount)
        .unwrap_or_revert_with(FaucetError::InvalidAvailableAmount);

    let remaining_amount_uref = runtime::get_key(REMAINING_AMOUNT)
        .unwrap_or_revert_with(FaucetError::MissingRemainingAmount)
        .into_uref()
        .unwrap_or_revert_with(FaucetError::InvalidRemainingAmount);

    storage::write(remaining_amount_uref, available_amount);
}

fn decrease_remaining_amount(amount: U512) -> U512 {
    let remaining_amount_uref = runtime::get_key(REMAINING_AMOUNT)
        .unwrap_or_revert_with(FaucetError::MissingRemainingAmount)
        .into_uref()
        .unwrap_or_revert_with(FaucetError::InvalidRemainingAmount);

    let remaining_amount: U512 = storage::read(remaining_amount_uref)
        .unwrap_or_revert_with(FaucetError::MissingRemainingAmount)
        .unwrap_or_revert_with(FaucetError::InvalidRemainingAmount);

    let new_remaining_amount = remaining_amount.saturating_sub(amount);
    storage::write(remaining_amount_uref, new_remaining_amount);

    new_remaining_amount
}

fn set_last_distribution_time(t: BlockTime) {
    let last_distribution_time_uref = runtime::get_key(LAST_DISTRIBUTION)
        .unwrap_or_revert_with(FaucetError::MissingDistributionTime)
        .into_uref()
        .unwrap_or_revert_with(FaucetError::InvalidDistributionTime);

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
        Ok(val) => Some(val),
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

#[no_mangle]
pub fn init() {
    let installer = runtime::get_key(INSTALLER)
        .unwrap_or_revert_with(FaucetError::InstallerMissing)
        .into_account()
        .unwrap_or_revert_with(FaucetError::InstallerInvalid);

    if runtime::get_caller() != installer {
        runtime::revert(FaucetError::InvalidAccount);
    }

    runtime::put_key(TIME_INTERVAL, storage::new_uref(7_200_000u64).into());
    runtime::put_key(LAST_DISTRIBUTION, storage::new_uref(0u64).into());
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
    let installer = runtime::get_key(INSTALLER)
        .unwrap_or_revert_with(FaucetError::InstallerMissing)
        .into_account()
        .unwrap_or_revert_with(FaucetError::InstallerInvalid);
    let caller = runtime::get_caller();
    if caller != installer {
        runtime::revert(FaucetError::InvalidAccount);
    }

    let new_time_interval: u64 = runtime::get_named_arg(ARG_TIME_INTERVAL);
    let time_interval_uref = runtime::get_key(TIME_INTERVAL)
        .unwrap_or_revert_with(FaucetError::MissingTimeInterval)
        .into_uref()
        .unwrap_or_revert_with(FaucetError::InvalidTimeInterval);

    storage::write(time_interval_uref, new_time_interval);

    let new_available_amount: U512 = runtime::get_named_arg(ARG_AVAILABLE_AMOUNT);
    let available_amount_uref = runtime::get_key(AVAILABLE_AMOUNT)
        .unwrap_or_revert_with(FaucetError::MissingAvailableAmount)
        .into_uref()
        .unwrap_or_revert_with(FaucetError::InvalidAvailableAmount);

    storage::write(available_amount_uref, new_available_amount);

    // when the available amount is reset, the remaining amount will be reset to match it.
    let remaining_amount_uref = runtime::get_key(REMAINING_AMOUNT)
        .unwrap_or_revert_with(FaucetError::MissingRemainingAmount)
        .into_uref()
        .unwrap_or_revert_with(FaucetError::InvalidRemainingAmount);

    storage::write(remaining_amount_uref, new_available_amount);

    let new_distributions_per_interval: u64 =
        runtime::get_named_arg(ARG_DISTRIBUTIONS_PER_INTERVAL);
    let distributions_per_interval_uref = runtime::get_key(DISTRIBUTIONS_PER_INTERVAL)
        .unwrap_or_revert_with(FaucetError::MissingDistributionsPerInterval)
        .into_uref()
        .unwrap_or_revert_with(FaucetError::InvalidDistributionsPerInterval);

    storage::write(
        distributions_per_interval_uref,
        new_distributions_per_interval,
    );
}
