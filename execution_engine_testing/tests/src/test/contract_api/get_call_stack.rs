use num_traits::One;

use casper_engine_test_support::{
    ExecuteRequest, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    LOCAL_GENESIS_REQUEST,
};
use casper_execution_engine::{engine_state::Error as CoreError, execution::ExecError};
use casper_types::{
    account::{Account, AccountHash},
    contracts::{ContractHash, ContractPackageHash},
    runtime_args,
    system::{Caller, CallerInfo},
    CLValue, EntityAddr, EntryPointType, HashAddr, Key, PackageHash, StoredValue, U512,
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
        entry_point_type: EntryPointType::Caller,
    }
}

fn stored_versioned_session(contract_package_hash: ContractPackageHash) -> Call {
    Call {
        contract_address: ContractAddress::ContractPackageHash(contract_package_hash),
        target_method: CONTRACT_FORWARDER_ENTRYPOINT_SESSION.to_string(),
        entry_point_type: EntryPointType::Caller,
    }
}

fn stored_contract(contract_hash: ContractHash) -> Call {
    Call {
        contract_address: ContractAddress::ContractHash(contract_hash),
        target_method: CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT.to_string(),
        entry_point_type: EntryPointType::Called,
    }
}

fn stored_versioned_contract(contract_package_hash: ContractPackageHash) -> Call {
    Call {
        contract_address: ContractAddress::ContractPackageHash(contract_package_hash),
        target_method: CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT.to_string(),
        entry_point_type: EntryPointType::Called,
    }
}

fn store_contract(builder: &mut LmdbWasmTestBuilder, session_filename: &str) {
    let store_contract_request =
        ExecuteRequestBuilder::standard(*DEFAULT_ACCOUNT_ADDR, session_filename, runtime_args! {})
            .build();
    builder
        .exec(store_contract_request)
        .commit()
        .expect_success();
}

fn execute_and_assert_result(
    call_depth: usize,
    builder: &mut LmdbWasmTestBuilder,
    execute_request: ExecuteRequest,
    is_invalid_context: bool,
) {
    if call_depth == 0 {
        builder.exec(execute_request).commit().expect_success();
    } else if is_invalid_context {
        builder.exec(execute_request).commit().expect_failure();
        let error = builder.get_error().expect("must have an error");
        assert!(matches!(
            error,
            // Call chains have stored contract trying to call stored session which we don't
            // support and is an actual error.
            CoreError::Exec(ExecError::InvalidContext)
        ));
    }
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
            .and_then(Key::into_hash_addr)
            .unwrap()
    }
}

trait BuilderExt {
    fn get_call_stack_from_session_context(&mut self, stored_call_stack_key: &str) -> Vec<Caller>;

    fn get_call_stack_from_contract_context(
        &mut self,
        stored_call_stack_key: &str,
        contract_package_hash: HashAddr,
    ) -> Vec<Caller>;
}

impl BuilderExt for LmdbWasmTestBuilder {
    fn get_call_stack_from_session_context(&mut self, stored_call_stack_key: &str) -> Vec<Caller> {
        let cl_value = self
            .query(
                None,
                (*DEFAULT_ACCOUNT_ADDR).into(),
                &[stored_call_stack_key.to_string()],
            )
            .unwrap();

        let caller_info = cl_value
            .into_cl_value()
            .map(CLValue::into_t::<Vec<CallerInfo>>)
            .unwrap()
            .unwrap();

        let mut callers = vec![];

        for info in caller_info {
            let kind = info.kind();
            match kind {
                0 => {
                    let account_hash = info
                        .get_field_by_index(0)
                        .map(|val| {
                            val.to_t::<Option<AccountHash>>()
                                .expect("must convert out of cl_value")
                        })
                        .expect("must have index 0 in fields")
                        .expect("account hash must be some");
                    callers.push(Caller::Initiator { account_hash });
                }
                3 => {
                    let package_hash = info
                        .get_field_by_index(1)
                        .map(|val| {
                            val.to_t::<Option<PackageHash>>()
                                .expect("must convert out of cl_value")
                        })
                        .expect("must have index 1 in fields")
                        .expect("package hash must be some");
                    let entity_addr = info
                        .get_field_by_index(3)
                        .map(|val| {
                            val.to_t::<Option<EntityAddr>>()
                                .expect("must convert out of cl_value")
                        })
                        .expect("must have index 3 in fields")
                        .expect("entity addr must be some");
                    callers.push(Caller::Entity {
                        package_hash,
                        entity_addr,
                    });
                }
                4 => {
                    let contract_package_hash = info
                        .get_field_by_index(2)
                        .map(|val| {
                            val.to_t::<Option<ContractPackageHash>>()
                                .expect("must convert out of cl_value")
                        })
                        .expect("must have index 2 in fields")
                        .expect("contract package hash must be some");
                    let contract_hash = info
                        .get_field_by_index(4)
                        .map(|val| {
                            val.to_t::<Option<ContractHash>>()
                                .expect("must convert out of cl_value")
                        })
                        .expect("must have index 4 in fields")
                        .expect("contract hash must be some");
                    callers.push(Caller::SmartContract {
                        contract_package_hash,
                        contract_hash,
                    });
                }
                _ => panic!("unhandled kind"),
            }
        }

        callers
    }

