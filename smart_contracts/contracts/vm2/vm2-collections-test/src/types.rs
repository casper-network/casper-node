#[cfg(all(not(target_arch = "wasm32")))]
use casper_contract_sdk::abi::{CasperABI, Definition, StructField};
use casper_contract_sdk::{
    collections::IterableMapHash,
    common::type_uid::{TypeUid, Uid},
    compat::types::{CLType, CLTyped},
    prelude::*,
    serializers::borsh::{BorshDeserialize, BorshSerialize},
};

use const_fnv1a_hash::fnv1a_hash_64;

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone, PartialEq, Eq)]
pub(crate) struct TestKey {
    pub(crate) id: u64,
    pub(crate) name: String,
}
impl TypeUid for TestKey {
    const UID: Uid = Uid::from_name("TestKey");
}
impl IterableMapHash for TestKey {}

impl CLTyped for TestKey {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

#[cfg(all(not(target_arch = "wasm32")))]
impl CasperABI for TestKey {
    fn definition() -> casper_contract_sdk::abi::Definition {
        Definition::Struct {
            items: vec![
                StructField {
                    name: "id".into(),
                    decl: casper_executor_wasm_common::type_uid::of::<u64>().into(),
                },
                StructField {
                    name: "name".into(),
                    decl: casper_executor_wasm_common::type_uid::of::<String>().into(),
                },
            ],
        }
    }
}

#[derive(BorshSerialize, BorshDeserialize, PartialEq)]
pub(crate) struct UnitKey;

impl TypeUid for UnitKey {
    const UID: Uid = Uid::from_name("UnitKey");
}

impl IterableMapHash for UnitKey {}

impl CLTyped for UnitKey {
    fn cl_type() -> CLType {
        CLType::Unit
    }
}

#[cfg(all(not(target_arch = "wasm32")))]
impl CasperABI for UnitKey {
    fn definition() -> casper_contract_sdk::abi::Definition {
        Definition::unit()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub(crate) struct CollidingKey(pub(crate) u64, pub(crate) u64);

impl IterableMapHash for CollidingKey {
    fn compute_hash(&self) -> u64 {
        let mut bytes = Vec::new();
        // Only serialize first field for hash computation
        self.0.serialize(&mut bytes).unwrap();
        fnv1a_hash_64(&bytes, None)
    }
}

impl CLTyped for CollidingKey {
    fn cl_type() -> CLType {
        CLType::Unit
    }
}

impl TypeUid for CollidingKey {
    const UID: Uid = Uid::from_name("CollidingKey");
}

#[cfg(all(not(target_arch = "wasm32")))]
impl CasperABI for CollidingKey {
    fn definition() -> casper_contract_sdk::abi::Definition {
        Definition::Tuple {
            items: vec![
                casper_executor_wasm_common::type_uid::of::<u64>().into(),
                casper_executor_wasm_common::type_uid::of::<u64>().into(),
            ],
        }
    }
}
