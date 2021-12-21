use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, PRODUCTION_PATH,
};
use casper_types::{runtime_args, RuntimeArgs};
use rand::{rngs::StdRng, Rng, SeedableRng};

#[ignore]
#[test]
fn named_dictionaries_should_work_as_expected() {
    // Types from `smart_contracts/contracts/test/named-dictionary-test/src/main.rs`.
    type DictIndex = u8;
    type KeySeed = u8;
    type Value = u8;

    let mut rng = StdRng::seed_from_u64(0);

    let puts: Vec<(DictIndex, KeySeed, Value)> = (0..1_000)
        .map(|_| (rng.gen_range(0..9), rng.gen_range(0..20), rng.gen()))
        .collect();

    let builder = &mut InMemoryWasmTestBuilder::new(&*PRODUCTION_PATH, None);
    builder.run_genesis_with_default_genesis_accounts();
    builder
        .exec(
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                "named-dictionary-test.wasm",
                runtime_args! { "puts" => puts },
            )
            .build(),
        )
        .expect_success();
}