    fn get_call_stack_from_contract_context(
        &mut self,
        stored_call_stack_key: &str,
        contract_package_hash: HashAddr,
    ) -> Vec<Caller> {
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

        let stack_elements = cl_value
            .into_cl_value()
            .map(CLValue::into_t::<Vec<CallerInfo>>)
            .unwrap()
            .unwrap();

        let mut callers = vec![];

        for info in stack_elements {
            let kind = info.kind();
            match kind {
                0 => {
                    let account_hash = info
                        .get_field_by_index(0)
                        .map(|val| {
                            val.to_t::<Option<AccountHash>>()
                                .expect("must convert out of cl_value")
                        })
                        .expect("must have index 0 in fields")
                        .expect("account hash must be some");
                    callers.push(Caller::Initiator { account_hash });
                }
                3 => {
                    let package_hash = info
                        .get_field_by_index(1)
                        .map(|val| {
                            val.to_t::<Option<PackageHash>>()
                                .expect("must convert out of cl_value")
                        })
                        .expect("must have index 1 in fields")
                        .expect("package hash must be some");
                    let entity_addr = info
                        .get_field_by_index(3)
                        .map(|val| {
                            val.to_t::<Option<EntityAddr>>()
                                .expect("must convert out of cl_value")
                        })
                        .expect("must have index 3 in fields")
                        .expect("entity addr must be some");
                    callers.push(Caller::Entity {
                        package_hash,
                        entity_addr,
                    });
                }
                4 => {
                    let contract_package_hash = info
                        .get_field_by_index(2)
                        .map(|val| {
                            val.to_t::<Option<ContractPackageHash>>()
                                .expect("must convert out of cl_value")
                        })
                        .expect("must have index 2 in fields")
                        .expect("contract package hash must be some");
                    let contract_hash = info
                        .get_field_by_index(4)
                        .map(|val| {
                            val.to_t::<Option<ContractHash>>()
                                .expect("must convert out of cl_value")
                        })
                        .expect("must have index 4 in fields")
                        .expect("contract hash must be some");
                    callers.push(Caller::SmartContract {
                        contract_package_hash,
                        contract_hash,
                    });
                }
                _ => panic!("unhandled kind"),
            }
        }

        callers
    }
}

fn setup() -> LmdbWasmTestBuilder {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());
    store_contract(&mut builder, CONTRACT_RECURSIVE_SUBCALL);
    builder
}

fn assert_each_context_has_correct_call_stack_info(
    builder: &mut LmdbWasmTestBuilder,
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
            EntryPointType::Called | EntryPointType::Factory => builder
                .get_call_stack_from_contract_context(
                    &stored_call_stack_key,
                    current_contract_package_hash,
                ),
            EntryPointType::Caller => {
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
            [Caller::Initiator {
                account_hash: *DEFAULT_ACCOUNT_ADDR,
            }],
        );
        assert_call_stack_matches_calls(rest.to_vec(), &calls);
    }
}

