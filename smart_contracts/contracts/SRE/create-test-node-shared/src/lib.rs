#![no_std]

use casper_contract::{
    contract_api::{runtime, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{account::AccountHash, ApiError, TransferredTo, U512};

#[repr(u16)]
enum Error {
    AccountAlreadyExists = 10,
    TransferFailed = 11,
    FailedToParseAccountHash = 12,
}

impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        ApiError::User(error as u16)
    }
}

fn parse_account_hash(hex: &[u8]) -> AccountHash {
    let mut buffer = [0u8; 32];
    let bytes_written = base16::decode_slice(hex, &mut buffer)
        .ok()
        .unwrap_or_revert_with(Error::FailedToParseAccountHash);
    if bytes_written != buffer.len() {
        runtime::revert(Error::FailedToParseAccountHash)
    }
    AccountHash::new(buffer)
}

pub fn create_account(account_addr: &[u8; 64], initial_amount: u64) {
    let account_hash = parse_account_hash(account_addr);
    let amount: U512 = U512::from(initial_amount);

    match system::transfer_to_account(account_hash, amount, None)
        .unwrap_or_revert_with(Error::TransferFailed)
    {
        TransferredTo::NewAccount => (),
        TransferredTo::ExistingAccount => runtime::revert(Error::AccountAlreadyExists),
    }
}
