mod repeated_ffi_call_should_gas_out_quickly {
    use std::{
        sync::mpsc::{self, RecvTimeoutError},
        thread,
        time::Duration,
    };

    use rand::Rng;
    use tempfile::TempDir;

    use casper_engine_test_support::{
        DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
        PRODUCTION_RUN_GENESIS_REQUEST,
    };
    use casper_execution_engine::{core::engine_state::Error, core::execution::Error as ExecError};
    use casper_types::{runtime_args, testing::TestRng, RuntimeArgs, U512};

    const CONTRACT: &str = "regression_20240105.wasm";
    const TIMEOUT: Duration = Duration::from_secs(4);
    const PAYMENT_AMOUNT: u64 = 1_000_000_000_000_u64;

    fn execute_with_timeout(session_args: RuntimeArgs) {
        // if cfg!(debug_assertions) {
        //     println!("not testing in debug mode");
        //     return;
        // }
        let (tx, rx) = mpsc::channel();
        let _ = thread::spawn(move || {
            let payment_args = runtime_args! { "amount" => U512::from(PAYMENT_AMOUNT) };
            let rng = &mut TestRng::new();
            let deploy = DeployItemBuilder::new()
                .with_address(*DEFAULT_ACCOUNT_ADDR)
                .with_session_code(CONTRACT, session_args)
                .with_empty_payment_bytes(payment_args.clone())
                .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
                .with_deploy_hash(rng.gen())
                .build();
            let exec_request = ExecuteRequestBuilder::from_deploy_item(deploy).build();

            let data_dir = TempDir::new().unwrap();
            let mut builder = LmdbWasmTestBuilder::new_with_production_chainspec(data_dir.path());
            builder
                .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST)
                .exec(exec_request);
            let err = builder.get_error().unwrap();
            tx.send(err).unwrap();
        });

        match rx.recv_timeout(TIMEOUT) {
            Ok(error) => assert!(
                matches!(error, Error::Exec(ExecError::GasLimit)),
                "expected gas limit error, but got {:?}",
                error
            ),
            Err(RecvTimeoutError::Timeout) => panic!("timed out"),
            Err(RecvTimeoutError::Disconnected) => unreachable!(),
        }
    }

    #[ignore]
    #[test]
    fn write_small() {
        let session_args = runtime_args! {
            "fn" => "write",
            "len" => 1_u32,
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn write_large() {
        let session_args = runtime_args! {
            "fn" => "write",
            "len" => 100_000_u32,
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn read_missing() {
        let session_args = runtime_args! {
            "fn" => "read",
            "len" => Option::<u32>::None,
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn read_small() {
        let session_args = runtime_args! {
            "fn" => "read",
            "len" => Some(1_u32),
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn read_large() {
        let session_args = runtime_args! {
            "fn" => "read",
            "len" => Some(100_000_u32),
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn add_small() {
        let session_args = runtime_args! {
            "fn" => "add",
            "large" => false
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn add_large() {
        let session_args = runtime_args! {
            "fn" => "add",
            "large" => true
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn new_uref_small() {
        let session_args = runtime_args! {
            "fn" => "new",
            "len" => 1_u32,
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn new_uref_large() {
        let session_args = runtime_args! {
            "fn" => "new",
            "len" => 1_000_u32,
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn call_contract_small_runtime_args() {
        let session_args = runtime_args! {
            "fn" => "call_contract",
            "args_len" => 1_u32
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn call_contract_large_runtime_args() {
        // TODO - use production chainspec value for `[deploy.session_args_max_length]`.
        let session_args = runtime_args! {
            "fn" => "call_contract",
            "args_len" => 1_024_u32
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn get_key_small_name_missing_key() {
        let session_args = runtime_args! {
            "fn" => "get_key",
            "large_name" => false,
            "large_key" => Option::<bool>::None
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn get_key_small_name_small_key() {
        let session_args = runtime_args! {
            "fn" => "get_key",
            "large_name" => false,
            "large_key" => Some(false)
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn get_key_small_name_large_key() {
        let session_args = runtime_args! {
            "fn" => "get_key",
            "large_name" => false,
            "large_key" => Some(true)
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn get_key_large_name_small_key() {
        let session_args = runtime_args! {
            "fn" => "get_key",
            "large_name" => true,
            "large_key" => Some(false)
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn get_key_large_name_large_key() {
        let session_args = runtime_args! {
            "fn" => "get_key",
            "large_name" => true,
            "large_key" => Some(true)
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn has_key_small_name_missing_key() {
        let session_args = runtime_args! {
            "fn" => "has_key",
            "large_name" => false,
            "key_exists" => false
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn has_key_small_name_existing_key() {
        let session_args = runtime_args! {
            "fn" => "has_key",
            "large_name" => false,
            "key_exists" => true
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn has_key_large_name_missing_key() {
        let session_args = runtime_args! {
            "fn" => "has_key",
            "large_name" => true,
            "key_exists" => false
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn has_key_large_name_existing_key() {
        let session_args = runtime_args! {
            "fn" => "has_key",
            "large_name" => true,
            "key_exists" => true
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn put_key_small_name_small_key() {
        let session_args = runtime_args! {
            "fn" => "put_key",
            "large_name" => false,
            "large_key" => false
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn put_key_small_name_large_key() {
        let session_args = runtime_args! {
            "fn" => "put_key",
            "large_name" => false,
            "large_key" => true
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn put_key_large_name_small_key() {
        let session_args = runtime_args! {
            "fn" => "put_key",
            "large_name" => true,
            "large_key" => false
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn put_key_large_name_large_key() {
        let session_args = runtime_args! {
            "fn" => "put_key",
            "large_name" => true,
            "large_key" => true
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn is_valid_uref_for_invalid() {
        let session_args = runtime_args! {
            "fn" => "is_valid_uref",
            "valid" => false
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn is_valid_uref_for_valid() {
        let session_args = runtime_args! {
            "fn" => "is_valid_uref",
            "valid" => true
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn add_and_remove_associated_key() {
        let session_args = runtime_args! {
            "fn" => "add_associated_key",
            "remove_after_adding" => true,
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn add_associated_key_duplicated() {
        let session_args = runtime_args! {
            "fn" => "add_associated_key",
            "remove_after_adding" => false,
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn remove_associated_key_non_existent() {
        let session_args = runtime_args! { "fn" => "remove_associated_key" };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn get_caller() {
        let session_args = runtime_args! { "fn" => "get_caller" };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn get_blocktime() {
        let session_args = runtime_args! { "fn" => "get_blocktime" };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn blake2b_small() {
        let session_args = runtime_args! {
            "fn" => "blake2b",
            "len" => 1_u32,
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn blake2b_large() {
        let session_args = runtime_args! {
            "fn" => "blake2b",
            "len" => 1_000_000_u32,
        };
        execute_with_timeout(session_args)
    }
}