fn assert_invalid_context(builder: &mut LmdbWasmTestBuilder, depth: usize) {
    if depth == 0 {
        builder.expect_success();
    } else {
        let error = builder.get_error().unwrap();
        assert!(matches!(
            error,
            casper_execution_engine::engine_state::Error::Exec(ExecError::InvalidContext)
        ));
    }
}

fn assert_each_context_has_correct_call_stack_info_module_bytes(
    builder: &mut LmdbWasmTestBuilder,
    subcalls: Vec<Call>,
    current_contract_package_hash: HashAddr,
) {
    let stored_call_stack_key = format!("call_stack-{}", 0);
    let call_stack = builder.get_call_stack_from_session_context(&stored_call_stack_key);
    let (head, _) = call_stack.split_at(usize::one());
    assert_eq!(
        head,
        [Caller::Initiator {
            account_hash: *DEFAULT_ACCOUNT_ADDR,
        }],
    );

    for (i, call) in (1..=subcalls.len()).zip(subcalls.iter()) {
        let stored_call_stack_key = format!("call_stack-{}", i);
        // we need to know where to look for the call stack information
        let call_stack = match call.entry_point_type {
            EntryPointType::Called | EntryPointType::Factory => builder
                .get_call_stack_from_contract_context(
                    &stored_call_stack_key,
                    current_contract_package_hash,
                ),
            EntryPointType::Caller => {
                builder.get_call_stack_from_session_context(&stored_call_stack_key)
            }
        };
        let (head, rest) = call_stack.split_at(usize::one());
        assert_eq!(
            head,
            [Caller::Initiator {
                account_hash: *DEFAULT_ACCOUNT_ADDR,
            }],
        );
        assert_call_stack_matches_calls(rest.to_vec(), &subcalls);
    }
}

fn assert_call_stack_matches_calls(call_stack: Vec<Caller>, calls: &[Call]) {
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
                Caller::SmartContract {
                    contract_package_hash,
                    ..
                },
            ) if *entry_point_type == EntryPointType::Called
                && contract_package_hash.value() == current_contract_package_hash.value() => {}

            // Unversioned Call with EntryPointType::Called
            (
                Some(Call {
                    entry_point_type,
                    contract_address: ContractAddress::ContractHash(current_contract_hash),
                    ..
                }),
                Caller::SmartContract { contract_hash, .. },
            ) if *entry_point_type == EntryPointType::Called
                && contract_hash.value() == current_contract_hash.value() => {}

            // Versioned Call with EntryPointType::Session
            (
                Some(Call {
                    entry_point_type,
                    contract_address:
                        ContractAddress::ContractPackageHash(current_contract_package_hash),
                    ..
                }),
                Caller::SmartContract {
                    contract_package_hash,
                    ..
                },
            ) if *entry_point_type == EntryPointType::Caller
                && *contract_package_hash == *current_contract_package_hash => {}

            // Unversioned Call with EntryPointType::Session
            (
                Some(Call {
                    entry_point_type,
                    contract_address: ContractAddress::ContractHash(current_contract_hash),
                    ..
                }),
                Caller::SmartContract { contract_hash, .. },
            ) if *entry_point_type == EntryPointType::Caller
                && contract_hash.value() == current_contract_hash.value() => {}

            _ => panic!(
                "call stack element {:#?} didn't match expected call {:#?} at index {}, {:#?}",
                expected_call_stack_element, maybe_call, index, call_stack,
            ),
        }
    }
}

mod session {

    use casper_engine_test_support::{ExecuteRequestBuilder, DEFAULT_ACCOUNT_ADDR};
    use casper_types::{execution::TransformKindV2, runtime_args, system::mint, Key};

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
            println!("{:?}", default_account);
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

            let effects = builder.get_effects().last().unwrap().clone();

            let key = if builder.chainspec().core_config.enable_addressable_entity {
                Key::Package(current_contract_package_hash)
            } else {
                Key::Hash(current_contract_package_hash)
            };

