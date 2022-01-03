#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use casper_contract::{
    contract_api::{self, runtime, storage, system},
    ext_ffi,
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    account::AccountHash, api_error, bytesrepr, bytesrepr::FromBytes, ApiError, BlockTime, CLValue,
    Key, U512,
};

const ARG_TARGET: &str = "target";
const ARG_ID: &str = "id";
const ARG_TIME_INCREMENT: &str = "time_increment";
const AVAILABLE_AMOUNT: &str = "available_amount";
const TIME_INCREMENT: &str = "time_increment";
const LAST_ISSUANCE: &str = "last_issuance";
const FAUCET_PURSE: &str = "faucet_purse";
const INSTALLER: &str = "installer";

#[repr(u16)]
enum CustomError {
    InvalidAccount = 1,
    InstallerMissing = 2,
    InstallerInvalid = 3,
    InstallerDoesNotFundItself = 4,
    InvalidAmount = 5,
    MissingIssuanceTime = 6,
    InvalidIssuanceTime = 7,
    MissingAvailableAmount = 8,
    InvalidAvailableAmount = 9,
    MissingTimeIncrement = 10,
    InvalidTimeIncrement = 11,
    MissingId = 12,
    InvalidId = 13,
    FromBytesCalledWithError = 14,
    FailedToTransfer = 15,
    FailedToGetArgBytes = 16,
    FailedToConstructReturnData = 17,
}

impl FromBytes for CustomError {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        runtime::revert(ApiError::User(CustomError::FromBytesCalledWithError as u16))
    }
}

/// Executes token transfer to supplied account hash.
/// Revert status codes:
/// 1 - requested transfer to already funded account hash.
#[no_mangle]
pub fn delegate() {
    // how much is available?
    // this was a passed arg previously (runtime::get_named_arg(ARG_AMOUNT);)
    // but will now be calculated
    let available_amount: U512 = get_available_amount();

    let id = get_optional_named_arg_with_user_errors(
        ARG_ID,
        CustomError::MissingId,
        CustomError::InvalidId,
    );

    let caller = runtime::get_caller();
    let installer = runtime::get_key(INSTALLER)
        .unwrap_or_revert_with(ApiError::User(CustomError::InstallerMissing as u16))
        .into_account()
        .unwrap_or_revert_with(ApiError::User(CustomError::InstallerInvalid as u16));

    let amount = {
        if caller == installer {
            // this is the installer / faucet account creating a NEW account
            // or topping off an existing account
            let target: AccountHash = runtime::get_named_arg(ARG_TARGET);
            transfer_and_reduce_available_amount(target, available_amount, id)
        } else {
            // this is an unknown existing account asking for top off
            transfer_and_reduce_available_amount(caller, available_amount, id)
        }
    };
    // return amount granted as a courtesy to the caller
    runtime::ret(
        CLValue::from_t(amount)
            .unwrap_or_revert_with(ApiError::User(CustomError::InvalidAmount as u16)),
    )
}

fn transfer_and_reduce_available_amount(
    target: AccountHash,
    amount: U512,
    id: Option<u64>,
) -> U512 {
    system::transfer_to_account(target, amount, id)
        .unwrap_or_revert_with(ApiError::User(CustomError::FailedToTransfer as u16));
    reduce_available_amount(amount)
}

fn get_available_amount() -> U512 {
    let available_amount_key = runtime::get_key(AVAILABLE_AMOUNT)
        .unwrap_or_revert_with(ApiError::User(CustomError::MissingAvailableAmount as u16));
    let mut total_available_amount: U512 = storage::read(
        available_amount_key
            .into_uref()
            .unwrap_or_revert_with(ApiError::User(CustomError::MissingAvailableAmount as u16)),
    )
    .unwrap_or_revert_with(ApiError::User(CustomError::MissingAvailableAmount as u16))
    .unwrap_or_revert_with(ApiError::User(CustomError::InvalidAvailableAmount as u16));

    let time_increment: u64 = storage::read(
        runtime::get_key(TIME_INCREMENT)
            .unwrap_or_revert_with(ApiError::User(CustomError::MissingTimeIncrement as u16))
            .into_uref()
            .unwrap_or_revert_with(ApiError::User(CustomError::InvalidTimeIncrement as u16)),
    )
    .unwrap_or_revert_with(ApiError::User(CustomError::MissingTimeIncrement as u16))
    .unwrap_or_revert_with(ApiError::User(CustomError::InvalidTimeIncrement as u16));

    let last_issuance_time: u64 = storage::read(
        runtime::get_key(LAST_ISSUANCE)
            .unwrap_or_revert_with(ApiError::User(CustomError::MissingIssuanceTime as u16))
            .into_uref()
            .unwrap_or_revert_with(ApiError::User(CustomError::InvalidIssuanceTime as u16)),
    )
    .unwrap_or_revert_with(ApiError::User(CustomError::MissingIssuanceTime as u16))
    .unwrap_or_revert_with(ApiError::User(CustomError::InvalidIssuanceTime as u16));

    let blocktime = runtime::get_blocktime();

    if blocktime > BlockTime::new(last_issuance_time + time_increment) {
        total_available_amount = increase_available_amount(total_available_amount)
    }
    // how much does _this caller_ get of the total available amount?
    let available_amount = U512::zero();
    available_amount
}

