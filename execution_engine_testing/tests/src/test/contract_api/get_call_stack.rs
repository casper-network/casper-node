use num_traits::One;

use casper_engine_test_support::{
    ExecuteRequestBuilder, InMemoryWasmTestBuilder, WasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    PRODUCTION_RUN_GENESIS_REQUEST,
};
use casper_execution_engine::storage::global_state::in_memory::InMemoryGlobalState;
use casper_types::{
    account::Account, runtime_args, system::CallStackElement, CLValue, ContractHash,
    ContractPackageHash, EntryPointType, HashAddr, Key, RuntimeArgs, StoredValue, U512,
};

use get_call_stack_recursive_subcall::{
    Call, ContractAddress, ARG_CALLS, ARG_CURRENT_DEPTH, METHOD_FORWARDER_CONTRACT_NAME,
    METHOD_FORWARDER_SESSION_NAME,
};

const CONTRACT_RECURSIVE_SUBCALL: &str = "get_call_stack_recursive_subcall.wasm";
const CONTRACT_CALL_RECURSIVE_SUBCALL: &str = "get_call_stack_call_recursive_subcall.wasm";

const CONTRACT_PACKAGE_NAME: &str = "forwarder";
const CONTRACT_NAME: &str = "our_contract_name";

const CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT: &str = METHOD_FORWARDER_CONTRACT_NAME;
const CONTRACT_FORWARDER_ENTRYPOINT_SESSION: &str = METHOD_FORWARDER_SESSION_NAME;

fn stored_session(contract_hash: ContractHash) -> Call {
    Call {
        contract_address: ContractAddress::ContractHash(contract_hash),
        target_method: CONTRACT_FORWARDER_ENTRYPOINT_SESSION.to_string(),
        entry_point_type: EntryPointType::Session,
    }
}

fn stored_versioned_session(contract_package_hash: ContractPackageHash) -> Call {
    Call {
        contract_address: ContractAddress::ContractPackageHash(contract_package_hash),
        target_method: CONTRACT_FORWARDER_ENTRYPOINT_SESSION.to_string(),
        entry_point_type: EntryPointType::Session,
    }
}

fn stored_contract(contract_hash: ContractHash) -> Call {
    Call {
        contract_address: ContractAddress::ContractHash(contract_hash),
        target_method: CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT.to_string(),
        entry_point_type: EntryPointType::Contract,
    }
}

fn stored_versioned_contract(contract_package_hash: ContractPackageHash) -> Call {
    Call {
        contract_address: ContractAddress::ContractPackageHash(contract_package_hash),
        target_method: CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT.to_string(),
        entry_point_type: EntryPointType::Contract,
    }
}

fn store_contract(builder: &mut WasmTestBuilder<InMemoryGlobalState>, session_filename: &str) {
    let store_contract_request =
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, session_filename, runtime_args! {})
            .build();
    builder
        .exec(store_contract_request)
        .commit()
        .expect_success();
}

// Constant from the contracts used in the tests below.
const LARGE_AMOUNT: u64 = 1_500_000_000_000;

// In the payment or session phase, this test will try to transfer `len + 1` times
// a fixed amount of `1_500_000_000_000` from the main purse of the account.
// We need to provide an explicit approval via passing that as an `amount` argument.
pub fn approved_amount(idx: usize) -> U512 {
    U512::from(LARGE_AMOUNT * (idx + 1) as u64)
}

trait AccountExt {
    fn get_hash(&self, key: &str) -> HashAddr;
}

impl AccountExt for Account {
    fn get_hash(&self, key: &str) -> HashAddr {
        self.named_keys()
            .get(key)
            .cloned()
            .and_then(Key::into_hash)
            .unwrap()
    }
}

trait BuilderExt {
    fn get_call_stack_from_session_context(
        &mut self,
        stored_call_stack_key: &str,
    ) -> Vec<CallStackElement>;

    fn get_call_stack_from_contract_context(
        &mut self,
        stored_call_stack_key: &str,
        contract_package_hash: HashAddr,
    ) -> Vec<CallStackElement>;
}

impl BuilderExt for WasmTestBuilder<InMemoryGlobalState> {
    fn get_call_stack_from_session_context(
        &mut self,
        stored_call_stack_key: &str,
    ) -> Vec<CallStackElement> {
        let cl_value = self
            .query(
                None,
                (*DEFAULT_ACCOUNT_ADDR).into(),
                &[stored_call_stack_key.to_string()],
            )
            .unwrap();

        cl_value
            .as_cl_value()
            .cloned()
            .map(CLValue::into_t::<Vec<CallStackElement>>)
            .unwrap()
            .unwrap()
    }

