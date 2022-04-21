//! Tests whether transforms produced by contracts appear ordered in the transform journal.
use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::shared::transform::Transform;
use casper_types::{
    runtime_args, system::standard_payment, ContractHash, Key, RuntimeArgs, URef, U512,
};
use core::convert::TryInto;
use rand::{rngs::StdRng, Rng, SeedableRng};

#[ignore]
#[test]
fn contract_transforms_should_be_ordered_in_the_journal() {
    // This many URefs will be created in the contract.
    const N_UREFS: u32 = 100;
    // This many operations will be scattered among these URefs.
    const N_OPS: usize = 1000;

    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);

    let mut rng = StdRng::seed_from_u64(0);

    // Installs the contract and creates the URefs, all initialized to `0_i32`.
    builder
        .exec(
            ExecuteRequestBuilder::from_deploy_item(
                DeployItemBuilder::new()
                    .with_address(*DEFAULT_ACCOUNT_ADDR)
                    .with_empty_payment_bytes(runtime_args! {
                        standard_payment::ARG_AMOUNT => U512::from(30_000_000_000_u64)
                    })
                    .with_session_code("ordered-transforms.wasm", runtime_args! { "n" => N_UREFS })
                    .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
                    .with_deploy_hash(rng.gen())
                    .build(),
            )
            .build(),
        )
        .expect_success()
        .commit();

    let contract_hash: ContractHash = match builder
        .get_expected_account(*DEFAULT_ACCOUNT_ADDR)
        .named_keys()["ordered-transforms-contract-hash"]
    {
        Key::Hash(addr) => addr.into(),
        _ => panic!("Couldn't find orderd-transforms contract."),
    };

    // List of operations to be performed by the contract.
    // An operation is a tuple (t, i, v) where:
    // * `t` is the operation type: 0 for reading, 1 for writing and 2 for adding;
    // * `i` is the URef index;
    // * `v` is the value to write or add (always zero for reads).
    let operations: Vec<(u8, u32, i32)> = (0..N_OPS)
        .map(|_| {
            let t: u8 = rng.gen_range(0..3);
            let i: u32 = rng.gen_range(0..N_UREFS);
            if t == 0 {
                (t, i, 0)
            } else {
                (t, i, rng.gen())
            }
        })
        .collect();

    builder
        .exec(
            ExecuteRequestBuilder::from_deploy_item(
                DeployItemBuilder::new()
                    .with_address(*DEFAULT_ACCOUNT_ADDR)
                    .with_empty_payment_bytes(runtime_args! {
                        standard_payment::ARG_AMOUNT => U512::from(10_000_000_000_u64),
                    })
                    .with_stored_session_hash(
                        contract_hash,
                        "perform_operations",
                        runtime_args! {
                            "operations" => operations.clone(),
                        },
                    )
                    .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
                    .with_deploy_hash(rng.gen())
                    .build(),
            )
            .build(),
        )
        .expect_success()
        .commit();

    let exec_result = builder.get_exec_result(1).unwrap();
    assert_eq!(exec_result.len(), 1);
    let journal = exec_result[0].execution_journal();

    let contract = builder.get_contract(contract_hash).unwrap();
    let urefs: Vec<URef> = (0..N_UREFS)
        .map(
            |i| match contract.named_keys().get(&format!("uref-{}", i)).unwrap() {
                Key::URef(uref) => *uref,
                _ => panic!("Expected a URef."),
            },
        )
        .collect();

    assert!(journal
        .clone()
        .into_iter()
        .filter_map(|(key, transform)| {
            let uref = match key {
                Key::URef(uref) => uref,
                _ => return None,
            };
            let uref_index: u32 = match urefs
                .iter()
                .enumerate()
                .find(|(_, u)| u.addr() == uref.addr())
            {
                Some((i, _)) => i.try_into().unwrap(),
                None => return None,
            };
            let (type_index, value): (u8, i32) = match transform {
                Transform::Identity => (0, 0),
                Transform::Write(sv) => {
                    let v: i32 = sv.as_cl_value().unwrap().clone().into_t().unwrap();
                    (1, v)
                }
                Transform::AddInt32(v) => (2, v),
                _ => panic!("Invalid transform."),
            };
            Some((type_index, uref_index, value))
        })
        .eq(operations.into_iter()));
}
