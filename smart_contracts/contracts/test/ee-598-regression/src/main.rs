#![no_std]
#![no_main]

use casperlabs_contract::contract_api::{account, runtime, system};
use casperlabs_types::{auction, runtime_args, ContractHash, PublicKey, RuntimeArgs, URef, U512};

const ARG_AMOUNT: &str = "amount";
const ARG_PUBLIC_KEY: &str = "public_key";

fn bond(
    contract_hash: ContractHash,
    public_key: PublicKey,
    bond_amount: U512,
    bonding_purse: URef,
) {
    let runtime_args = runtime_args! {
        auction::ARG_AMOUNT => bond_amount,
        auction::ARG_SOURCE_PURSE => bonding_purse,
        auction::ARG_PUBLIC_KEY => public_key,
    };
    runtime::call_contract::<(URef, U512)>(contract_hash, auction::METHOD_BOND, runtime_args);
}

fn unbond(contract_hash: ContractHash, public_key: PublicKey, unbond_amount: U512) {
    let args = runtime_args! {
        auction::ARG_AMOUNT => unbond_amount,
        auction::ARG_PUBLIC_KEY => public_key,
    };
    runtime::call_contract::<(URef, U512)>(contract_hash, auction::METHOD_UNBOND, args);
}

#[no_mangle]
pub extern "C" fn call() {
    let amount: U512 = runtime::get_named_arg(ARG_AMOUNT);
    let public_key = runtime::get_named_arg(ARG_PUBLIC_KEY);
    // unbond attempt for more than is staked should fail
    let contract_hash = system::get_auction();
    bond(contract_hash, public_key, amount, account::get_main_purse());
    unbond(contract_hash, public_key, amount + 1);
}
