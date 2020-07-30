#![no_std]
#![no_main]

use casperlabs_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};
use casperlabs_types::{
    auction::{ActiveBids, Delegators, EraValidators, FoundingValidators},
    auction::{ACTIVE_BIDS_KEY, DELEGATORS_KEY, ERA_VALIDATORS_KEY, FOUNDER_VALIDATORS_KEY},
    contracts::NamedKeys,
    CLValue,
};

const HASH_KEY_NAME: &str = "auction_hash";
const ACCESS_KEY_NAME: &str = "auction_access";

#[no_mangle]
pub extern "C" fn install() {
    let entry_points = auction::get_entry_points();

    let (contract_package_hash, access_uref) = storage::create_contract_package_at_hash();
    runtime::put_key(HASH_KEY_NAME, contract_package_hash.into());
    runtime::put_key(ACCESS_KEY_NAME, access_uref.into());

    let named_keys = {
        let mut named_keys = NamedKeys::new();

        named_keys.insert(
            FOUNDER_VALIDATORS_KEY.into(),
            storage::new_uref(FoundingValidators::new()).into(),
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

        named_keys
    };

    let (contract_key, _contract_version) =
        storage::add_contract_version(contract_package_hash, entry_points, named_keys);

    let return_value = CLValue::from_t(contract_key).unwrap_or_revert();
    runtime::ret(return_value);
}
