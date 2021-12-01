#![no_std]
#![no_main]

extern crate alloc;
use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};
use casper_contract::{
    contract_api::{runtime, storage},
    unwrap_or_revert::UnwrapOrRevert,
};

type DictIndex = u8; // Must fit into `usize`.
type KeySeed = u8;
type Value = u8;
const DICTIONARY_NAMES: &[&str] = &[
    "the", "quick", "brown", "fox", "jumps", "over", "the_", "lazy", "dog",
];

#[no_mangle]
pub extern "C" fn call() {
    let puts: Vec<(DictIndex, KeySeed, Value)> = runtime::get_named_arg("puts");

    for name in DICTIONARY_NAMES {
        let _ = storage::new_dictionary(name).unwrap_or_revert();
    }

    let mut maps: Vec<BTreeMap<String, Value>> = (0..DICTIONARY_NAMES.len())
        .map(|_| BTreeMap::new())
        .collect();
    for (dict_index, key_seed, value) in puts {
        let dict_index = dict_index as usize;
        assert!(dict_index < DICTIONARY_NAMES.len());
        let key = key_seed.to_string();
        assert_eq!(
            maps[dict_index].get(&key),
            storage::named_dictionary_get(DICTIONARY_NAMES[dict_index], &key)
                .unwrap_or_revert()
                .as_ref()
        );
        storage::named_dictionary_put(DICTIONARY_NAMES[dict_index], &key, value);
        maps[dict_index].insert(key, value);
    }

    for i in 0..DICTIONARY_NAMES.len() {
        for (key, &value) in maps[i].iter() {
            assert_eq!(
                storage::named_dictionary_get(DICTIONARY_NAMES[i], key).unwrap_or_revert(),
                Some(value)
            );
        }
    }
}
