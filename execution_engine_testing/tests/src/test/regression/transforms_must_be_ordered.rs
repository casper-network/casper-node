//! Tests whether transforms produced by contracts appear ordered in the effects.
use core::convert::TryInto;

use rand::{rngs::StdRng, Rng, SeedableRng};

use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_types::{
    execution::TransformKind, runtime_args, system::standard_payment, AddressableEntityHash, Key,
    URef, U512,
};

#[ignore]
#[test]
fn contract_transforms_should_be_ordered_in_the_effects() {
    // This many URefs will be created in the contract.
    const N_UREFS: u32 = 100;
    // This many operations will be scattered among these URefs.
    const N_OPS: usize = 1000;

    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone());

    let mut rng = StdRng::seed_from_u64(0);

    let execution_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        "ordered-transforms.wasm",
        runtime_args! { "n" => N_UREFS },
    )
    .build();

    // Installs the contract and creates the URefs, all initialized to `0_i32`.
    builder.exec(execution_request).expect_success().commit();

    let contract_hash = match builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .unwrap()
        .named_keys()
        .get("ordered-transforms-contract-hash")
        .unwrap()
    {
        Key::AddressableEntity(entity_addr) => AddressableEntityHash::new(entity_addr.value()),
        _ => panic!("Couldn't find ordered-transforms contract."),
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
                        standard_payment::ARG_AMOUNT => U512::from(150_000_000_000_u64),
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

    let exec_result = builder.get_exec_result_owned(1).unwrap();
    assert_eq!(exec_result.len(), 1);
    let effects = exec_result[0].effects();

    let contract = builder
        .get_entity_with_named_keys_by_entity_hash(contract_hash)
        .unwrap();
    let urefs: Vec<URef> = (0..N_UREFS)
        .map(
            |i| match contract.named_keys().get(&format!("uref-{}", i)).unwrap() {
                Key::URef(uref) => *uref,
                _ => panic!("Expected a URef."),
            },
        )
        .collect();

    assert!(effects
        .transforms()
        .iter()
        .filter_map(|transform| {
            let uref = match transform.key() {
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
            let (type_index, value): (u8, i32) = match transform.kind() {
                TransformKind::Identity => (0, 0),
                TransformKind::Write(sv) => {
                    let v: i32 = sv.as_cl_value().unwrap().clone().into_t().unwrap();
                    (1, v)
                }
                TransformKind::AddInt32(v) => (2, *v),
                _ => panic!("Invalid transform."),
            };
            Some((type_index, uref_index, value))
        })
        .eq(operations.into_iter()));
}
