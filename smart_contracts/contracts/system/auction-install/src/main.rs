#![no_std]
#![no_main]

extern crate alloc;

use alloc::collections::BTreeMap;
use casperlabs_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casperlabs_types::{
    account::AccountHash,
    auction::{
        ActiveBids, Delegators, EraIndex, EraValidators, FoundingValidator, FoundingValidators,
        ERA_INDEX_KEY,
    },
    auction::{ACTIVE_BIDS_KEY, DELEGATORS_KEY, ERA_VALIDATORS_KEY, FOUNDER_VALIDATORS_KEY},
    contracts::{NamedKeys, CONTRACT_INITIAL_VERSION},
    runtime_args,
    system_contract_errors::mint,
    CLValue, ContractPackageHash, RuntimeArgs, URef, U512,
};

const HASH_KEY_NAME: &str = "auction_hash";
const ACCESS_KEY_NAME: &str = "auction_access";
const ENTRY_POINT_MINT: &str = "mint";
const ARG_AMOUNT: &str = "amount";
const ARG_GENESIS_VALIDATORS: &str = "genesis_validators";

#[no_mangle]
pub extern "C" fn install() {
    let mint_package_hash: ContractPackageHash =
        runtime::get_named_arg("mint_contract_package_hash");

    let entry_points = auction::get_entry_points();
    let (contract_package_hash, access_uref) = storage::create_contract_package_at_hash();
    runtime::put_key(HASH_KEY_NAME, contract_package_hash.into());
    runtime::put_key(ACCESS_KEY_NAME, access_uref.into());

    let named_keys = {
        let mut named_keys = NamedKeys::new();

        let mut founding_validators = FoundingValidators::new();

        let genesis_validators: BTreeMap<AccountHash, U512> =
            runtime::get_named_arg(ARG_GENESIS_VALIDATORS);

        for (validator_account_hash, amount) in genesis_validators {
            let bonding_purse = mint_purse(mint_package_hash, amount);
            let founding_validator = FoundingValidator::new(bonding_purse, amount);
            founding_validators.insert(validator_account_hash, founding_validator);
        }

        named_keys.insert(
            FOUNDER_VALIDATORS_KEY.into(),
            storage::new_uref(founding_validators).into(),
        );
        named_keys.insert(
            ACTIVE_BIDS_KEY.into(),
            storage::new_uref(ActiveBids::new()).into(),
        );
        named_keys.insert(
            DELEGATORS_KEY.into(),
            storage::new_uref(Delegators::new()).into(),
        );
        named_keys.insert(
            ERA_VALIDATORS_KEY.into(),
            storage::new_uref(EraValidators::new()).into(),
        );

        let era_index: EraIndex = 0;

        named_keys.insert(ERA_INDEX_KEY.into(), storage::new_uref(era_index).into());

        named_keys
    };

    let (contract_key, _contract_version) =
        storage::add_contract_version(contract_package_hash, entry_points, named_keys);

    let return_value = CLValue::from_t(contract_key).unwrap_or_revert();
    runtime::ret(return_value);
}

fn mint_purse(contract_package_hash: ContractPackageHash, amount: U512) -> URef {
    let args = runtime_args! {
        ARG_AMOUNT => amount,
    };

    let result: Result<URef, mint::Error> = runtime::call_versioned_contract(
        contract_package_hash,
        Some(CONTRACT_INITIAL_VERSION),
        ENTRY_POINT_MINT,
        args,
    );

    result.unwrap_or_revert()
}