fn increase_available_amount(current_total_amount: U512) -> U512 {
    let new_amount = U512::zero();
    let available_amount = current_total_amount + new_amount;
    let last_issuance_uref = runtime::get_key(LAST_ISSUANCE)
        .unwrap_or_revert_with(ApiError::User(CustomError::MissingIssuanceTime as u16))
        .into_uref()
        .unwrap_or_revert_with(ApiError::User(CustomError::InvalidIssuanceTime as u16));

    let last_issuance_time: u64 = storage::read(last_issuance_uref)
        .unwrap_or_revert_with(ApiError::User(CustomError::MissingIssuanceTime as u16))
        .unwrap_or_revert_with(ApiError::User(CustomError::InvalidIssuanceTime as u16));

    storage::write(last_issuance_uref, last_issuance_time);

    available_amount
}

fn reduce_available_amount(amount: U512) -> U512 {
    let available_amount_uref = runtime::get_key(AVAILABLE_AMOUNT)
        .unwrap_or_revert_with(ApiError::User(CustomError::MissingAvailableAmount as u16))
        .into_uref()
        .unwrap_or_revert_with(ApiError::User(CustomError::InvalidAvailableAmount as u16));

    let mut available_amount: U512 = storage::read(available_amount_uref)
        .unwrap_or_revert_with(ApiError::User(CustomError::MissingAvailableAmount as u16))
        .unwrap_or_revert_with(ApiError::User(CustomError::InvalidAvailableAmount as u16));

    if amount.is_zero() {
        return available_amount;
    }

    let new_available_amount = available_amount.saturating_sub(amount);
    storage::write(available_amount_uref, new_available_amount);

    new_available_amount
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
    missing: CustomError,
    invalid: CustomError,
) -> Option<T> {
    match get_named_arg_with_user_errors(name, missing, invalid) {
        Ok(val) => Some(val),
        Err(err @ CustomError::InvalidId) => runtime::revert(ApiError::User(err as u16)),
        Err(_) => None,
    }
}

fn get_named_arg_with_user_errors<T: FromBytes>(
    name: &str,
    missing: CustomError,
    invalid: CustomError,
) -> Result<T, CustomError> {
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
        res.unwrap_or_revert_with(ApiError::User(CustomError::FailedToGetArgBytes as u16))
    } else {
        // Avoids allocation with 0 bytes and a call to get_named_arg
        Vec::new()
    };

    bytesrepr::deserialize(arg_bytes).map_err(|_| invalid)?
}

#[no_mangle]
pub fn init() {
    let installer = runtime::get_key(INSTALLER)
        .unwrap_or_revert_with(ApiError::User(CustomError::InstallerMissing as u16))
        .into_account()
        .unwrap_or_revert_with(ApiError::User(CustomError::InstallerInvalid as u16));

    if runtime::get_caller() != installer {
        runtime::revert(ApiError::User(CustomError::InvalidAccount as u16));
    }

    runtime::put_key(TIME_INCREMENT, storage::new_uref(7_200_000u64).into());
    runtime::put_key(LAST_ISSUANCE, storage::new_uref(0u64).into());
    runtime::put_key(AVAILABLE_AMOUNT, storage::new_uref(U512::zero()).into());

    let purse = system::create_purse();

    runtime::put_key(FAUCET_PURSE, purse.into());
    runtime::ret(CLValue::from_t(purse).unwrap_or_revert_with(ApiError::User(
        CustomError::FailedToConstructReturnData as u16,
    )))
}

#[no_mangle]
pub fn set_variable() {
    let installer = runtime::get_key(INSTALLER)
        .unwrap_or_revert_with(ApiError::User(CustomError::InstallerMissing as u16))
        .into_account()
        .unwrap_or_revert_with(ApiError::User(CustomError::InstallerInvalid as u16));
    let caller = runtime::get_caller();
    if caller != installer {
        runtime::revert(ApiError::User(CustomError::InvalidAccount as u16));
    }

    match get_named_arg_with_user_errors::<u64>(
        ARG_TIME_INCREMENT,
        CustomError::MissingTimeIncrement,
        CustomError::InvalidTimeIncrement,
    ) {
        Ok(val) => {
            storage::write(
                runtime::get_key(TIME_INCREMENT)
                    .unwrap_or_revert_with(ApiError::User(CustomError::MissingTimeIncrement as u16))
                    .into_uref()
                    .unwrap_or_revert_with(ApiError::User(
                        CustomError::InvalidTimeIncrement as u16,
                    )),
                val,
            );
        }
        Err(err @ CustomError::InvalidTimeIncrement) => runtime::revert(ApiError::User(err as u16)),
        Err(_) => (),
    };

    // lump sum per 2 hr / 500
    // 20 blocks x 25 deploys = 500
}