    fn get_call_stack_from_contract_context(
        &mut self,
        stored_call_stack_key: &str,
        contract_package_hash: HashAddr,
    ) -> Vec<CallStackElement> {
        let value = self
            .query(None, Key::Hash(contract_package_hash), &[])
            .unwrap();

        let contract_package = match value {
            StoredValue::ContractPackage(package) => package,
            _ => panic!("unreachable"),
        };

        let current_contract_hash = contract_package.current_contract_hash().unwrap();

        let cl_value = self
            .query(
                None,
                current_contract_hash.into(),
                &[stored_call_stack_key.to_string()],
            )
            .unwrap();

        cl_value
            .as_cl_value()
            .cloned()
            .map(CLValue::into_t::<Vec<CallStackElement>>)
            .unwrap()
            .unwrap()
    }
}

fn setup() -> WasmTestBuilder<InMemoryGlobalState> {
    let mut builder = InMemoryWasmTestBuilder::default();
    builder.run_genesis(&PRODUCTION_RUN_GENESIS_REQUEST);
    store_contract(&mut builder, CONTRACT_RECURSIVE_SUBCALL);
    builder
}

fn assert_each_context_has_correct_call_stack_info(
    builder: &mut InMemoryWasmTestBuilder,
    top_level_call: Call,
    mut subcalls: Vec<Call>,
    current_contract_package_hash: HashAddr,
) {
    let mut calls = vec![top_level_call];
    calls.append(&mut subcalls);

    // query for and verify that all the elements in the call stack match their
    // pre-defined Call element
    for (i, call) in calls.iter().enumerate() {
        let stored_call_stack_key = format!("call_stack-{}", i);
        // we need to know where to look for the call stack information
        let call_stack = match call.entry_point_type {
            EntryPointType::Contract => builder.get_call_stack_from_contract_context(
                &stored_call_stack_key,
                current_contract_package_hash,
            ),
            EntryPointType::Session => {
                builder.get_call_stack_from_session_context(&stored_call_stack_key)
            }
        };
        assert_eq!(
            call_stack.len(),
            i + 2,
            "call stack len was an unexpected size {}, should be {} {:#?}",
            call_stack.len(),
            i + 2,
            call_stack,
        );
        let (head, rest) = call_stack.split_at(usize::one());

        assert_eq!(
            head,
            [CallStackElement::Session {
                account_hash: *DEFAULT_ACCOUNT_ADDR,
            }],
        );
        assert_call_stack_matches_calls(rest.to_vec(), &calls);
    }
}

fn assert_invalid_context(builder: &mut InMemoryWasmTestBuilder, depth: usize) {
    if depth == 0 {
        builder.expect_success();
    } else {
        let error = builder.get_error().unwrap();
        assert!(matches!(
            error,
            casper_execution_engine::core::engine_state::Error::Exec(
                casper_execution_engine::core::execution::Error::InvalidContext
            )
        ));
    }
}

fn assert_each_context_has_correct_call_stack_info_module_bytes(
    builder: &mut InMemoryWasmTestBuilder,
    subcalls: Vec<Call>,
    current_contract_package_hash: HashAddr,
) {
    let stored_call_stack_key = format!("call_stack-{}", 0);
    let call_stack = builder.get_call_stack_from_session_context(&stored_call_stack_key);
    let (head, _) = call_stack.split_at(usize::one());
    assert_eq!(
        head,
        [CallStackElement::Session {
            account_hash: *DEFAULT_ACCOUNT_ADDR,
        }],
    );

    for (i, call) in (1..=subcalls.len()).zip(subcalls.iter()) {
        let stored_call_stack_key = format!("call_stack-{}", i);
        // we need to know where to look for the call stack information
        let call_stack = match call.entry_point_type {
            EntryPointType::Contract => builder.get_call_stack_from_contract_context(
                &stored_call_stack_key,
                current_contract_package_hash,
            ),
            EntryPointType::Session => {
                builder.get_call_stack_from_session_context(&stored_call_stack_key)
            }
        };
        let (head, rest) = call_stack.split_at(usize::one());
        assert_eq!(
            head,
            [CallStackElement::Session {
                account_hash: *DEFAULT_ACCOUNT_ADDR,
            }],
        );
        assert_call_stack_matches_calls(rest.to_vec(), &subcalls);
    }
}

