mod repeated_ffi_call_should_gas_out_quickly {
    use std::{
        env,
        sync::mpsc::{self, RecvTimeoutError},
        thread,
        time::{Duration, Instant},
    };

    use rand::Rng;
    use tempfile::TempDir;

    use casper_engine_test_support::{
        ChainspecConfig, DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder,
        DEFAULT_ACCOUNT_ADDR, PRODUCTION_CHAINSPEC_PATH, PRODUCTION_RUN_GENESIS_REQUEST,
    };
    use casper_execution_engine::core::{
        engine_state::{EngineConfig, Error},
        execution::Error as ExecError,
    };
    use casper_types::{
        account::AccountHash, runtime_args, testing::TestRng, RuntimeArgs,
        DICTIONARY_ITEM_KEY_MAX_LENGTH, U512,
    };

    const CONTRACT: &str = "regression_20240105.wasm";
    const TIMEOUT: Duration = Duration::from_secs(4);
    const PAYMENT_AMOUNT: u64 = 1_000_000_000_000_u64;

    fn production_max_associated_keys() -> u8 {
        let chainspec_config =
            ChainspecConfig::from_chainspec_path(&*PRODUCTION_CHAINSPEC_PATH).unwrap();
        chainspec_config.max_associated_keys().try_into().unwrap()
    }

    struct Fixture {
        builder: LmdbWasmTestBuilder,
        data_dir: TempDir,
        rng: TestRng,
    }

    impl Fixture {
        fn new() -> Self {
            let data_dir = TempDir::new().unwrap();
            let mut builder = LmdbWasmTestBuilder::new_with_production_chainspec(data_dir.path());
            builder
                .run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST)
                .commit();
            let rng = TestRng::new();
            Fixture {
                builder,
                data_dir,
                rng,
            }
        }

        /// Calls regression_20240105.wasm with some setup function.  Execution is expected to
        /// succeed.
        fn execute_setup(&mut self, session_args: RuntimeArgs) {
            let payment_args = runtime_args! { "amount" => U512::from(PAYMENT_AMOUNT * 4) };
            let deploy = DeployItemBuilder::new()
                .with_address(*DEFAULT_ACCOUNT_ADDR)
                .with_session_code(CONTRACT, session_args)
                .with_empty_payment_bytes(payment_args)
                .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
                .with_deploy_hash(self.rng.gen())
                .build();
            let exec_request = ExecuteRequestBuilder::from_deploy_item(deploy).build();
            self.builder.exec(exec_request).expect_success().commit();
        }

