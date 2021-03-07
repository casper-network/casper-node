#![no_std]
#![no_main]

extern crate alloc;

use alloc::{string::String, vec::Vec};

use casper_contract::contract_api::runtime;
use casper_types::contracts::NamedKeys;

const ARG_INITIAL_NAMED_KEYS: &str = "initial_named_args";
const ARG_NEW_NAMED_KEYS: &str = "new_named_keys";

#[no_mangle]
pub extern "C" fn call() {
    // Account starts with two known named keys: mint uref & handle payment uref.
    let expected_initial_named_keys: NamedKeys = runtime::get_named_arg(ARG_INITIAL_NAMED_KEYS);

    let actual_named_keys = runtime::list_named_keys();
    assert_eq!(expected_initial_named_keys, actual_named_keys);

    // Add further named keys and assert that each is returned in `list_named_keys()`.
    let new_named_keys: NamedKeys = runtime::get_named_arg(ARG_NEW_NAMED_KEYS);
    let mut expected_named_keys = expected_initial_named_keys;

    for (key, value) in new_named_keys {
        runtime::put_key(&key, value);
        assert!(expected_named_keys.insert(key, value).is_none());
        let actual_named_keys = runtime::list_named_keys();
        assert_eq!(expected_named_keys, actual_named_keys);
    }

    // Remove all named keys and check that removed keys aren't returned in `list_named_keys()`.
    let all_key_names: Vec<String> = expected_named_keys.keys().cloned().collect();
    for key in all_key_names {
        runtime::remove_key(&key);
        assert!(expected_named_keys.remove(&key).is_some());
        let actual_named_keys = runtime::list_named_keys();
        assert_eq!(expected_named_keys, actual_named_keys);
    }
}
