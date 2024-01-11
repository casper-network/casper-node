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
        if cfg!(debug_assertions) {
            println!("not testing in debug mode");
            return;
        }
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
            "len" => 1_000_000_u32,
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
            "len" => Some(50_000_u32),
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
    fn new_uref() {
        let session_args = runtime_args! { "fn" => "new" };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn has_key_missing() {
        let session_args = runtime_args! {
            "fn" => "has_key",
            "exists" => false
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn has_key_existing() {
        let session_args = runtime_args! {
            "fn" => "has_key",
            "exists" => true
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn put_key_small() {
        let session_args = runtime_args! {
            "fn" => "put_key",
            "large" => false
        };
        execute_with_timeout(session_args)
    }

    #[ignore]
    #[test]
    fn put_key_large() {
        let session_args = runtime_args! {
            "fn" => "put_key",
            "large" => true
        };
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