            assert!(
                effects
                    .transforms()
                    .iter()
                    .any(|transform| transform.key() == &key
                        && transform.kind() == &TransformKindV2::Identity),
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

            let effects = builder.get_effects().last().unwrap().clone();

            assert!(
                effects.transforms().iter().any(|transform| transform.key()
                    == &Key::Hash(current_contract_hash)
                    && transform.kind() == &TransformKindV2::Identity),
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
    use std::iter;

    use rand::Rng;

    use casper_engine_test_support::{
        DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    };
    use casper_types::{runtime_args, system::mint, HashAddr, RuntimeArgs};
    use get_call_stack_recursive_subcall::Call;

    use crate::wasm_utils;

    use super::{
        approved_amount, AccountExt, ARG_CALLS, ARG_CURRENT_DEPTH, CONTRACT_CALL_RECURSIVE_SUBCALL,
        CONTRACT_FORWARDER_ENTRYPOINT_SESSION, CONTRACT_NAME, CONTRACT_PACKAGE_NAME,
    };

    // DEPTHS should not contain 1, as it will eliminate the initial element from the subcalls
    // vector.  Going further than 6 will hit the gas limit.
    const DEPTHS: &[usize] = &[0, 6, 10];

    fn execute(
        builder: &mut LmdbWasmTestBuilder,
        call_depth: usize,
        subcalls: Vec<Call>,
        is_invalid_context: bool,
    ) {
        let execute_request = {
            let mut rng = rand::thread_rng();
            let deploy_hash = rng.gen();
            let sender = *DEFAULT_ACCOUNT_ADDR;
            let args = runtime_args! {
                ARG_CALLS => subcalls,
                ARG_CURRENT_DEPTH => 0u8,
                mint::ARG_AMOUNT => approved_amount(call_depth),
            };
            let deploy = DeployItemBuilder::new()
                .with_address(sender)
                .with_payment_code(CONTRACT_CALL_RECURSIVE_SUBCALL, args)
                .with_session_bytes(wasm_utils::do_nothing_bytes(), RuntimeArgs::default())
                .with_authorization_keys(&[sender])
                .with_deploy_hash(deploy_hash)
                .build();
            ExecuteRequestBuilder::from_deploy_item(&deploy).build()
        };

        super::execute_and_assert_result(call_depth, builder, execute_request, is_invalid_context);
    }

    fn execute_stored_payment_by_package_name(
        builder: &mut LmdbWasmTestBuilder,
        call_depth: usize,
        subcalls: Vec<Call>,
    ) {
        let execute_request = {
            let mut rng = rand::thread_rng();
            let deploy_hash = rng.gen();

            let sender = *DEFAULT_ACCOUNT_ADDR;

            let args = runtime_args! {
                ARG_CALLS => subcalls,
                ARG_CURRENT_DEPTH => 0u8,
                mint::ARG_AMOUNT => approved_amount(call_depth),
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

            ExecuteRequestBuilder::from_deploy_item(&deploy).build()
        };

        super::execute_and_assert_result(call_depth, builder, execute_request, false);
    }

    fn execute_stored_payment_by_package_hash(
        builder: &mut LmdbWasmTestBuilder,
        call_depth: usize,
        subcalls: Vec<Call>,
        current_contract_package_hash: HashAddr,
    ) {
        let execute_request = {
            let mut rng = rand::thread_rng();
            let deploy_hash = rng.gen();
            let sender = *DEFAULT_ACCOUNT_ADDR;
            let args = runtime_args! {
                ARG_CALLS => subcalls,
                ARG_CURRENT_DEPTH => 0u8,
                mint::ARG_AMOUNT => approved_amount(call_depth),
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
            ExecuteRequestBuilder::from_deploy_item(&deploy).build()
        };

        super::execute_and_assert_result(call_depth, builder, execute_request, false);
    }

    fn execute_stored_payment_by_contract_name(
        builder: &mut LmdbWasmTestBuilder,
        call_depth: usize,
        subcalls: Vec<Call>,
    ) {
        let execute_request = {
            let mut rng = rand::thread_rng();
            let deploy_hash = rng.gen();

            let sender = *DEFAULT_ACCOUNT_ADDR;

            let args = runtime_args! {
                ARG_CALLS => subcalls,
                ARG_CURRENT_DEPTH => 0u8,
                mint::ARG_AMOUNT => approved_amount(call_depth),
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

            ExecuteRequestBuilder::from_deploy_item(&deploy).build()
        };

        super::execute_and_assert_result(call_depth, builder, execute_request, false);
    }

    fn execute_stored_payment_by_contract_hash(
        builder: &mut LmdbWasmTestBuilder,
        call_depth: usize,
        subcalls: Vec<Call>,
        current_contract_hash: HashAddr,
    ) {
        let execute_request = {
            let mut rng = rand::thread_rng();
            let deploy_hash = rng.gen();
            let sender = *DEFAULT_ACCOUNT_ADDR;
            let args = runtime_args! {
                ARG_CALLS => subcalls,
                ARG_CURRENT_DEPTH => 0u8,
                mint::ARG_AMOUNT => approved_amount(call_depth),
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
            ExecuteRequestBuilder::from_deploy_item(&deploy).build()
        };

        super::execute_and_assert_result(call_depth, builder, execute_request, false);
    }

    // Session + recursive subcall

    #[ignore]
    #[test]
    fn payment_bytes_to_stored_versioned_session_to_stored_versioned_contract() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_session(current_contract_package_hash.into());
                    call_depth.saturating_sub(1)
                ];
            if *call_depth > 0 {
                subcalls.push(super::stored_versioned_contract(
                    current_contract_package_hash.into(),
                ));
            }

            execute(&mut builder, *call_depth, subcalls, false);
        }
    }

    #[ignore]
    #[test]
    fn payment_bytes_to_stored_versioned_session_to_stored_contract() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_session(current_contract_package_hash.into());
                    call_depth.saturating_sub(1)
                ];
            if *call_depth > 0 {
                subcalls.push(super::stored_contract(current_contract_hash.into()));
            }

            execute(&mut builder, *call_depth, subcalls, false);
        }
    }