fn assert_call_stack_matches_calls(call_stack: Vec<CallStackElement>, calls: &[Call]) {
    for (index, expected_call_stack_element) in call_stack.iter().enumerate() {
        let maybe_call = calls.get(index);
        match (maybe_call, expected_call_stack_element) {
            // Versioned Call with EntryPointType::Contract
            (
                Some(Call {
                    entry_point_type,
                    contract_address:
                        ContractAddress::ContractPackageHash(current_contract_package_hash),
                    ..
                }),
                CallStackElement::StoredContract {
                    contract_package_hash,
                    ..
                },
            ) if *entry_point_type == EntryPointType::Contract
                && *contract_package_hash == *current_contract_package_hash => {}

            // Unversioned Call with EntryPointType::Contract
            (
                Some(Call {
                    entry_point_type,
                    contract_address: ContractAddress::ContractHash(current_contract_hash),
                    ..
                }),
                CallStackElement::StoredContract { contract_hash, .. },
            ) if *entry_point_type == EntryPointType::Contract
                && *contract_hash == *current_contract_hash => {}

            // Versioned Call with EntryPointType::Session
            (
                Some(Call {
                    entry_point_type,
                    contract_address:
                        ContractAddress::ContractPackageHash(current_contract_package_hash),
                    ..
                }),
                CallStackElement::StoredSession {
                    account_hash,
                    contract_package_hash,
                    ..
                },
            ) if *entry_point_type == EntryPointType::Session
                && *account_hash == *DEFAULT_ACCOUNT_ADDR
                && *contract_package_hash == *current_contract_package_hash => {}

            // Unversioned Call with EntryPointType::Session
            (
                Some(Call {
                    entry_point_type,
                    contract_address: ContractAddress::ContractHash(current_contract_hash),
                    ..
                }),
                CallStackElement::StoredSession {
                    account_hash,
                    contract_hash,
                    ..
                },
            ) if *entry_point_type == EntryPointType::Session
                && *account_hash == *DEFAULT_ACCOUNT_ADDR
                && *contract_hash == *current_contract_hash => {}

            _ => panic!(
                "call stack element {:#?} didn't match expected call {:#?} at index {}, {:#?}",
                expected_call_stack_element, maybe_call, index, call_stack,
            ),
        }
    }
}

mod session {
    use casper_engine_test_support::{ExecuteRequestBuilder, DEFAULT_ACCOUNT_ADDR};
    use casper_execution_engine::shared::transform::Transform;
    use casper_types::{runtime_args, system::mint, Key, RuntimeArgs};

    use super::{
        approved_amount, AccountExt, ARG_CALLS, ARG_CURRENT_DEPTH, CONTRACT_CALL_RECURSIVE_SUBCALL,
        CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT, CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
        CONTRACT_NAME, CONTRACT_PACKAGE_NAME,
    };

    // DEPTHS should not contain 1, as it will eliminate the initial element from the subcalls
    // vector
    const DEPTHS: &[usize] = &[0, 2, 5, 10];

    // Session + recursive subcall

