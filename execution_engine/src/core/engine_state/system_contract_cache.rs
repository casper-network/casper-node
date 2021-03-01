use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use parity_wasm::elements::Module;

use crate::storage::protocol_data::ProtocolData;
use casper_types::ContractHash;

/// A cache of deserialized contracts.
#[derive(Clone, Default, Debug)]
pub struct SystemContractCache(Arc<RwLock<HashMap<ContractHash, Module>>>);

impl SystemContractCache {
    /// Returns `true` if the cache has a contract corresponding to `contract_hash`.
    pub fn has(&self, contract_hash: ContractHash) -> bool {
        let guarded_map = self.0.read().unwrap();
        guarded_map.contains_key(&contract_hash)
    }

    /// Inserts `contract` into the cache under `contract_hash`.
    ///
    /// If the cache did not have this key present, `None` is returned.
    ///
    /// If the cache did have this key present, the value is updated, and the old value is returned.
    pub fn insert(&self, contract_hash: ContractHash, module: Module) -> Option<Module> {
        let mut guarded_map = self.0.write().unwrap();
        guarded_map.insert(contract_hash, module)
    }

    /// Returns a clone of the contract corresponding to `contract_hash`.
    pub fn get(&self, contract_hash: ContractHash) -> Option<Module> {
        let guarded_map = self.0.read().unwrap();
        guarded_map.get(&contract_hash).cloned()
    }

    /// Initializes cache from protocol data.
    pub fn initialize_with_protocol_data(&self, protocol_data: &ProtocolData, module: &Module) {
        // TODO: the SystemContractCache is vestigial and should be removed. In the meantime,
        // a minimal viable wasm module is used as a placeholder to fulfil expectations of
        // the runtime.
        let mint_hash = protocol_data.mint();
        if !self.has(mint_hash) {
            self.insert(mint_hash, module.clone());
        }
        let auction_hash = protocol_data.auction();
        if !self.has(auction_hash) {
            self.insert(auction_hash, module.clone());
        }
        let handle_payment_hash = protocol_data.handle_payment();
        if !self.has(handle_payment_hash) {
            self.insert(handle_payment_hash, module.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use once_cell::sync::Lazy;
    use parity_wasm::elements::{Module, ModuleNameSubsection, NameSection, Section};

    use crate::core::{
        engine_state::system_contract_cache::SystemContractCache,
        execution::{AddressGenerator, AddressGeneratorBuilder},
    };
    use casper_types::contracts::ContractHash;

    static ADDRESS_GENERATOR: Lazy<Mutex<AddressGenerator>> = Lazy::new(|| {
        Mutex::new(
            AddressGeneratorBuilder::new()
                .seed_with(b"test_seed")
                .build(),
        )
    });

    #[test]
    fn should_insert_module() {
        let reference = {
            let mut address_generator = ADDRESS_GENERATOR.lock().unwrap();
            address_generator.create_address()
        };
        let module = Module::default();

        let cache = SystemContractCache::default();

        let result = cache.insert(reference.into(), module);

        assert!(result.is_none())
    }

    #[test]
    fn should_has_false() {
        let cache = SystemContractCache::default();
        let reference = {
            let mut address_generator = ADDRESS_GENERATOR.lock().unwrap();
            address_generator.create_address()
        }
        .into();

        assert!(!cache.has(reference))
    }

    #[test]
    fn should_has_true() {
        let cache = SystemContractCache::default();
        let reference = {
            let mut address_generator = ADDRESS_GENERATOR.lock().unwrap();
            address_generator.create_address()
        }
        .into();
        let module = Module::default();

        cache.insert(reference, module);

        assert!(cache.has(reference))
    }

    #[test]
    fn should_has_true_normalized_has() {
        let cache = SystemContractCache::default();
        let reference = {
            let mut address_generator = ADDRESS_GENERATOR.lock().unwrap();
            address_generator.create_address()
        }
        .into();
        let module = Module::default();

        cache.insert(reference, module);

        assert!(cache.has(reference))
    }

    #[test]
    fn should_has_true_normalized_insert() {
        let cache = SystemContractCache::default();
        let reference = {
            let mut address_generator = ADDRESS_GENERATOR.lock().unwrap();
            address_generator.create_address()
        }
        .into();
        let module = Module::default();

        cache.insert(reference, module);

        assert!(cache.has(reference))
    }

    #[test]
    fn should_get_none() {
        let reference = {
            let mut address_generator = ADDRESS_GENERATOR.lock().unwrap();
            address_generator.create_address()
        }
        .into();
        let cache = SystemContractCache::default();

        let result = cache.get(reference);

        assert!(result.is_none())
    }

    #[test]
    fn should_get_module() {
        let cache = SystemContractCache::default();
        let reference = {
            let mut address_generator = ADDRESS_GENERATOR.lock().unwrap();
            address_generator.create_address()
        }
        .into();
        let module = Module::default();

        cache.insert(reference, module.clone());

        let result = cache.get(reference);

        assert_eq!(result, Some(module))
    }

    #[test]
    fn should_get_module_normalized_get() {
        let cache = SystemContractCache::default();
        let reference = {
            let mut address_generator = ADDRESS_GENERATOR.lock().unwrap();
            address_generator.create_address()
        }
        .into();
        let module = Module::default();

        cache.insert(reference, module.clone());

        let result = cache.get(reference);

        assert_eq!(result, Some(module.clone()));

        let result = cache.get(reference);

        assert_eq!(result, Some(module))
    }

    #[test]
    fn should_get_module_normalized_insert() {
        let cache = SystemContractCache::default();
        let reference: ContractHash = {
            let mut address_generator = ADDRESS_GENERATOR.lock().unwrap();
            address_generator.create_address()
        }
        .into();
        let module = Module::default();

        cache.insert(reference, module.clone());

        let result = cache.get(reference);

        assert_eq!(result, Some(module.clone()));

        let result = cache.get(reference);

        assert_eq!(result, Some(module))
    }

    #[test]
    fn should_update_module() {
        let cache = SystemContractCache::default();
        let reference = {
            let mut address_generator = ADDRESS_GENERATOR.lock().unwrap();
            address_generator.create_address()
        }
        .into();
        let initial_module = Module::default();
        let updated_module = {
            let section = NameSection::new(Some(ModuleNameSubsection::new("a_mod")), None, None);
            let sections = vec![Section::Name(section)];
            Module::new(sections)
        };

        assert_ne!(initial_module, updated_module);

        let result = cache.insert(reference, initial_module.clone());

        assert!(result.is_none());

        let result = cache.insert(reference, updated_module.clone());

        assert_eq!(result, Some(initial_module));

        let result = cache.get(reference);

        assert_eq!(result, Some(updated_module))
    }

    #[test]
    fn should_update_module_normalized() {
        let cache = SystemContractCache::default();
        let reference = {
            let mut address_generator = ADDRESS_GENERATOR.lock().unwrap();
            address_generator.create_address()
        }
        .into();
        let initial_module = Module::default();
        let updated_module = {
            let section = NameSection::new(Some(ModuleNameSubsection::new("a_mod")), None, None);
            let sections = vec![Section::Name(section)];
            Module::new(sections)
        };

        assert_ne!(initial_module, updated_module);

        let result = cache.insert(reference, initial_module.clone());

        assert!(result.is_none());

        let result = cache.insert(reference, updated_module.clone());

        assert_eq!(result, Some(initial_module));

        let result = cache.get(reference);

        assert_eq!(result, Some(updated_module))
    }
}
