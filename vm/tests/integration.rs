use blake2::{Blake2b, Digest};
use digest::consts::U32;
use rand::prelude::*;
use std::{
    borrow::BorrowMut,
    collections::{BTreeMap, HashMap},
    sync::{Arc, RwLock},
};
// use digest::{self, Digest, FixedOutput, Update, HashMarker, typenum::Unsigned, consts::U32};
use bytes::Bytes;
use vm::{
    backend::{Context, WasmInstance},
    storage::{self, Address, CreateResult, Entry, Manifest, Storage},
    ConfigBuilder, VM,
};

// use super::*;
const TEST_CONTRACT_WASM: &[u8] = include_bytes!("../test-contract.wasm");

type Blake2b256 = Blake2b<U32>;

fn blake2b256(updater: impl FnOnce(&mut Blake2b256)) -> Address {
    let mut hasher = Blake2b256::new();
    updater(&mut hasher);
    hasher.finalize().into()
}

fn make_contract_address(package_address: &Address, version: u32) -> Address {
    blake2b256(|hasher| {
        hasher.update(&package_address);
        hasher.update(&version.to_le_bytes());
    })
}

#[derive(Default, Debug, Clone)]
struct Contract {
    code_hash: Address,
    manifest: Manifest,
}

#[derive(Default, Debug, Clone)]
struct Package {
    versions: Vec<Address>,
}

#[derive(Default, Debug, Clone)]
struct MockStorage {
    // journal: Arc<Vec<JournalEntry>>,
    db: Arc<RwLock<BTreeMap<u64, BTreeMap<Bytes, (u64, Bytes)>>>>,
    balances: Arc<RwLock<BTreeMap<Bytes, u64>>>,

    code: Arc<RwLock<BTreeMap<Address, Bytes>>>,
    packages: Arc<RwLock<BTreeMap<Address, Package>>>,
    contracts: Arc<RwLock<BTreeMap<Address, Contract>>>,
    // contracts: Arc<RwLock<BTreeMap<Bytes>
}

/// VM execute request specifies execution context, the wasm bytes, and other necessary information
/// to execute.
pub struct ExecuteRequest {
    /// Wasm module.
    pub wasm_bytes: Bytes,
    /// Input.
    pub input: Bytes,
}

impl Storage for MockStorage {
    fn write(
        &self,
        key_tag: u64,
        key: &[u8],
        value_tag: u64,
        value: &[u8],
    ) -> Result<(), storage::Error> {
        let key_bytes = Bytes::copy_from_slice(key);
        let value_bytes = Bytes::copy_from_slice(value);
        // self.journal.push(JournalEntry::Write(key_bytes.clone(), value_bytes.clone()));
        self.db
            .write()
            .unwrap()
            .entry(key_tag)
            .or_default()
            .insert(key_bytes, (value_tag, value_bytes));
        Ok(())
    }

    fn read(&self, key_tag: u64, key: &[u8]) -> Result<Option<Entry>, storage::Error> {
        // let key_bytes = Bytes::copy_from_slice(key);
        // self.journal.push(JournalEntry::Read(key_bytes.clone()));
        match self
            .db
            .read()
            .unwrap()
            .get(&key_tag)
            .and_then(|inner| inner.get(key))
        {
            Some((value_tag, value)) => Ok(Some(Entry {
                tag: *value_tag,
                data: value.clone(),
            })),
            None => Ok(None),
        }
    }

    fn call(
        &self,
        address: &[u8],
        value: u64,
        entry_point: u32,
    ) -> Result<storage::CallOutcome, storage::Error> {
        todo!()
    }

    fn get_balance(&self, entity_address: &[u8]) -> Result<Option<u64>, storage::Error> {
        match self.balances.read().unwrap().get(entity_address) {
            Some(balance) => Ok(Some(*balance)),
            None => Ok(None),
        }
    }

    fn update_balance(
        &self,
        entity_address: &[u8],
        new_balance: u64,
    ) -> Result<Option<u64>, storage::Error> {
        {
            let mut balances = self.balances.write().unwrap();
            if let Some(bal) = balances.get_mut(entity_address) {
                let old = *bal;
                *bal = new_balance;
                return Ok(Some(old));
            }
        }

        self.balances
            .write()
            .unwrap()
            .insert(Bytes::copy_from_slice(entity_address), new_balance);
        Ok(None)
    }

    fn create_contract(
        &self,
        code_bytes: Bytes,
        manifest: storage::Manifest,
    ) -> Result<storage::CreateResult, storage::Error> {
        let initial_version = 1u32;
        let package_address: Address = rand::thread_rng().gen();
        let contract_address: Address = make_contract_address(&package_address, initial_version);
        let code_hash = blake2b256(|hasher| hasher.update(&code_bytes));

        let contract = Contract {
            code_hash,
            manifest,
        };

        let mut package = Package::default();
        package.versions.push(contract_address);

        {
            let mut code = self.code.write().unwrap();
            code.borrow_mut().insert(code_hash, code_bytes);
        }

        {
            let mut packages = self.packages.write().unwrap();
            packages.borrow_mut().insert(package_address, package);
        }

        {
            let mut contracts = self.contracts.write().unwrap();
            contracts.borrow_mut().insert(contract_address, contract);
        }

        Ok(CreateResult {
            package_address,
            contract_address,
        })
    }
}

struct ContractRuntime {
    vm: VM,
    storage: MockStorage,
}

impl ContractRuntime {
    fn execute(&mut self, execute_request: ExecuteRequest) {
        let ExecuteRequest { wasm_bytes, input } = execute_request;
    }
}

const ALICE: [u8; 32] = [100; 32];
const BOB: [u8; 32] = [101; 32];
const CSPR: u64 = 10u64.pow(9);

#[test]
fn smoke() {
    let bytecode = Bytes::from_static(TEST_CONTRACT_WASM);

    // for gas_limit in 1000..2000 {
    // dbg!(&gas_limit);

    let storage = MockStorage::default();

    {
        // "Genesis"
        storage.update_balance(&ALICE, 10 * CSPR).unwrap();
        storage.update_balance(&BOB, 10 * CSPR).unwrap();
    }

    let mut vm = VM::new();

    let mut contract_runtime = ContractRuntime {
        vm: VM::new(),
        storage: storage.clone(),
    };

    let input = borsh::to_vec(&("Hello, world!".to_string(), 123456789u32))
        .map(Bytes::from)
        .unwrap();

    const GAS_LIMIT: u64 = 1_000_000;
    const MEMORY_LIMIT: u32 = 17;

    // let input = b"This is a very long input data.".to_vec();

    let config = ConfigBuilder::new()
        .with_gas_limit(GAS_LIMIT)
        .with_memory_limit(MEMORY_LIMIT)
        .with_input(input)
        .build();

    let mock_context = Context { storage };

    let retrieved_context = {
        let mut instance = vm
            .prepare(bytecode, mock_context, config)
            .expect("should prepare");
        eprintln!("gas_limit={GAS_LIMIT}");
        let (result, gas_summary) = instance.call_export("call");
        eprintln!("{result:?} {gas_summary:?}");
        instance.teardown()
    };

    // // dbg!(&res);
    // dbg!(&retrieved_context.storage);
    // // retrieved_context.storage
}