    #[ignore]
    #[test]
    fn session_bytes_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let subcalls =
                vec![super::stored_versioned_contract(current_contract_package_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_CALL_RECURSIVE_SUBCALL,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info_module_bytes(
                &mut builder,
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_contract(current_contract_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_CALL_RECURSIVE_SUBCALL,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info_module_bytes(
                &mut builder,
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_versioned_contract_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    len.saturating_sub(1)
                ];
            if *len > 0 {
                subcalls.push(super::stored_contract(current_contract_hash.into()));
            }

            let execute_request = ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_CALL_RECURSIVE_SUBCALL,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info_module_bytes(
                &mut builder,
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_contract_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![super::stored_contract(current_contract_hash.into()); len.saturating_sub(1)];
            if *len > 0 {
                subcalls.push(super::stored_versioned_contract(
                    current_contract_package_hash.into(),
                ));
            }

            let execute_request = ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_CALL_RECURSIVE_SUBCALL,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info_module_bytes(
                &mut builder,
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_versioned_session_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_session(current_contract_package_hash.into());
                    len.saturating_sub(1)
                ];
            if *len > 0 {
                subcalls.push(super::stored_versioned_contract(
                    current_contract_package_hash.into(),
                ));
            }

            let execute_request = ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_CALL_RECURSIVE_SUBCALL,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info_module_bytes(
                &mut builder,
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_versioned_session_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_session(current_contract_package_hash.into());
                    len.saturating_sub(1)
                ];
            if *len > 0 {
                subcalls.push(super::stored_contract(current_contract_hash.into()));
            }

            let execute_request = ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_CALL_RECURSIVE_SUBCALL,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info_module_bytes(
                &mut builder,
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_session_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![super::stored_session(current_contract_hash.into()); len.saturating_sub(1)];
            if *len > 0 {
                subcalls.push(super::stored_versioned_contract(
                    current_contract_package_hash.into(),
                ));
            }

            let execute_request = ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_CALL_RECURSIVE_SUBCALL,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info_module_bytes(
                &mut builder,
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_session_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![super::stored_session(current_contract_hash.into()); len.saturating_sub(1)];
            if *len > 0 {
                subcalls.push(super::stored_contract(current_contract_hash.into()));
            }

            let execute_request = ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_CALL_RECURSIVE_SUBCALL,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info_module_bytes(
                &mut builder,
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_versioned_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let subcalls =
                vec![super::stored_versioned_session(current_contract_package_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_CALL_RECURSIVE_SUBCALL,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info_module_bytes(
                &mut builder,
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_versioned_session_to_stored_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_session(current_contract_package_hash.into());
                    len.saturating_sub(1)
                ];
            if *len > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()));
            }

            let execute_request = ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_CALL_RECURSIVE_SUBCALL,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info_module_bytes(
                &mut builder,
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_session_to_stored_versioned_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![super::stored_session(current_contract_hash.into()); len.saturating_sub(1)];
            if *len > 0 {
                subcalls.push(super::stored_versioned_session(
                    current_contract_package_hash.into(),
                ));
            }

            let execute_request = ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_CALL_RECURSIVE_SUBCALL,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info_module_bytes(
                &mut builder,
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_session(current_contract_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_CALL_RECURSIVE_SUBCALL,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info_module_bytes(
                &mut builder,
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    // Session + recursive subcall failure cases

    #[ignore]
    #[test]
    fn session_bytes_to_stored_versioned_contract_to_stored_versioned_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    len.saturating_sub(1)
                ];
            if *len > 0 {
                subcalls.push(super::stored_versioned_session(
                    current_contract_package_hash.into(),
                ));
            }

            let execute_request = ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_CALL_RECURSIVE_SUBCALL,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_versioned_contract_to_stored_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    len.saturating_sub(1)
                ];
            if *len > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()));
            }

            let execute_request = ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_CALL_RECURSIVE_SUBCALL,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_contract_to_stored_versioned_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![super::stored_contract(current_contract_hash.into()); len.saturating_sub(1)];
            if *len > 0 {
                subcalls.push(super::stored_versioned_session(
                    current_contract_package_hash.into(),
                ));
            }

            let execute_request = ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_CALL_RECURSIVE_SUBCALL,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_contract_to_stored_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![super::stored_contract(current_contract_hash.into()); len.saturating_sub(1)];
            if *len > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()));
            }

            let execute_request = ExecuteRequestBuilder::standard(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_CALL_RECURSIVE_SUBCALL,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    // Stored contract + recursive subcall

    #[ignore]
    #[test]
    fn stored_versioned_contract_by_name_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let subcalls =
                vec![super::stored_versioned_contract(current_contract_package_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_PACKAGE_NAME,
                None,
                CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_versioned_contract(current_contract_package_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_contract_by_hash_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let subcalls =
                vec![super::stored_versioned_contract(current_contract_package_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::versioned_contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                current_contract_package_hash.into(),
                None,
                CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            let transforms = builder.get_execution_journals().last().unwrap().clone();

            assert!(
                transforms.iter().any(|(key, transform)| key
                    == &Key::Hash(current_contract_package_hash)
                    && transform == &Transform::Identity),
                "Missing `Identity` transform for a contract package being called."
            );

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_versioned_contract(current_contract_package_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_contract_by_name_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_contract(current_contract_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_PACKAGE_NAME,
                None,
                CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_versioned_contract(current_contract_package_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_contract_by_hash_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_contract(current_contract_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::versioned_contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                current_contract_package_hash.into(),
                None,
                CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_versioned_contract(current_contract_package_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_contract_by_name_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls =
                vec![super::stored_versioned_contract(current_contract_package_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_NAME,
                CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_contract(current_contract_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_contract_by_hash_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls =
                vec![super::stored_versioned_contract(current_contract_package_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                current_contract_hash.into(),
                CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            let transforms = builder.get_execution_journals().last().unwrap().clone();

            assert!(
                transforms
                    .iter()
                    .any(|(key, transform)| key == &Key::Hash(current_contract_hash)
                        && transform == &Transform::Identity),
                "Missing `Identity` transform for a contract being called."
            );

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_versioned_contract(current_contract_package_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_contract_by_name_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_contract(current_contract_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_NAME,
                CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_contract(current_contract_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_contract_by_hash_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_contract(current_contract_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                current_contract_hash.into(),
                CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_contract(current_contract_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    // Stored contract + recursive subcall failure cases

    #[ignore]
    #[test]
    fn stored_versioned_contract_by_name_to_stored_versioned_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let subcalls =
                vec![super::stored_versioned_session(current_contract_package_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_PACKAGE_NAME,
                None,
                CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_contract_by_hash_to_stored_versioned_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let subcalls =
                vec![super::stored_versioned_session(current_contract_package_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::versioned_contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                current_contract_package_hash.into(),
                None,
                CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_contract_by_name_to_stored_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_session(current_contract_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_PACKAGE_NAME,
                None,
                CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_contract_by_hash_to_stored_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_session(current_contract_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::versioned_contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                current_contract_package_hash.into(),
                None,
                CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_contract_by_name_to_stored_versioned_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let subcalls =
                vec![super::stored_versioned_session(current_contract_package_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_NAME,
                CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_contract_by_hash_to_stored_versioned_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls =
                vec![super::stored_versioned_session(current_contract_package_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                current_contract_hash.into(),
                CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_contract_by_name_to_stored_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_session(current_contract_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_NAME,
                CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_contract_by_hash_to_stored_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_session(current_contract_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                current_contract_hash.into(),
                CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_contract_by_name_to_stored_versioned_contract_to_stored_versioned_session_should_fail(
    ) {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    len.saturating_sub(1)
                ];
            if *len > 0 {
                subcalls.push(super::stored_versioned_session(
                    current_contract_package_hash.into(),
                ))
            }

            let execute_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_PACKAGE_NAME,
                None,
                CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_contract_by_hash_to_stored_versioned_contract_to_stored_session_should_fail(
    ) {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    len.saturating_sub(1)
                ];
            if *len > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()))
            }

            let execute_request = ExecuteRequestBuilder::versioned_contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                current_contract_package_hash.into(),
                None,
                CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_contract_by_name_to_stored_contract_to_stored_versioned_session_should_fail(
    ) {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![super::stored_contract(current_contract_hash.into()); len.saturating_sub(1)];
            if *len > 0 {
                subcalls.push(super::stored_versioned_session(
                    current_contract_package_hash.into(),
                ))
            }

            let execute_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_PACKAGE_NAME,
                None,
                CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_contract_by_hash_to_stored_contract_to_stored_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![super::stored_contract(current_contract_hash.into()); len.saturating_sub(1)];
            if *len > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()))
            }

            let execute_request = ExecuteRequestBuilder::versioned_contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                current_contract_package_hash.into(),
                None,
                CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_contract_by_name_to_stored_versioned_contract_to_stored_versioned_session_should_fail(
    ) {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    len.saturating_sub(1)
                ];
            if *len > 0 {
                subcalls.push(super::stored_versioned_session(
                    current_contract_package_hash.into(),
                ))
            }

            let execute_request = ExecuteRequestBuilder::contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_NAME,
                CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_contract_by_hash_to_stored_versioned_contract_to_stored_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    len.saturating_sub(1)
                ];
            if *len > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()))
            }

            let execute_request = ExecuteRequestBuilder::contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                current_contract_hash.into(),
                CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_contract_by_name_to_stored_contract_to_stored_versioned_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![super::stored_contract(current_contract_hash.into()); len.saturating_sub(1)];
            if *len > 0 {
                subcalls.push(super::stored_versioned_session(
                    current_contract_package_hash.into(),
                ))
            }

            let execute_request = ExecuteRequestBuilder::contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_NAME,
                CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_contract_by_hash_to_stored_contract_to_stored_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![super::stored_contract(current_contract_hash.into()); len.saturating_sub(1)];
            if *len > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()))
            }

            let execute_request = ExecuteRequestBuilder::contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                current_contract_hash.into(),
                CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    // Stored session + recursive subcall

    #[ignore]
    #[test]
    fn stored_versioned_session_by_name_to_stored_versioned_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let subcalls =
                vec![super::stored_versioned_session(current_contract_package_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_PACKAGE_NAME,
                None,
                CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_versioned_session(current_contract_package_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_hash_to_stored_versioned_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let subcalls =
                vec![super::stored_versioned_session(current_contract_package_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::versioned_contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                current_contract_package_hash.into(),
                None,
                CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_versioned_session(current_contract_package_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_name_to_stored_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_session(current_contract_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_PACKAGE_NAME,
                None,
                CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_versioned_session(current_contract_package_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_hash_to_stored_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_session(current_contract_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::versioned_contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                current_contract_package_hash.into(),
                None,
                CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_versioned_session(current_contract_package_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_name_to_stored_versioned_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls =
                vec![super::stored_versioned_session(current_contract_package_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_NAME,
                CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_session(current_contract_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_hash_to_stored_versioned_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls =
                vec![super::stored_versioned_session(current_contract_package_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                current_contract_hash.into(),
                CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_versioned_session(current_contract_package_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_name_to_stored_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_session(current_contract_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_NAME,
                CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_session(current_contract_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_hash_to_stored_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_session(current_contract_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                current_contract_hash.into(),
                CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_session(current_contract_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_name_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let subcalls =
                vec![super::stored_versioned_contract(current_contract_package_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_PACKAGE_NAME,
                None,
                CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_versioned_session(current_contract_package_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_hash_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let subcalls =
                vec![super::stored_versioned_contract(current_contract_package_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::versioned_contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                current_contract_package_hash.into(),
                None,
                CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_versioned_session(current_contract_package_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_name_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_contract(current_contract_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_PACKAGE_NAME,
                None,
                CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_versioned_session(current_contract_package_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_hash_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_contract(current_contract_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::versioned_contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                current_contract_package_hash.into(),
                None,
                CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_versioned_session(current_contract_package_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_name_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls =
                vec![super::stored_versioned_contract(current_contract_package_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_NAME,
                CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_session(current_contract_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_hash_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls =
                vec![super::stored_versioned_contract(current_contract_package_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                current_contract_hash.into(),
                CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_session(current_contract_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_name_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_contract(current_contract_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_NAME,
                CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_session(current_contract_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_hash_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_contract(current_contract_hash.into()); *len];

            let execute_request = ExecuteRequestBuilder::contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                current_contract_hash.into(),
                CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_session(current_contract_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    // Stored session + recursive subcall failure cases

    #[ignore]
    #[test]
    fn stored_versioned_session_by_name_to_stored_versioned_contract_to_stored_versioned_session_should_fail(
    ) {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    len.saturating_sub(1)
                ];
            if *len > 0 {
                subcalls.push(super::stored_versioned_session(
                    current_contract_package_hash.into(),
                ))
            }

            let execute_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_PACKAGE_NAME,
                None,
                CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_hash_to_stored_versioned_contract_to_stored_session_should_fail()
    {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    len.saturating_sub(1)
                ];
            if *len > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()))
            }

            let execute_request = ExecuteRequestBuilder::versioned_contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                current_contract_package_hash.into(),
                None,
                CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_name_to_stored_contract_to_stored_versioned_session_should_fail()
    {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![super::stored_contract(current_contract_hash.into()); len.saturating_sub(1)];
            if *len > 0 {
                subcalls.push(super::stored_versioned_session(
                    current_contract_package_hash.into(),
                ))
            }

            let execute_request = ExecuteRequestBuilder::versioned_contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_PACKAGE_NAME,
                None,
                CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_hash_to_stored_contract_to_stored_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![super::stored_contract(current_contract_hash.into()); len.saturating_sub(1)];
            if *len > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()))
            }

            let execute_request = ExecuteRequestBuilder::versioned_contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                current_contract_package_hash.into(),
                None,
                CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_name_to_stored_versioned_contract_to_stored_versioned_session_should_fail()
    {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    len.saturating_sub(1)
                ];
            if *len > 0 {
                subcalls.push(super::stored_versioned_session(
                    current_contract_package_hash.into(),
                ))
            }

            let execute_request = ExecuteRequestBuilder::contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_NAME,
                CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_hash_to_stored_versioned_contract_to_stored_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    len.saturating_sub(1)
                ];
            if *len > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()))
            }

            let execute_request = ExecuteRequestBuilder::contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                current_contract_hash.into(),
                CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_name_to_stored_contract_to_stored_versioned_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![super::stored_contract(current_contract_hash.into()); len.saturating_sub(1)];
            if *len > 0 {
                subcalls.push(super::stored_versioned_session(
                    current_contract_package_hash.into(),
                ))
            }

            let execute_request = ExecuteRequestBuilder::contract_call_by_name(
                *DEFAULT_ACCOUNT_ADDR,
                CONTRACT_NAME,
                CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_hash_to_stored_contract_to_stored_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![super::stored_contract(current_contract_hash.into()); len.saturating_sub(1)];
            if *len > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()))
            }

            let execute_request = ExecuteRequestBuilder::contract_call_by_hash(
                *DEFAULT_ACCOUNT_ADDR,
                current_contract_hash.into(),
                CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                },
            )
            .build();

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }
}

mod payment {
    use rand::Rng;

    use casper_engine_test_support::{
        DeployItemBuilder, ExecuteRequestBuilder, DEFAULT_ACCOUNT_ADDR,
    };
    use casper_types::{runtime_args, system::mint, RuntimeArgs};

    use crate::wasm_utils;

    use super::{
        approved_amount, AccountExt, ARG_CALLS, ARG_CURRENT_DEPTH, CONTRACT_CALL_RECURSIVE_SUBCALL,
        CONTRACT_FORWARDER_ENTRYPOINT_SESSION, CONTRACT_NAME, CONTRACT_PACKAGE_NAME,
    };

    // DEPTHS should not contain 1, as it will eliminate the initial element from the subcalls
    // vector.  Going further than 5 will git the gas limit.
    const DEPTHS: &[usize] = &[0, 2, 5];

    // Session + recursive subcall

    #[ignore]
    #[test]
    fn session_bytes_to_stored_versioned_session_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_session(current_contract_package_hash.into());
                    len.saturating_sub(1)
                ];
            if *len > 0 {
                subcalls.push(super::stored_versioned_contract(
                    current_contract_package_hash.into(),
                ));
            }

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_payment_code(CONTRACT_CALL_RECURSIVE_SUBCALL, args)
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info_module_bytes(
                &mut builder,
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_versioned_session_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_session(current_contract_package_hash.into());
                    len.saturating_sub(1)
                ];
            if *len > 0 {
                subcalls.push(super::stored_contract(current_contract_hash.into()));
            }

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_payment_code(CONTRACT_CALL_RECURSIVE_SUBCALL, args)
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info_module_bytes(
                &mut builder,
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_session_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![super::stored_session(current_contract_hash.into()); len.saturating_sub(1)];
            if *len > 0 {
                subcalls.push(super::stored_versioned_contract(
                    current_contract_package_hash.into(),
                ));
            }

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_payment_code(CONTRACT_CALL_RECURSIVE_SUBCALL, args)
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info_module_bytes(
                &mut builder,
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_contract_to_stored_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![super::stored_session(current_contract_hash.into()); len.saturating_sub(1)];
            if *len > 0 {
                subcalls.push(super::stored_contract(current_contract_hash.into()));
            }

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_payment_code(CONTRACT_CALL_RECURSIVE_SUBCALL, args)
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };

            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info_module_bytes(
                &mut builder,
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    // Session + recursive subcall failure cases

    #[ignore]
    #[test]
    fn session_bytes_to_stored_versioned_contract_to_stored_versioned_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    len.saturating_sub(1)
                ];
            if *len > 0 {
                subcalls.push(super::stored_versioned_session(
                    current_contract_package_hash.into(),
                ));
            }

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_payment_code(CONTRACT_CALL_RECURSIVE_SUBCALL, args)
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_versioned_contract_to_stored_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    len.saturating_sub(1)
                ];
            if *len > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()));
            }

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_payment_code(CONTRACT_CALL_RECURSIVE_SUBCALL, args)
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_contract_to_stored_versioned_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![super::stored_contract(current_contract_hash.into()); len.saturating_sub(1)];
            if *len > 0 {
                subcalls.push(super::stored_versioned_session(
                    current_contract_package_hash.into(),
                ));
            }

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_payment_code(CONTRACT_CALL_RECURSIVE_SUBCALL, args)
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_contract_to_stored_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![super::stored_contract(current_contract_hash.into()); len.saturating_sub(1)];
            if *len > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()));
            }

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_payment_code(CONTRACT_CALL_RECURSIVE_SUBCALL, args)
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    // Stored session + recursive subcall

    #[ignore]
    #[test]
    fn stored_versioned_session_by_name_to_stored_versioned_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let subcalls =
                vec![super::stored_versioned_session(current_contract_package_hash.into()); *len];

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();

                let sender = *DEFAULT_ACCOUNT_ADDR;

                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };

                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_stored_versioned_payment_contract_by_name(
                        CONTRACT_PACKAGE_NAME,
                        None,
                        CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                        args,
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();

                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };
            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_versioned_session(current_contract_package_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_hash_to_stored_versioned_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let subcalls =
                vec![super::stored_versioned_session(current_contract_package_hash.into()); *len];

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_stored_versioned_payment_contract_by_hash(
                        current_contract_package_hash,
                        None,
                        CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                        args,
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };
            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_versioned_session(current_contract_package_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_name_to_stored_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_session(current_contract_hash.into()); *len];

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();

                let sender = *DEFAULT_ACCOUNT_ADDR;

                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };

                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_stored_versioned_payment_contract_by_name(
                        CONTRACT_PACKAGE_NAME,
                        None,
                        CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                        args,
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();

                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };
            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_versioned_session(current_contract_package_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_hash_to_stored_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_session(current_contract_hash.into()); *len];

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_stored_versioned_payment_contract_by_hash(
                        current_contract_package_hash,
                        None,
                        CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                        args,
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };
            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_versioned_session(current_contract_package_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_name_to_stored_versioned_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls =
                vec![super::stored_versioned_session(current_contract_package_hash.into()); *len];

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();

                let sender = *DEFAULT_ACCOUNT_ADDR;

                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };

                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_stored_payment_named_key(
                        CONTRACT_NAME,
                        CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                        args,
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();

                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };
            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_session(current_contract_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_hash_to_stored_versioned_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls =
                vec![super::stored_versioned_session(current_contract_package_hash.into()); *len];

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_stored_payment_hash(
                        current_contract_hash.into(),
                        CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                        args,
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };
            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_session(current_contract_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_name_to_stored_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_session(current_contract_hash.into()); *len];

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_stored_payment_named_key(
                        CONTRACT_NAME,
                        CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                        args,
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };
            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_session(current_contract_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_hash_to_stored_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_session(current_contract_hash.into()); *len];

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_stored_payment_hash(
                        current_contract_hash.into(),
                        CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                        args,
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };
            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_session(current_contract_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_name_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let subcalls =
                vec![super::stored_versioned_contract(current_contract_package_hash.into()); *len];

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_stored_versioned_payment_named_key(
                        CONTRACT_PACKAGE_NAME,
                        CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                        args,
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };
            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_versioned_session(current_contract_package_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_hash_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let subcalls =
                vec![super::stored_versioned_contract(current_contract_package_hash.into()); *len];

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_stored_versioned_payment_hash(
                        current_contract_package_hash.into(),
                        CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                        args,
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };
            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_versioned_session(current_contract_package_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_name_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_contract(current_contract_hash.into()); *len];

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_stored_versioned_payment_named_key(
                        CONTRACT_PACKAGE_NAME,
                        CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                        args,
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };
            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_session(current_contract_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_hash_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_contract(current_contract_hash.into()); *len];

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_stored_versioned_payment_hash(
                        current_contract_package_hash.into(),
                        CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                        args,
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };
            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_versioned_session(current_contract_package_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_name_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls =
                vec![super::stored_versioned_contract(current_contract_package_hash.into()); *len];

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_stored_payment_named_key(
                        CONTRACT_NAME,
                        CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                        args,
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };
            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_session(current_contract_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_hash_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls =
                vec![super::stored_versioned_contract(current_contract_package_hash.into()); *len];

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_stored_payment_hash(
                        current_contract_hash.into(),
                        CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                        args,
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };
            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_session(current_contract_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_name_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_contract(current_contract_hash.into()); *len];

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_stored_payment_named_key(
                        CONTRACT_NAME,
                        CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                        args,
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };
            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_session(current_contract_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_hash_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_contract(current_contract_hash.into()); *len];

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_stored_payment_hash(
                        current_contract_hash.into(),
                        CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                        args,
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };
            builder.exec(execute_request).commit().expect_success();

            super::assert_each_context_has_correct_call_stack_info(
                &mut builder,
                super::stored_session(current_contract_hash.into()),
                subcalls,
                current_contract_package_hash,
            );
        }
    }

    // Stored session + recursive subcall failure cases

    #[ignore]
    #[test]
    fn stored_versioned_session_by_name_to_stored_versioned_contract_to_stored_versioned_session_should_fail(
    ) {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    len.saturating_sub(1)
                ];
            if *len > 0 {
                subcalls.push(super::stored_versioned_session(
                    current_contract_package_hash.into(),
                ))
            }

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_stored_versioned_payment_named_key(
                        CONTRACT_PACKAGE_NAME,
                        CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                        args,
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_hash_to_stored_versioned_contract_to_stored_session_should_fail()
    {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    len.saturating_sub(1)
                ];
            if *len > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()))
            }

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_stored_versioned_payment_hash(
                        current_contract_package_hash.into(),
                        CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                        args,
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_name_to_stored_contract_to_stored_versioned_session_should_fail()
    {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![super::stored_contract(current_contract_hash.into()); len.saturating_sub(1)];
            if *len > 0 {
                subcalls.push(super::stored_versioned_session(
                    current_contract_package_hash.into(),
                ))
            }

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_stored_versioned_payment_named_key(
                        CONTRACT_PACKAGE_NAME,
                        CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                        args,
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_hash_to_stored_contract_to_stored_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![super::stored_contract(current_contract_hash.into()); len.saturating_sub(1)];
            if *len > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()))
            }

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_stored_versioned_payment_hash(
                        current_contract_package_hash.into(),
                        CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                        args,
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_name_to_stored_versioned_contract_to_stored_versioned_session_should_fail()
    {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    len.saturating_sub(1)
                ];
            if *len > 0 {
                subcalls.push(super::stored_versioned_session(
                    current_contract_package_hash.into(),
                ))
            }

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_stored_payment_named_key(
                        CONTRACT_NAME,
                        CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                        args,
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_hash_to_stored_versioned_contract_to_stored_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    len.saturating_sub(1)
                ];
            if *len > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()))
            }

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_stored_payment_hash(
                        current_contract_hash.into(),
                        CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                        args,
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_name_to_stored_contract_to_stored_versioned_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![super::stored_contract(current_contract_hash.into()); len.saturating_sub(1)];
            if *len > 0 {
                subcalls.push(super::stored_versioned_session(
                    current_contract_package_hash.into(),
                ))
            }

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_stored_payment_named_key(
                        CONTRACT_NAME,
                        CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                        args,
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_name_to_stored_contract_to_stored_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![super::stored_contract(current_contract_hash.into()); len.saturating_sub(1)];
            if *len > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()))
            }

            let execute_request = {
                let mut rng = rand::thread_rng();
                let deploy_hash = rng.gen();
                let sender = *DEFAULT_ACCOUNT_ADDR;
                let args = runtime_args! {
                    ARG_CALLS => subcalls.clone(),
                    ARG_CURRENT_DEPTH => 0u8,
                    mint::ARG_AMOUNT => approved_amount(*len),
                };
                let deploy = DeployItemBuilder::new()
                    .with_address(sender)
                    .with_stored_payment_named_key(
                        CONTRACT_NAME,
                        CONTRACT_FORWARDER_ENTRYPOINT_SESSION,
                        args,
                    )
                    .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                    .with_authorization_keys(&[sender])
                    .with_deploy_hash(deploy_hash)
                    .build();
                ExecuteRequestBuilder::new().push_deploy(deploy).build()
            };

            builder.exec(execute_request).commit();

            super::assert_invalid_context(&mut builder, *len);
        }
    }
}
