use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, RwLock},
};

use bytes::Bytes;
use vm::{
    backend::{Context, WasmInstance},
    storage::{self, Entry, Storage},
    ConfigBuilder,  VM,
};

// use super::*;
const TEST_CONTRACT_WASM: &[u8] = include_bytes!("../test-contract.wasm");

#[derive(Default, Debug, Clone)]
struct MockStorage {
    // journal: Arc<Vec<JournalEntry>>,
    db: Arc<RwLock<BTreeMap<u64, BTreeMap<Bytes, (u64, Bytes)>>>>,
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

    fn call(&self, address: &[u8], value: u64, entry_point: u32) -> Result<storage::CallOutcome, storage::Error> {
        todo!()
    }
}

// #[ignore]
// #[test]
// fn trying_to_find_panic_points() {
//     for gas_limit in 1000..2000 {
//         // dbg!(&gas_limit);
//         let mut vm = VM::new();
//         let execute_request = ExecuteRequest {
//             wasm_bytes: Bytes::from_static(TEST_CONTRACT_WASM),
//             input: todo!(),
//         };

//         let storage = MockStorage::default();

//         const MEMORY_LIMIT: u32 = 17;

//         let config = ConfigBuilder::new()
//             .with_gas_limit(gas_limit)
//             .with_memory_limit(MEMORY_LIMIT)
//             .build();

//         let mock_context = Context { storage };

//         let retrieved_context = {
//             let mut instance = vm
//                 .prepare(execute_request, mock_context, config)
//                 .expect("should prepare");

//             let args = &[
//                 b"hello".as_slice(),
//                 b"world but longer".as_slice(),
//                 b"another argument",
//             ];

//             eprintln!("gas_limit={gas_limit}");
//             let (result, gas_summary) = instance.call_export("call");
//             // dbg!(&result, gas_summary);
//             eprintln!("{result:?} {gas_summary:?}");

//             instance.teardown()
//         };

//         // dbg!(&res);
//         dbg!(&retrieved_context.storage);
//         // retrieved_context.storage
//     }
// }

struct ContractRuntime {
    vm: VM,
    storage: MockStorage,
}

impl ContractRuntime {
    fn execute(&mut self, execute_request: ExecuteRequest) {
        let ExecuteRequest{ wasm_bytes, input } = execute_request;
    }
}



#[test]
fn smoke() {
    // for gas_limit in 1000..2000 {
    // dbg!(&gas_limit);

    let storage = MockStorage::default();

    let mut vm =VM::new();

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
            .prepare(TEST_CONTRACT_WASM, mock_context, config)
            .expect("should prepare");

        eprintln!("gas_limit={GAS_LIMIT}");
        let (result, gas_summary) = instance.call_export("call");
        // dbg!(&result, gas_summary);
        eprintln!("{result:?} {gas_summary:?}");

        instance.teardown()
    };

    // dbg!(&res);
    dbg!(&retrieved_context.storage);
    // retrieved_context.storage
}