    #[ignore]
    #[test]
    fn payment_bytes_to_stored_session_to_stored_versioned_contract() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls = vec![
                super::stored_session(current_contract_hash.into());
                call_depth.saturating_sub(1)
            ];
            if *call_depth > 0 {
                subcalls.push(super::stored_versioned_contract(
                    current_contract_package_hash.into(),
                ));
            }

            execute(&mut builder, *call_depth, subcalls, false)
        }
    }

    // Payment logic is tethered to a low gas amount. It is not forbidden to attempt to do calls
    // however they are expensive and if you exceed the gas limit it should fail with a
    // GasLimit error.
    #[ignore]
    #[test]
    fn payment_bytes_to_stored_contract_to_stored_session() {
        let call_depth = 5usize;
        let mut builder = super::setup();
        let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
        let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

        let subcalls = vec![
            super::stored_contract(current_contract_hash.into()),
            super::stored_session(current_contract_hash.into()),
        ];
        execute(&mut builder, call_depth, subcalls, true)
    }

    #[ignore]
    #[test]
    fn payment_bytes_to_stored_session_to_stored_contract_() {
        let call_depth = 5usize;
        let mut builder = super::setup();
        let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
        let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

        let subcalls = iter::repeat_with(|| {
            [
                super::stored_session(current_contract_hash.into()),
                super::stored_contract(current_contract_hash.into()),
            ]
        })
        .take(call_depth)
        .flatten();
        execute(&mut builder, call_depth, subcalls.collect(), false)
    }

    // Session + recursive subcall failure cases

    #[ignore]
    #[test]
    fn payment_bytes_to_stored_versioned_contract_to_stored_versioned_session_should_fail() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    call_depth.saturating_sub(1)
                ];
            if *call_depth > 0 {
                subcalls.push(super::stored_versioned_session(
                    current_contract_package_hash.into(),
                ));
            }

            execute(&mut builder, *call_depth, subcalls, true)
        }
    }

    #[ignore]
    #[test]
    fn payment_bytes_to_stored_versioned_contract_to_stored_session_should_fail() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    call_depth.saturating_sub(1)
                ];
            if *call_depth > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()));
            }

            execute(&mut builder, *call_depth, subcalls, true)
        }
    }

    #[ignore]
    #[test]
    fn payment_bytes_to_stored_contract_to_stored_versioned_session_should_fail() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls = vec![
                super::stored_contract(current_contract_hash.into());
                call_depth.saturating_sub(1)
            ];
            if *call_depth > 0 {
                subcalls.push(super::stored_versioned_session(
                    current_contract_package_hash.into(),
                ));
            }

            execute(&mut builder, *call_depth, subcalls, true)
        }
    }

    #[ignore]
    #[test]
    fn payment_bytes_to_stored_contract_to_stored_session_should_fail() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls = vec![
                super::stored_contract(current_contract_hash.into());
                call_depth.saturating_sub(1)
            ];
            if *call_depth > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()));
            }

            execute(&mut builder, *call_depth, subcalls, true)
        }
    }

    // Stored session + recursive subcall

    #[ignore]
    #[test]
    fn stored_versioned_payment_by_name_to_stored_versioned_session() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let subcalls =
                vec![
                    super::stored_versioned_session(current_contract_package_hash.into());
                    *call_depth
                ];

            execute_stored_payment_by_package_name(&mut builder, *call_depth, subcalls);
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_payment_by_hash_to_stored_versioned_session() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let subcalls =
                vec![
                    super::stored_versioned_session(current_contract_package_hash.into());
                    *call_depth
                ];

            execute_stored_payment_by_package_hash(
                &mut builder,
                *call_depth,
                subcalls,
                current_contract_package_hash,
            )
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_payment_by_name_to_stored_session() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_session(current_contract_hash.into()); *call_depth];

            execute_stored_payment_by_package_name(&mut builder, *call_depth, subcalls)
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_payment_by_hash_to_stored_session() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_session(current_contract_hash.into()); *call_depth];

            execute_stored_payment_by_package_hash(
                &mut builder,
                *call_depth,
                subcalls,
                current_contract_package_hash,
            )
        }
    }

    #[ignore]
    #[test]
    fn stored_payment_by_name_to_stored_versioned_session() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let subcalls =
                vec![
                    super::stored_versioned_session(current_contract_package_hash.into());
                    *call_depth
                ];

            execute_stored_payment_by_contract_name(&mut builder, *call_depth, subcalls)
        }
    }

    #[ignore]
    #[test]
    fn stored_payment_by_hash_to_stored_versioned_session() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls =
                vec![
                    super::stored_versioned_session(current_contract_package_hash.into());
                    *call_depth
                ];

            execute_stored_payment_by_contract_hash(
                &mut builder,
                *call_depth,
                subcalls,
                current_contract_hash,
            )
        }
    }

    #[ignore]
    #[test]
    fn stored_payment_by_name_to_stored_session() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_session(current_contract_hash.into()); *call_depth];

            execute_stored_payment_by_contract_name(&mut builder, *call_depth, subcalls)
        }
    }

    #[ignore]
    #[test]
    fn stored_payment_by_hash_to_stored_session() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_session(current_contract_hash.into()); *call_depth];

            execute_stored_payment_by_contract_hash(
                &mut builder,
                *call_depth,
                subcalls,
                current_contract_hash,
            )
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_payment_by_name_to_stored_versioned_contract() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    *call_depth
                ];

            execute_stored_payment_by_contract_name(&mut builder, *call_depth, subcalls)
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_payment_by_hash_to_stored_versioned_contract() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    *call_depth
                ];

            execute_stored_payment_by_package_hash(
                &mut builder,
                *call_depth,
                subcalls,
                current_contract_package_hash,
            )
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_payment_by_name_to_stored_contract() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_contract(current_contract_hash.into()); *call_depth];

            execute_stored_payment_by_package_name(&mut builder, *call_depth, subcalls)
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_payment_by_hash_to_stored_contract() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_contract(current_contract_hash.into()); *call_depth];

            execute_stored_payment_by_package_hash(
                &mut builder,
                *call_depth,
                subcalls,
                current_contract_package_hash,
            )
        }
    }

    #[ignore]
    #[test]
    fn stored_payment_by_name_to_stored_versioned_contract() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    *call_depth
                ];

            execute_stored_payment_by_contract_name(&mut builder, *call_depth, subcalls)
        }
    }

    #[ignore]
    #[test]
    fn stored_payment_by_hash_to_stored_versioned_contract() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    *call_depth
                ];

            execute_stored_payment_by_contract_hash(
                &mut builder,
                *call_depth,
                subcalls,
                current_contract_hash,
            )
        }
    }

    #[ignore]
    #[test]
    fn stored_payment_by_name_to_stored_contract() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_contract(current_contract_hash.into()); *call_depth];

            execute_stored_payment_by_contract_name(&mut builder, *call_depth, subcalls)
        }
    }

    #[ignore]
    #[test]
    fn stored_payment_by_hash_to_stored_contract() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            println!("DA {:?}", default_account);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_contract(current_contract_hash.into()); *call_depth];

            execute_stored_payment_by_contract_hash(
                &mut builder,
                *call_depth,
                subcalls,
                current_contract_hash,
            )
        }
    }

    // Stored session + recursive subcall failure cases

    #[ignore]
    #[test]
    fn stored_versioned_payment_by_name_to_stored_versioned_contract_to_stored_versioned_session_should_fail(
    ) {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    call_depth.saturating_sub(1)
                ];
            if *call_depth > 0 {
                subcalls.push(super::stored_versioned_session(
                    current_contract_package_hash.into(),
                ))
            }

            execute_stored_payment_by_package_name(&mut builder, *call_depth, subcalls)
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_payment_by_hash_to_stored_versioned_contract_to_stored_session_should_fail()
    {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    call_depth.saturating_sub(1)
                ];
            if *call_depth > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()))
            }

            execute_stored_payment_by_package_hash(
                &mut builder,
                *call_depth,
                subcalls,
                current_contract_package_hash,
            )
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_payment_by_name_to_stored_contract_to_stored_versioned_session_should_fail()
    {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls = vec![
                super::stored_contract(current_contract_hash.into());
                call_depth.saturating_sub(1)
            ];
            if *call_depth > 0 {
                subcalls.push(super::stored_versioned_session(
                    current_contract_package_hash.into(),
                ))
            }

            execute_stored_payment_by_package_name(&mut builder, *call_depth, subcalls)
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_payment_by_hash_to_stored_contract_to_stored_session_should_fail() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls = vec![
                super::stored_contract(current_contract_hash.into());
                call_depth.saturating_sub(1)
            ];
            if *call_depth > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()))
            }

            execute_stored_payment_by_package_hash(
                &mut builder,
                *call_depth,
                subcalls,
                current_contract_package_hash,
            )
        }
    }

    #[ignore]
    #[test]
    fn stored_payment_by_name_to_stored_versioned_contract_to_stored_versioned_session_should_fail()
    {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    call_depth.saturating_sub(1)
                ];
            if *call_depth > 0 {
                subcalls.push(super::stored_versioned_session(
                    current_contract_package_hash.into(),
                ))
            }

            execute_stored_payment_by_contract_name(&mut builder, *call_depth, subcalls)
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_hash_to_stored_versioned_contract_to_stored_session_should_fail() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    call_depth.saturating_sub(1)
                ];
            if *call_depth > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()))
            }

            execute_stored_payment_by_contract_hash(
                &mut builder,
                *call_depth,
                subcalls,
                current_contract_hash,
            )
        }
    }

    #[ignore]
    #[test]
    fn stored_payment_by_name_to_stored_contract_to_stored_versioned_session_should_fail() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_package_hash = default_account.get_hash(CONTRACT_PACKAGE_NAME);
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls = vec![
                super::stored_contract(current_contract_hash.into());
                call_depth.saturating_sub(1)
            ];
            if *call_depth > 0 {
                subcalls.push(super::stored_versioned_session(
                    current_contract_package_hash.into(),
                ))
            }

            execute_stored_payment_by_contract_name(&mut builder, *call_depth, subcalls)
        }
    }

    #[ignore]
    #[test]
    fn stored_payment_by_name_to_stored_contract_to_stored_session_should_fail() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder.get_account(*DEFAULT_ACCOUNT_ADDR).unwrap();
            let current_contract_hash = default_account.get_hash(CONTRACT_NAME);

            let mut subcalls = vec![
                super::stored_contract(current_contract_hash.into());
                call_depth.saturating_sub(1)
            ];
            if *call_depth > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()))
            }

            execute_stored_payment_by_contract_name(&mut builder, *call_depth, subcalls)
        }
    }
}
