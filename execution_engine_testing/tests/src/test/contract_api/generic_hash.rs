use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, LOCAL_GENESIS_REQUEST,
};
use casper_types::{runtime_args, HashAlgorithm};

const GENERIC_HASH_WASM: &str = "generic_hash.wasm";

#[ignore]
#[test]
fn should_run_generic_hash_blake2() {
    LmdbWasmTestBuilder::default()
        .run_genesis(LOCAL_GENESIS_REQUEST.clone())
        .exec(
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                GENERIC_HASH_WASM,
                runtime_args! {
                    "data" => "blake2 hash test",
                    "algorithm" => HashAlgorithm::Blake2b as u8,
                    "expected" => [0x0A, 0x24, 0xA2, 0xDF, 0x30, 0x46, 0x1F, 0xA9, 0x69, 0x36, 0x67, 0x97, 0xE4, 0xD4, 0x30, 0xA1, 0x13, 0xC6, 0xCE, 0xE2, 0x78, 0xB5, 0xEF, 0x63, 0xBD, 0x5D, 0x00, 0xA0, 0xA6, 0x61, 0x1E, 0x29]
                },
            )
            .build(),
        )
        .expect_success()
        .commit();
}

#[ignore]
#[test]
fn should_run_generic_hash_blake3() {
    LmdbWasmTestBuilder::default()
        .run_genesis(LOCAL_GENESIS_REQUEST.clone())
        .exec(
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                GENERIC_HASH_WASM,
                runtime_args! {
                    "data" => "blake3 hash test",
                    "algorithm" => HashAlgorithm::Blake3 as u8,
                    "expected" => [0x01, 0x65, 0x7D, 0x50, 0x0C, 0x51, 0x9B, 0xB6, 0x8D, 0x01, 0x26, 0x53, 0x66, 0xE2, 0x72, 0x2E, 0x1A, 0x05, 0x65, 0x2E, 0xD7, 0x0C, 0x77, 0xB0, 0x06, 0x80, 0xF8, 0xE8, 0x9E, 0xF9, 0x0F, 0xA1]
                },
            )
            .build(),
        )
        .expect_success()
        .commit();
}

#[ignore]
#[test]
fn should_run_generic_hash_sha256() {
    LmdbWasmTestBuilder::default()
        .run_genesis(LOCAL_GENESIS_REQUEST.clone())
        .exec(
            ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                GENERIC_HASH_WASM,
                runtime_args! {
                    "data" => "sha256 hash test",
                    "algorithm" => HashAlgorithm::Sha256 as u8,
                    "expected" => [0x29, 0xD2, 0xC7, 0x7B, 0x39, 0x7F, 0xF6, 0x9E, 0x25, 0x0D, 0x81, 0xA3, 0xBA, 0xBB, 0x32, 0xDE, 0xFF, 0x3C, 0x2D, 0x06, 0xC9, 0x8E, 0x5E, 0x73, 0x60, 0x54, 0x3C, 0xE4, 0x91, 0xAC, 0x81, 0xCA]
                },
            )
            .build(),
        )
        .expect_success()
        .commit();
}
