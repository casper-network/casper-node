use bytes::Bytes;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use vm::{
    backend::{Context, WasmInstance},
    storage::{self, Storage},
    ExecuteRequest, VM,
};

// use super::*;
const TEST_CONTRACT_WASM: &[u8] = include_bytes!("../test-contract.wasm");

// fn fibonacci(n: u64) -> u64 {
//     match n {
//         0 => 1,
//         1 => 1,
//         n => fibonacci(n-1) + fibonacci(n-2),
//     }
// }

fn bench_call() {
    // let mut vm = VM::new();
    // let execute_request = ExecuteRequest {
    //     wasm_bytes: Bytes::from_static(TEST_CONTRACT_WASM),
    // };

    // let storage = MockStorage::default();

    // let mock_context = Context { storage };

    // let retrieved_context = {
    //     let mut instance = vm
    //         .prepare(execute_request, mock_context)
    //         .expect("should prepare");

    //     let args = &[b"hello".as_slice(), b"world but longer".as_slice()];

    //     let (result, gas_summary) = instance.call_export("call", args);
    //     dbg!(&result, gas_summary);

    //     instance.teardown()
    // };
}

#[derive(Default, Debug, Clone)]
struct MockStorage;
impl Storage for MockStorage {
    fn write(
        &self,
        key_tag: u64,
        key: &[u8],
        value_tag: u64,
        value: &[u8],
    ) -> Result<(), storage::Error> {
        todo!()
    }

    fn read(&self, key_tag: u64, key: &[u8]) -> Result<Option<storage::Entry>, storage::Error> {
        todo!()
    }
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("call", |b| {
        let mut vm = VM::new();
        let storage = MockStorage::default();

        let execute_request = ExecuteRequest {
            wasm_bytes: Bytes::from_static(TEST_CONTRACT_WASM),
        };

        let mock_context = Context { storage };

        let mut instance = vm
            .prepare(execute_request, mock_context)
            .expect("should prepare");

        b.iter(|| {
            let args = &[b"hello".as_slice(), b"world but longer".as_slice()];
            let (result, gas_summary) = instance.call_export("call", args);
        });

        instance.teardown();
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