        /// Calls regression_20240105.wasm with expectation of execution failure due to running out
        /// of gas within the duration specified in `TIMEOUT`.
        fn execute_with_timeout(self, session_args: RuntimeArgs, extra_auth_keys: u8) {
            if cfg!(debug_assertions) {
                println!("not testing in debug mode");
                return;
            }
            let (tx, rx) = mpsc::channel();
            let Fixture {
                builder,
                data_dir,
                mut rng,
            } = self;
            let post_state_hash = builder.get_post_state_hash();
            let mut auth_keys = Vec::new();
            auth_keys.push(*DEFAULT_ACCOUNT_ADDR);
            for i in 1..=extra_auth_keys {
                auth_keys.push(AccountHash::new([i; 32]));
            }
            let executor = thread::spawn(move || {
                let payment_args = runtime_args! { "amount" => U512::from(PAYMENT_AMOUNT) };
                let deploy = DeployItemBuilder::new()
                    .with_address(*DEFAULT_ACCOUNT_ADDR)
                    .with_session_code(CONTRACT, session_args)
                    .with_empty_payment_bytes(payment_args.clone())
                    .with_authorization_keys(&auth_keys)
                    .with_deploy_hash(rng.gen())
                    .build();
                let exec_request = ExecuteRequestBuilder::from_deploy_item(deploy).build();

                let chainspec_config =
                    ChainspecConfig::from_chainspec_path(&*PRODUCTION_CHAINSPEC_PATH).unwrap();
                let mut engine_config = EngineConfig::from(chainspec_config);
                // Increase the `max_memory` available in order to avoid hitting unreachable
                // instruction during execution.
                engine_config.set_max_memory(10_000);
                let mut builder =
                    LmdbWasmTestBuilder::open(data_dir.path(), engine_config, post_state_hash);
                let error = match builder.try_exec(exec_request) {
                    Ok(_) => builder.get_error().unwrap(),
                    Err(error) => error,
                };
                let _ = tx.send(error);
            });

            let timeout = if let Ok(value) = env::var("CL_TEST_TIMEOUT_SECS") {
                Duration::from_secs(value.parse().expect("should parse as u64"))
            } else {
                TIMEOUT
            };
            let start = Instant::now();
            let receiver_result = rx.recv_timeout(timeout);
            executor.join().unwrap();
            match receiver_result {
                Ok(error) => {
                    assert!(
                        matches!(error, Error::Exec(ExecError::GasLimit)),
                        "expected gas limit error, but got {:?}",
                        error
                    );
                }
                Err(RecvTimeoutError::Timeout) => {
                    panic!(
                        "execution should take less than {} seconds, but took {} seconds ",
                        timeout.as_secs_f32(),
                        start.elapsed().as_secs_f32(),
                    )
                }
                Err(RecvTimeoutError::Disconnected) => unreachable!(),
            }
        }
    }

    #[ignore]
    #[test]
    fn write_small() {
        let session_args = runtime_args! {
            "fn" => "write",
            "len" => 1_u32,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn write_large() {
        let session_args = runtime_args! {
            "fn" => "write",
            "len" => 100_000_u32,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn read_missing() {
        let session_args = runtime_args! {
            "fn" => "read",
            "len" => Option::<u32>::None,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn read_small() {
        let session_args = runtime_args! {
            "fn" => "read",
            "len" => Some(1_u32),
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn read_large() {
        let session_args = runtime_args! {
            "fn" => "read",
            "len" => Some(100_000_u32),
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn add_small() {
        let session_args = runtime_args! {
            "fn" => "add",
            "large" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn add_large() {
        let session_args = runtime_args! {
            "fn" => "add",
            "large" => true
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn new_uref_small() {
        let session_args = runtime_args! {
            "fn" => "new",
            "len" => 1_u32,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn new_uref_large() {
        let session_args = runtime_args! {
            "fn" => "new",
            "len" => 1_000_u32,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn call_contract_small_runtime_args() {
        let session_args = runtime_args! {
            "fn" => "call_contract",
            "args_len" => 1_u32
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn call_contract_large_runtime_args() {
        let session_args = runtime_args! {
            "fn" => "call_contract",
            "args_len" => 1_024_u32
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn get_key_small_name_missing_key() {
        let session_args = runtime_args! {
            "fn" => "get_key",
            "large_name" => false,
            "large_key" => Option::<bool>::None
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn get_key_small_name_small_key() {
        let session_args = runtime_args! {
            "fn" => "get_key",
            "large_name" => false,
            "large_key" => Some(false)
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn get_key_small_name_large_key() {
        let session_args = runtime_args! {
            "fn" => "get_key",
            "large_name" => false,
            "large_key" => Some(true)
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn get_key_large_name_small_key() {
        let session_args = runtime_args! {
            "fn" => "get_key",
            "large_name" => true,
            "large_key" => Some(false)
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn get_key_large_name_large_key() {
        let session_args = runtime_args! {
            "fn" => "get_key",
            "large_name" => true,
            "large_key" => Some(true)
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn has_key_small_name_missing_key() {
        let session_args = runtime_args! {
            "fn" => "has_key",
            "large_name" => false,
            "key_exists" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn has_key_small_name_existing_key() {
        let session_args = runtime_args! {
            "fn" => "has_key",
            "large_name" => false,
            "key_exists" => true
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn has_key_large_name_missing_key() {
        let session_args = runtime_args! {
            "fn" => "has_key",
            "large_name" => true,
            "key_exists" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn has_key_large_name_existing_key() {
        let session_args = runtime_args! {
            "fn" => "has_key",
            "large_name" => true,
            "key_exists" => true
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn put_key_small_name_small_key() {
        let session_args = runtime_args! {
            "fn" => "put_key",
            "large_name" => false,
            "large_key" => false,
            "num_keys" => Option::<u32>::None
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn put_key_small_name_large_key() {
        let session_args = runtime_args! {
            "fn" => "put_key",
            "large_name" => false,
            "large_key" => true,
            "num_keys" => Option::<u32>::None
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn put_key_large_name_small_key() {
        let session_args = runtime_args! {
            "fn" => "put_key",
            "large_name" => true,
            "large_key" => false,
            "num_keys" => Option::<u32>::None
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn put_key_large_name_large_key() {
        let session_args = runtime_args! {
            "fn" => "put_key",
            "large_name" => true,
            "large_key" => true,
            "num_keys" => Option::<u32>::None
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn is_valid_uref_for_invalid() {
        let session_args = runtime_args! {
            "fn" => "is_valid_uref",
            "valid" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn is_valid_uref_for_valid() {
        let session_args = runtime_args! {
            "fn" => "is_valid_uref",
            "valid" => true
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn add_and_remove_associated_key() {
        let session_args = runtime_args! {
            "fn" => "add_associated_key",
            "remove_after_adding" => true,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn add_associated_key_duplicated() {
        let session_args = runtime_args! {
            "fn" => "add_associated_key",
            "remove_after_adding" => false,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn remove_associated_key_non_existent() {
        let session_args = runtime_args! { "fn" => "remove_associated_key" };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn update_associated_key_non_existent() {
        let session_args = runtime_args! {
            "fn" => "update_associated_key",
            "exists" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn update_associated_key_existing() {
        let session_args = runtime_args! {
            "fn" => "update_associated_key",
            "exists" => true
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn set_action_threshold() {
        let session_args = runtime_args! { "fn" => "set_action_threshold" };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn load_named_keys_empty() {
        let session_args = runtime_args! {
            "fn" => "load_named_keys",
            "num_keys" => 0_u32,
            "large_name" => true
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn load_named_keys_one_key_small_name() {
        let num_keys = 1_u32;
        let mut fixture = Fixture::new();
        let session_args = runtime_args! {
            "fn" => "put_key",
            "large_name" => false,
            "large_key" => true,
            "num_keys" => Some(num_keys),
        };
        fixture.execute_setup(session_args);
        let session_args = runtime_args! {
            "fn" => "load_named_keys",
            "num_keys" => num_keys
        };
        fixture.execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn load_named_keys_one_key_large_name() {
        let num_keys = 1_u32;
        let mut fixture = Fixture::new();
        let session_args = runtime_args! {
            "fn" => "put_key",
            "large_name" => true,
            "large_key" => true,
            "num_keys" => Some(num_keys),
        };
        fixture.execute_setup(session_args);
        let session_args = runtime_args! {
            "fn" => "load_named_keys",
            "num_keys" => num_keys,
        };
        fixture.execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn load_named_keys_many_keys_small_name() {
        let num_keys = 1_000_u32;
        let mut fixture = Fixture::new();
        let session_args = runtime_args! {
            "fn" => "put_key",
            "large_name" => false,
            "large_key" => true,
            "num_keys" => Some(num_keys),
        };
        fixture.execute_setup(session_args);
        let session_args = runtime_args! {
            "fn" => "load_named_keys",
            "num_keys" => num_keys,
        };
        fixture.execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn load_named_keys_many_keys_large_name() {
        let num_keys = 10_u32;
        let mut fixture = Fixture::new();
        let session_args = runtime_args! {
            "fn" => "put_key",
            "large_name" => true,
            "large_key" => true,
            "num_keys" => Some(num_keys),
        };
        fixture.execute_setup(session_args);
        let session_args = runtime_args! {
            "fn" => "load_named_keys",
            "num_keys" => num_keys,
        };
        fixture.execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn remove_key_small_name() {
        let session_args = runtime_args! {
            "fn" => "remove_key",
            "large_name" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn remove_key_large_name() {
        let session_args = runtime_args! {
            "fn" => "remove_key",
            "large_name" => true
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn get_caller() {
        let session_args = runtime_args! { "fn" => "get_caller" };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn get_blocktime() {
        let session_args = runtime_args! { "fn" => "get_blocktime" };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn create_purse() {
        let session_args = runtime_args! { "fn" => "create_purse" };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn transfer_to_account_create_account() {
        let session_args = runtime_args! {
            "fn" => "transfer_to_account",
            "account_exists" => false,
            "amount" => U512::MAX
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn transfer_to_account_existing_account() {
        let session_args = runtime_args! {
            "fn" => "transfer_to_account",
            "account_exists" => true,
            "amount" => U512::MAX
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn transfer_from_purse_to_account_create_account() {
        let session_args = runtime_args! {
            "fn" => "transfer_from_purse_to_account",
            "account_exists" => false,
            "amount" => U512::MAX
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn transfer_from_purse_to_account_existing_account() {
        let session_args = runtime_args! {
            "fn" => "transfer_from_purse_to_account",
            "account_exists" => true,
            "amount" => U512::MAX
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn transfer_from_purse_to_purse() {
        let session_args = runtime_args! {
            "fn" => "transfer_from_purse_to_purse",
            "amount" => U512::MAX
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn get_balance_non_existent_purse() {
        let session_args = runtime_args! {
            "fn" => "get_balance",
            "purse_exists" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn get_balance_existing_purse() {
        let session_args = runtime_args! {
            "fn" => "get_balance",
            "purse_exists" => true
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn get_phase() {
        let session_args = runtime_args! { "fn" => "get_phase" };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn get_system_contract() {
        let session_args = runtime_args! { "fn" => "get_system_contract" };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn get_main_purse() {
        let session_args = runtime_args! { "fn" => "get_main_purse" };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn read_host_buffer_empty() {
        let session_args = runtime_args! { "fn" => "read_host_buffer" };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn create_contract_package_at_hash() {
        let session_args = runtime_args! { "fn" => "create_contract_package_at_hash" };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn add_contract_version_no_entry_points_no_named_keys() {
        let session_args = runtime_args! {
            "fn" => "add_contract_version",
            "entry_points_len" => 0_u32,
            "named_keys_len" => 0_u32,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn add_contract_version_small_entry_points_small_named_keys() {
        let session_args = runtime_args! {
            "fn" => "add_contract_version",
            "entry_points_len" => 1_u32,
            "named_keys_len" => 1_u32,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn add_contract_version_small_entry_points_large_named_keys() {
        let session_args = runtime_args! {
            "fn" => "add_contract_version",
            "entry_points_len" => 1_u32,
            "named_keys_len" => 100_u32,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn add_contract_version_large_entry_points_small_named_keys() {
        let session_args = runtime_args! {
            "fn" => "add_contract_version",
            "entry_points_len" => 100_u32,
            "named_keys_len" => 1_u32,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn add_contract_version_large_entry_points_large_named_keys() {
        let session_args = runtime_args! {
            "fn" => "add_contract_version",
            "entry_points_len" => 100_u32,
            "named_keys_len" => 100_u32,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn disable_contract_version() {
        let session_args = runtime_args! { "fn" => "disable_contract_version" };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn call_versioned_contract_small_runtime_args() {
        let session_args = runtime_args! {
            "fn" => "call_versioned_contract",
            "args_len" => 1_u32
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn call_versioned_contract_large_runtime_args() {
        let session_args = runtime_args! {
            "fn" => "call_versioned_contract",
            "args_len" => 1_024_u32
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn create_contract_user_group_small_label_no_new_urefs_no_existing_urefs() {
        let session_args = runtime_args! {
            "fn" => "create_contract_user_group",
            "label_len" => 1_u32,
            "num_new_urefs" => 0_u8,
            "num_existing_urefs" => 0_u8,
            "allow_exceeding_max_groups" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn create_contract_user_group_small_label_no_new_urefs_few_existing_urefs() {
        let session_args = runtime_args! {
            "fn" => "create_contract_user_group",
            "label_len" => 1_u32,
            "num_new_urefs" => 0_u8,
            "num_existing_urefs" => 1_u8,
            "allow_exceeding_max_groups" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn create_contract_user_group_small_label_no_new_urefs_many_existing_urefs() {
        let session_args = runtime_args! {
            "fn" => "create_contract_user_group",
            "label_len" => 1_u32,
            "num_new_urefs" => 0_u8,
            "num_existing_urefs" => 10_u8,
            "allow_exceeding_max_groups" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn create_contract_user_group_small_label_few_new_urefs_no_existing_urefs() {
        let session_args = runtime_args! {
            "fn" => "create_contract_user_group",
            "label_len" => 1_u32,
            "num_new_urefs" => 1_u8,
            "num_existing_urefs" => 0_u8,
            "allow_exceeding_max_groups" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn create_contract_user_group_small_label_few_new_urefs_few_existing_urefs() {
        let session_args = runtime_args! {
            "fn" => "create_contract_user_group",
            "label_len" => 1_u32,
            "num_new_urefs" => 1_u8,
            "num_existing_urefs" => 1_u8,
            "allow_exceeding_max_groups" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn create_contract_user_group_small_label_few_new_urefs_many_existing_urefs() {
        let session_args = runtime_args! {
            "fn" => "create_contract_user_group",
            "label_len" => 1_u32,
            "num_new_urefs" => 1_u8,
            "num_existing_urefs" => 5_u8,
            "allow_exceeding_max_groups" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn create_contract_user_group_small_label_many_new_urefs_no_existing_urefs() {
        let session_args = runtime_args! {
            "fn" => "create_contract_user_group",
            "label_len" => 1_u32,
            "num_new_urefs" => 5_u8,
            "num_existing_urefs" => 0_u8,
            "allow_exceeding_max_groups" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn create_contract_user_group_small_label_many_new_urefs_few_existing_urefs() {
        let session_args = runtime_args! {
            "fn" => "create_contract_user_group",
            "label_len" => 1_u32,
            "num_new_urefs" => 5_u8,
            "num_existing_urefs" => 1_u8,
            "allow_exceeding_max_groups" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn create_contract_user_group_small_label_many_new_urefs_many_existing_urefs() {
        let session_args = runtime_args! {
            "fn" => "create_contract_user_group",
            "label_len" => 1_u32,
            "num_new_urefs" => 5_u8,
            "num_existing_urefs" => 5_u8,
            "allow_exceeding_max_groups" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn create_contract_user_group_large_label_no_new_urefs_no_existing_urefs() {
        let session_args = runtime_args! {
            "fn" => "create_contract_user_group",
            "label_len" => 1_000_000_u32,
            "num_new_urefs" => 0_u8,
            "num_existing_urefs" => 0_u8,
            "allow_exceeding_max_groups" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn create_contract_user_group_large_label_no_new_urefs_few_existing_urefs() {
        let session_args = runtime_args! {
            "fn" => "create_contract_user_group",
            "label_len" => 1_000_000_u32,
            "num_new_urefs" => 0_u8,
            "num_existing_urefs" => 1_u8,
            "allow_exceeding_max_groups" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn create_contract_user_group_large_label_no_new_urefs_many_existing_urefs() {
        let session_args = runtime_args! {
            "fn" => "create_contract_user_group",
            "label_len" => 1_000_000_u32,
            "num_new_urefs" => 0_u8,
            "num_existing_urefs" => 10_u8,
            "allow_exceeding_max_groups" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn create_contract_user_group_large_label_few_new_urefs_no_existing_urefs() {
        let session_args = runtime_args! {
            "fn" => "create_contract_user_group",
            "label_len" => 1_000_000_u32,
            "num_new_urefs" => 1_u8,
            "num_existing_urefs" => 0_u8,
            "allow_exceeding_max_groups" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn create_contract_user_group_large_label_few_new_urefs_few_existing_urefs() {
        let session_args = runtime_args! {
            "fn" => "create_contract_user_group",
            "label_len" => 1_000_000_u32,
            "num_new_urefs" => 1_u8,
            "num_existing_urefs" => 1_u8,
            "allow_exceeding_max_groups" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn create_contract_user_group_large_label_few_new_urefs_many_existing_urefs() {
        let session_args = runtime_args! {
            "fn" => "create_contract_user_group",
            "label_len" => 1_000_000_u32,
            "num_new_urefs" => 1_u8,
            "num_existing_urefs" => 5_u8,
            "allow_exceeding_max_groups" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn create_contract_user_group_large_label_many_new_urefs_no_existing_urefs() {
        let session_args = runtime_args! {
            "fn" => "create_contract_user_group",
            "label_len" => 1_000_000_u32,
            "num_new_urefs" => 5_u8,
            "num_existing_urefs" => 0_u8,
            "allow_exceeding_max_groups" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn create_contract_user_group_large_label_many_new_urefs_few_existing_urefs() {
        let session_args = runtime_args! {
            "fn" => "create_contract_user_group",
            "label_len" => 1_000_000_u32,
            "num_new_urefs" => 5_u8,
            "num_existing_urefs" => 1_u8,
            "allow_exceeding_max_groups" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn create_contract_user_group_large_label_many_new_urefs_many_existing_urefs() {
        let session_args = runtime_args! {
            "fn" => "create_contract_user_group",
            "label_len" => 1_000_000_u32,
            "num_new_urefs" => 5_u8,
            "num_existing_urefs" => 5_u8,
            "allow_exceeding_max_groups" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn create_contract_user_group_failure_max_urefs_exceeded() {
        let session_args = runtime_args! {
            "fn" => "create_contract_user_group",
            "label_len" => 1_u32,
            "num_new_urefs" => u8::MAX,
            "num_existing_urefs" => 0_u8,
            "allow_exceeding_max_groups" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn create_contract_user_group_failure_max_groups_exceeded() {
        let session_args = runtime_args! {
            "fn" => "create_contract_user_group",
            "label_len" => 1_u32,
            "num_new_urefs" => 0_u8,
            "num_existing_urefs" => 0_u8,
            "allow_exceeding_max_groups" => true
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    // #[ignore]
    // #[test]
    // fn print_small() {
    //     let session_args = runtime_args! {
    //         "fn" => "print",
    //         "num_chars" => 1_u32
    //     };
    //     Fixture::new().execute_with_timeout(session_args, 0)
    // }
    //
    // #[ignore]
    // #[test]
    // fn print_large() {
    //     let session_args = runtime_args! {
    //         "fn" => "print",
    //         "num_chars" => 1_000_000_u32
    //     };
    //     Fixture::new().execute_with_timeout(session_args, 0)
    // }

    #[ignore]
    #[test]
    fn get_runtime_arg_size_zero() {
        let session_args = runtime_args! {
            "fn" => "get_runtime_arg_size",
            "arg" => ()
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn get_runtime_arg_size_small() {
        let session_args = runtime_args! {
            "fn" => "get_runtime_arg_size",
            "arg" => 1_u8
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn get_runtime_arg_size_large() {
        let session_args = runtime_args! {
            "fn" => "get_runtime_arg_size",
            "arg" => [1_u8; 1_000_000]
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn get_runtime_arg_zero_size() {
        let session_args = runtime_args! {
            "fn" => "get_runtime_arg",
            "arg" => ()
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn get_runtime_arg_small_size() {
        let session_args = runtime_args! {
            "fn" => "get_runtime_arg",
            "arg" => 1_u8
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn get_runtime_arg_large_size() {
        let session_args = runtime_args! {
            "fn" => "get_runtime_arg",
            "arg" => [1_u8; 1_000_000]
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn remove_contract_user_group() {
        let session_args = runtime_args! { "fn" => "remove_contract_user_group" };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn extend_contract_user_group_urefs_and_remove_as_required() {
        let session_args = runtime_args! {
            "fn" => "extend_contract_user_group_urefs",
            "allow_exceeding_max_urefs" => false
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn extend_contract_user_group_urefs_failure_max_urefs_exceeded() {
        let session_args = runtime_args! {
            "fn" => "extend_contract_user_group_urefs",
            "allow_exceeding_max_urefs" => true
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn remove_contract_user_group_urefs() {
        let session_args = runtime_args! { "fn" => "remove_contract_user_group_urefs" };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn blake2b_small() {
        let session_args = runtime_args! {
            "fn" => "blake2b",
            "len" => 1_u32
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn blake2b_large() {
        let session_args = runtime_args! {
            "fn" => "blake2b",
            "len" => 1_000_000_u32
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn new_dictionary() {
        let session_args = runtime_args! { "fn" => "new_dictionary" };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn dictionary_get_small_name_small_value() {
        let session_args = runtime_args! {
            "fn" => "dictionary_get",
            "name_len" => 1_u32,
            "value_len" => 1_u32,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn dictionary_get_small_name_large_value() {
        let session_args = runtime_args! {
            "fn" => "dictionary_get",
            "name_len" => 1_u32,
            "value_len" => 1_000_u32,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn dictionary_get_large_name_small_value() {
        let session_args = runtime_args! {
            "fn" => "dictionary_get",
            "name_len" => DICTIONARY_ITEM_KEY_MAX_LENGTH as u32 - 4,
            "value_len" => 1_u32,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn dictionary_get_large_name_large_value() {
        let session_args = runtime_args! {
            "fn" => "dictionary_get",
            "name_len" => DICTIONARY_ITEM_KEY_MAX_LENGTH as u32 - 4,
            "value_len" => 1_000_u32,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn dictionary_put_small_name_small_value() {
        let session_args = runtime_args! {
            "fn" => "dictionary_put",
            "name_len" => 1_u32,
            "value_len" => 1_u32,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn dictionary_put_small_name_large_value() {
        let session_args = runtime_args! {
            "fn" => "dictionary_put",
            "name_len" => 1_u32,
            "value_len" => 1_000_u32,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn dictionary_put_large_name_small_value() {
        let session_args = runtime_args! {
            "fn" => "dictionary_put",
            "name_len" => DICTIONARY_ITEM_KEY_MAX_LENGTH as u32 - 4,
            "value_len" => 1_u32,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn dictionary_put_large_name_large_value() {
        let session_args = runtime_args! {
            "fn" => "dictionary_put",
            "name_len" => DICTIONARY_ITEM_KEY_MAX_LENGTH as u32 - 4,
            "value_len" => 1_000_u32,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn load_call_stack() {
        let session_args = runtime_args! { "fn" => "load_call_stack" };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn load_authorization_keys_small() {
        let session_args = runtime_args! {
            "fn" => "load_authorization_keys",
            "setup" => false,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn load_authorization_keys_large() {
        let session_args = runtime_args! {
            "fn" => "load_authorization_keys",
            "setup" => true,
        };
        let mut fixture = Fixture::new();
        fixture.execute_setup(session_args);
        let session_args = runtime_args! {
            "fn" => "load_authorization_keys",
            "setup" => false,
        };
        fixture.execute_with_timeout(session_args, production_max_associated_keys() - 1)
    }

    #[ignore]
    #[test]
    fn random_bytes() {
        let session_args = runtime_args! { "fn" => "random_bytes" };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn dictionary_read_small_name_small_value() {
        let session_args = runtime_args! {
            "fn" => "dictionary_read",
            "name_len" => 1_u32,
            "value_len" => 1_u32,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn dictionary_read_small_name_large_value() {
        let session_args = runtime_args! {
            "fn" => "dictionary_read",
            "name_len" => 1_u32,
            "value_len" => 1_000_u32,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn dictionary_read_large_name_small_value() {
        let session_args = runtime_args! {
            "fn" => "dictionary_read",
            "name_len" => DICTIONARY_ITEM_KEY_MAX_LENGTH as u32 - 4,
            "value_len" => 1_u32,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn dictionary_read_large_name_large_value() {
        let session_args = runtime_args! {
            "fn" => "dictionary_read",
            "name_len" => DICTIONARY_ITEM_KEY_MAX_LENGTH as u32 - 4,
            "value_len" => 1_000_u32,
        };
        Fixture::new().execute_with_timeout(session_args, 0)
    }

    #[ignore]
    #[test]
    fn enable_contract_version() {
        let session_args = runtime_args! { "fn" => "enable_contract_version" };
        Fixture::new().execute_with_timeout(session_args, 0)
    }
}
