use num_traits::One;

use casper_engine_test_support::{LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR};
use casper_execution_engine::engine_state::{Error as CoreError, ExecError, ExecuteRequest};
use casper_types::{
    addressable_entity::NamedKeys, system::CallStackElement, AddressableEntity,
    AddressableEntityHash, CLValue, EntityAddr, EntryPointType, HashAddr, Key, PackageAddr,
    PackageHash, StoredValue, U512,
};

use crate::lmdb_fixture;

use get_call_stack_recursive_subcall::{
    Call, ContractAddress, ARG_CALLS, ARG_CURRENT_DEPTH, METHOD_FORWARDER_CONTRACT_NAME,
    METHOD_FORWARDER_SESSION_NAME,
};

const CONTRACT_CALL_RECURSIVE_SUBCALL: &str = "get_call_stack_call_recursive_subcall.wasm";

const CONTRACT_PACKAGE_NAME: &str = "forwarder";
const CONTRACT_NAME: &str = "our_contract_name";

const CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT: &str = METHOD_FORWARDER_CONTRACT_NAME;
const CONTRACT_FORWARDER_ENTRYPOINT_SESSION: &str = METHOD_FORWARDER_SESSION_NAME;

const IS_SESSION_ENTRY_POINT: bool = true;
const IS_NOT_SESSION_ENTRY_POINT: bool = false;

const CALL_STACK_FIXTURE: &str = "call_stack_fixture";

fn stored_session(contract_hash: AddressableEntityHash) -> Call {
    Call {
        contract_address: ContractAddress::ContractHash(contract_hash),
        target_method: CONTRACT_FORWARDER_ENTRYPOINT_SESSION.to_string(),
        entry_point_type: EntryPointType::Session,
    }
}

fn stored_versioned_session(contract_package_hash: PackageHash) -> Call {
    Call {
        contract_address: ContractAddress::ContractPackageHash(contract_package_hash),
        target_method: CONTRACT_FORWARDER_ENTRYPOINT_SESSION.to_string(),
        entry_point_type: EntryPointType::Session,
    }
}

fn stored_contract(contract_hash: AddressableEntityHash) -> Call {
    Call {
        contract_address: ContractAddress::ContractHash(contract_hash),
        target_method: CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT.to_string(),
        entry_point_type: EntryPointType::AddressableEntity,
    }
}

fn stored_versioned_contract(contract_package_hash: PackageHash) -> Call {
    Call {
        contract_address: ContractAddress::ContractPackageHash(contract_package_hash),
        target_method: CONTRACT_FORWARDER_ENTRYPOINT_CONTRACT.to_string(),
        entry_point_type: EntryPointType::AddressableEntity,
    }
}

fn execute_and_assert_result(
    call_depth: usize,
    builder: &mut LmdbWasmTestBuilder,
    execute_request: ExecuteRequest,
    is_entry_point_type_session: bool,
) {
    if call_depth == 0 && !is_entry_point_type_session {
        builder.exec(execute_request).commit().expect_success();
    } else {
        builder.exec(execute_request).commit().expect_failure();
        let error = builder.get_error().expect("must have an error");

        assert!(matches!(
            error,
            // Call chains have stored contract trying to call stored session which we don't
            // support and is an actual error. Due to variable opcode costs such
            // execution may end up in a success (and fail with InvalidContext) or GasLimit when
            // executing longer chains.
            CoreError::Exec(ExecError::InvalidContext) | CoreError::Exec(ExecError::GasLimit)
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

struct EntityWithKeys {
    #[allow(dead_code)]
    entity: AddressableEntity,
    named_keys: NamedKeys,
}

impl EntityWithKeys {
    fn new(entity: AddressableEntity, named_keys: NamedKeys) -> Self {
        Self { entity, named_keys }
    }
}

trait AccountExt {
    fn get_entity_hash(&self, key: &str) -> HashAddr;

    fn get_package_hash(&self, key: &str) -> PackageAddr;
}

impl AccountExt for EntityWithKeys {
    fn get_entity_hash(&self, key: &str) -> HashAddr {
        self.named_keys
            .get(key)
            .cloned()
            .and_then(Key::into_entity_hash_addr)
            .unwrap()
    }

    fn get_package_hash(&self, key: &str) -> PackageAddr {
        self.named_keys
            .get(key)
            .cloned()
            .and_then(Key::into_package_addr)
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

impl BuilderExt for LmdbWasmTestBuilder {
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
            .into_cl_value()
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
            .query(None, Key::Package(contract_package_hash), &[])
            .unwrap();

        let package = match value {
            StoredValue::Package(package) => package,
            _ => panic!("unreachable"),
        };

        let current_entity_hash = package.current_entity_hash().unwrap();
        let current_contract_entity_key =
            EntityAddr::new_contract_entity_addr(current_entity_hash.value());

        let cl_value = self
            .query(
                None,
                current_contract_entity_key.into(),
                &[stored_call_stack_key.to_string()],
            )
            .unwrap();

        cl_value
            .into_cl_value()
            .map(CLValue::into_t::<Vec<CallStackElement>>)
            .unwrap()
            .unwrap()
    }
}

fn setup() -> LmdbWasmTestBuilder {
    let (builder, _, _) = lmdb_fixture::builder_from_global_state_fixture(CALL_STACK_FIXTURE);
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
            EntryPointType::AddressableEntity | EntryPointType::Factory => builder
                .get_call_stack_from_contract_context(
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

fn assert_invalid_context(builder: &mut LmdbWasmTestBuilder, depth: usize) {
    if depth == 0 {
        builder.expect_success();
    } else {
        let error = builder.get_error().unwrap();
        assert!(matches!(
            error,
            casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext
            )
        ));
    }
}

fn assert_each_context_has_correct_call_stack_info_module_bytes(
    builder: &mut LmdbWasmTestBuilder,
    subcalls: Vec<Call>,
    current_contract_package_hash: PackageHash,
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
            EntryPointType::AddressableEntity | EntryPointType::Factory => builder
                .get_call_stack_from_contract_context(
                    &stored_call_stack_key,
                    current_contract_package_hash.value(),
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
                CallStackElement::AddressableEntity {
                    package_hash: contract_package_hash,
                    ..
                },
            ) if *entry_point_type == EntryPointType::AddressableEntity
                && *contract_package_hash == *current_contract_package_hash => {}

            // Unversioned Call with EntryPointType::Contract
            (
                Some(Call {
                    entry_point_type,
                    contract_address: ContractAddress::ContractHash(current_contract_hash),
                    ..
                }),
                CallStackElement::AddressableEntity {
                    entity_hash: contract_hash,
                    ..
                },
            ) if *entry_point_type == EntryPointType::AddressableEntity
                && *contract_hash == *current_contract_hash => {}

            _ => panic!(
                "call stack element {:#?} didn't match expected call {:#?} at index {}, {:#?}",
                expected_call_stack_element, maybe_call, index, call_stack,
            ),
        }
    }
}

mod session {
    use crate::test::contract_api::get_call_stack::EntityWithKeys;
    use casper_engine_test_support::{ExecuteRequestBuilder, DEFAULT_ACCOUNT_ADDR};
    use casper_types::{execution::TransformKind, runtime_args, system::mint, Key};
    use num_traits::Zero;

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();

            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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
                current_contract_package_hash.into(),
            );
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();

            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
                current_contract_package_hash.into(),
            );
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_versioned_contract_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();

            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
                current_contract_package_hash.into(),
            );
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_contract_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();

            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
                current_contract_package_hash.into(),
            );
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_versioned_session_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();

            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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

            // At current depth with len at 0, the invoked wasm will early exit and
            // does not invoke the stored session. Therefore for the first iteration we
            // expect successful execution of the module bytes.
            if len.is_zero() {
                builder.exec(execute_request).expect_success().commit();
            } else {
                builder.exec(execute_request).expect_failure();

                let expected_error = casper_execution_engine::engine_state::Error::Exec(
                    casper_execution_engine::execution::Error::InvalidContext,
                );

                builder.assert_error(expected_error)
            }
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_versioned_session_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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

            // At current depth with len at 0, the invoked wasm will early exit and
            // does not invoke the stored session. Therefore for the first iteration we
            // expect successful execution of the module bytes.
            if len.is_zero() {
                builder.exec(execute_request).expect_success().commit();
            } else {
                builder.exec(execute_request).expect_failure();

                let expected_error = casper_execution_engine::engine_state::Error::Exec(
                    casper_execution_engine::execution::Error::InvalidContext,
                );

                builder.assert_error(expected_error)
            }
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_session_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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

            // At current depth with len at 0, the invoked wasm will early exit and
            // does not invoke the stored session. Therefore for the first iteration we
            // expect successful execution of the module bytes.
            if len.is_zero() {
                builder.exec(execute_request).expect_success().commit();
            } else {
                builder.exec(execute_request).expect_failure();

                let expected_error = casper_execution_engine::engine_state::Error::Exec(
                    casper_execution_engine::execution::Error::InvalidContext,
                );

                builder.assert_error(expected_error)
            }
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_session_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();

            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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

            // At current depth with len at 0, the invoked wasm will early exit and
            // does not invoke the stored session. Therefore for the first iteration we
            // expect successful execution of the module bytes.
            if len.is_zero() {
                builder.exec(execute_request).expect_success().commit();
            } else {
                builder.exec(execute_request).expect_failure();

                let expected_error = casper_execution_engine::engine_state::Error::Exec(
                    casper_execution_engine::execution::Error::InvalidContext,
                );

                builder.assert_error(expected_error)
            }
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_versioned_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();

            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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

            // At current depth with len at 0, the invoked wasm will early exit and
            // does not invoke the stored session. Therefore for the first iteration we
            // expect successful execution of the module bytes.
            if len.is_zero() {
                builder.exec(execute_request).expect_success().commit();
            } else {
                builder.exec(execute_request).expect_failure();

                let expected_error = casper_execution_engine::engine_state::Error::Exec(
                    casper_execution_engine::execution::Error::InvalidContext,
                );

                builder.assert_error(expected_error)
            }
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_versioned_session_to_stored_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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

            // At current depth with len at 0, the invoked wasm will early exit and
            // does not invoke the stored session. Therefore for the first iteration we
            // expect successful execution of the module bytes.
            if len.is_zero() {
                builder.exec(execute_request).expect_success().commit();
            } else {
                builder.exec(execute_request).expect_failure();

                let expected_error = casper_execution_engine::engine_state::Error::Exec(
                    casper_execution_engine::execution::Error::InvalidContext,
                );

                builder.assert_error(expected_error)
            }
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_session_to_stored_versioned_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();

            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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

            // At current depth with len at 0, the invoked wasm will early exit and
            // does not invoke the stored session. Therefore for the first iteration we
            // expect successful execution of the module bytes.
            if len.is_zero() {
                builder.exec(execute_request).expect_success().commit();
            } else {
                builder.exec(execute_request).expect_failure();

                let expected_error = casper_execution_engine::engine_state::Error::Exec(
                    casper_execution_engine::execution::Error::InvalidContext,
                );

                builder.assert_error(expected_error)
            }
        }
    }

    #[ignore]
    #[test]
    fn session_bytes_to_stored_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();

            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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

            // At current depth with len at 0, the invoked wasm will early exit and
            // does not invoke the stored session. Therefore for the first iteration we
            // expect successful execution of the module bytes.
            if len.is_zero() {
                builder.exec(execute_request).expect_success().commit();
            } else {
                builder.exec(execute_request).expect_failure();

                let expected_error = casper_execution_engine::engine_state::Error::Exec(
                    casper_execution_engine::execution::Error::InvalidContext,
                );

                builder.assert_error(expected_error)
            }
        }
    }

    // Session + recursive subcall failure cases

    #[ignore]
    #[test]
    fn session_bytes_to_stored_versioned_contract_to_stored_versioned_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();

            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();

            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();

            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();

            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();

            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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

            assert!(
                effects.transforms().iter().any(|transform| transform.key()
                    == &Key::Package(current_contract_package_hash)
                    && transform.kind() == &TransformKind::Identity),
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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
                    == &Key::contract_entity_key(current_contract_hash.into())
                    && transform.kind() == &TransformKind::Identity),
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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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

            builder.exec(execute_request).expect_failure();

            let expected_error = casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext,
            );

            builder.assert_error(expected_error);
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_hash_to_stored_versioned_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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

            builder.exec(execute_request).expect_failure();

            let expected_error = casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext,
            );

            builder.assert_error(expected_error);
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_name_to_stored_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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

            builder.exec(execute_request).expect_failure();

            let expected_error = casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext,
            );

            builder.assert_error(expected_error);
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_hash_to_stored_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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

            builder.exec(execute_request).expect_failure();

            let expected_error = casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext,
            );

            builder.assert_error(expected_error);
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_name_to_stored_versioned_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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

            builder.exec(execute_request).expect_failure();

            let expected_error = casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext,
            );

            builder.assert_error(expected_error);
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_hash_to_stored_versioned_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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

            builder.exec(execute_request).expect_failure();

            let expected_error = casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext,
            );

            builder.assert_error(expected_error);
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_name_to_stored_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();

            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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

            builder.exec(execute_request).expect_failure();

            let expected_error = casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext,
            );

            builder.assert_error(expected_error);
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_hash_to_stored_session() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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

            builder.exec(execute_request).expect_failure();

            let expected_error = casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext,
            );

            builder.assert_error(expected_error);
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_name_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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

            builder.exec(execute_request).expect_failure();

            let expected_error = casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext,
            );

            builder.assert_error(expected_error);
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_hash_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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

            builder.exec(execute_request).expect_failure();

            let expected_error = casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext,
            );

            builder.assert_error(expected_error);
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_name_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();

            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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

            builder.exec(execute_request).expect_failure();

            let expected_error = casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext,
            );

            builder.assert_error(expected_error);
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_hash_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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

            builder.exec(execute_request).expect_failure();

            let expected_error = casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext,
            );

            builder.assert_error(expected_error);
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_name_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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

            builder.exec(execute_request).expect_failure();

            let expected_error = casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext,
            );

            builder.assert_error(expected_error);
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_hash_to_stored_versioned_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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

            builder.exec(execute_request).expect_failure();

            let expected_error = casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext,
            );

            builder.assert_error(expected_error);
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_name_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();

            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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

            builder.exec(execute_request).expect_failure();

            let expected_error = casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext,
            );

            builder.assert_error(expected_error);
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_hash_to_stored_contract() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();

            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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

            builder.exec(execute_request).expect_failure();

            let expected_error = casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext,
            );

            builder.assert_error(expected_error);
        }
    }

    // Stored session + recursive subcall failure cases

    #[ignore]
    #[test]
    fn stored_versioned_session_by_name_to_stored_versioned_contract_to_stored_versioned_session_should_fail(
    ) {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();

            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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

            builder.exec(execute_request).expect_failure();

            let expected_error = casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext,
            );

            builder.assert_error(expected_error);
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_hash_to_stored_versioned_contract_to_stored_session_should_fail()
    {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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

            builder.exec(execute_request).expect_failure();

            let expected_error = casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext,
            );

            builder.assert_error(expected_error);
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_name_to_stored_contract_to_stored_versioned_session_should_fail()
    {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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

            builder.exec(execute_request).expect_failure();

            let expected_error = casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext,
            );

            builder.assert_error(expected_error);
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_session_by_hash_to_stored_contract_to_stored_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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

            builder.exec(execute_request).expect_failure();

            let expected_error = casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext,
            );

            builder.assert_error(expected_error);
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_name_to_stored_versioned_contract_to_stored_versioned_session_should_fail()
    {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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

            builder.exec(execute_request).expect_failure();

            let expected_error = casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext,
            );

            builder.assert_error(expected_error);
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_hash_to_stored_versioned_contract_to_stored_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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

            builder.exec(execute_request).expect_failure();

            let expected_error = casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext,
            );

            builder.assert_error(expected_error);
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_name_to_stored_contract_to_stored_versioned_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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

            builder.exec(execute_request).expect_failure();

            let expected_error = casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext,
            );

            builder.assert_error(expected_error);
        }
    }

    #[ignore]
    #[test]
    fn stored_session_by_hash_to_stored_contract_to_stored_session_should_fail() {
        for len in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);
            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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

            builder.exec(execute_request).expect_failure();

            let expected_error = casper_execution_engine::engine_state::Error::Exec(
                casper_execution_engine::execution::Error::InvalidContext,
            );

            builder.assert_error(expected_error);
        }
    }
}

mod payment {
    use std::iter;

    use rand::Rng;

    use crate::test::contract_api::get_call_stack::{
        EntityWithKeys, IS_NOT_SESSION_ENTRY_POINT, IS_SESSION_ENTRY_POINT,
    };
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

    fn execute(builder: &mut LmdbWasmTestBuilder, call_depth: usize, subcalls: Vec<Call>) {
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
            ExecuteRequestBuilder::new().push_deploy(deploy).build()
        };

        super::execute_and_assert_result(
            call_depth,
            builder,
            execute_request,
            IS_NOT_SESSION_ENTRY_POINT,
        );
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

            ExecuteRequestBuilder::new().push_deploy(deploy).build()
        };

        super::execute_and_assert_result(
            call_depth,
            builder,
            execute_request,
            IS_SESSION_ENTRY_POINT,
        );
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
            ExecuteRequestBuilder::new().push_deploy(deploy).build()
        };

        super::execute_and_assert_result(
            call_depth,
            builder,
            execute_request,
            IS_SESSION_ENTRY_POINT,
        );
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

            ExecuteRequestBuilder::new().push_deploy(deploy).build()
        };

        super::execute_and_assert_result(
            call_depth,
            builder,
            execute_request,
            IS_SESSION_ENTRY_POINT,
        );
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
            ExecuteRequestBuilder::new().push_deploy(deploy).build()
        };

        let is_entry_point_type_session = true;

        super::execute_and_assert_result(
            call_depth,
            builder,
            execute_request,
            is_entry_point_type_session,
        );
    }

    // Session + recursive subcall

    #[ignore]
    #[test]
    fn payment_bytes_to_stored_versioned_session_to_stored_versioned_contract() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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

            execute(&mut builder, *call_depth, subcalls);
        }
    }

    #[ignore]
    #[test]
    fn payment_bytes_to_stored_versioned_session_to_stored_contract() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_session(current_contract_package_hash.into());
                    call_depth.saturating_sub(1)
                ];
            if *call_depth > 0 {
                subcalls.push(super::stored_contract(current_contract_hash.into()));
            }

            execute(&mut builder, *call_depth, subcalls);
        }
    }

    #[ignore]
    #[test]
    fn payment_bytes_to_stored_session_to_stored_versioned_contract() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

            let mut subcalls = vec![
                super::stored_session(current_contract_hash.into());
                call_depth.saturating_sub(1)
            ];
            if *call_depth > 0 {
                subcalls.push(super::stored_versioned_contract(
                    current_contract_package_hash.into(),
                ));
            }

            execute(&mut builder, *call_depth, subcalls)
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
        let default_account = builder
            .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
            .unwrap();
        let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

        let default_entity = EntityWithKeys::new(default_account, named_keys);

        let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

        let subcalls = vec![
            super::stored_contract(current_contract_hash.into()),
            super::stored_session(current_contract_hash.into()),
        ];
        execute(&mut builder, call_depth, subcalls)
    }

    #[ignore]
    #[test]
    fn payment_bytes_to_stored_session_to_stored_contract_() {
        let call_depth = 5usize;
        let mut builder = super::setup();
        let default_account = builder
            .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
            .unwrap();
        let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

        let default_entity = EntityWithKeys::new(default_account, named_keys);

        let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

        let subcalls = iter::repeat_with(|| {
            [
                super::stored_session(current_contract_hash.into()),
                super::stored_contract(current_contract_hash.into()),
            ]
        })
        .take(call_depth)
        .flatten();
        execute(&mut builder, call_depth, subcalls.collect())
    }

    // Session + recursive subcall failure cases

    #[ignore]
    #[test]
    fn payment_bytes_to_stored_versioned_contract_to_stored_versioned_session_should_fail() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);
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

            execute(&mut builder, *call_depth, subcalls)
        }
    }

    #[ignore]
    #[test]
    fn payment_bytes_to_stored_versioned_contract_to_stored_session_should_fail() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

            let mut subcalls =
                vec![
                    super::stored_versioned_contract(current_contract_package_hash.into());
                    call_depth.saturating_sub(1)
                ];
            if *call_depth > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()));
            }

            execute(&mut builder, *call_depth, subcalls)
        }
    }

    #[ignore]
    #[test]
    fn payment_bytes_to_stored_contract_to_stored_versioned_session_should_fail() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

            let mut subcalls = vec![
                super::stored_contract(current_contract_hash.into());
                call_depth.saturating_sub(1)
            ];
            if *call_depth > 0 {
                subcalls.push(super::stored_versioned_session(
                    current_contract_package_hash.into(),
                ));
            }

            execute(&mut builder, *call_depth, subcalls)
        }
    }

    #[ignore]
    #[test]
    fn payment_bytes_to_stored_contract_to_stored_session_should_fail() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

            let mut subcalls = vec![
                super::stored_contract(current_contract_hash.into());
                call_depth.saturating_sub(1)
            ];
            if *call_depth > 0 {
                subcalls.push(super::stored_session(current_contract_hash.into()));
            }

            execute(&mut builder, *call_depth, subcalls)
        }
    }

    // Stored session + recursive subcall

    #[ignore]
    #[test]
    fn stored_versioned_payment_by_name_to_stored_versioned_session() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);
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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_session(current_contract_hash.into()); *call_depth];

            execute_stored_payment_by_package_name(&mut builder, *call_depth, subcalls)
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_payment_by_hash_to_stored_session() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_session(current_contract_hash.into()); *call_depth];

            execute_stored_payment_by_contract_name(&mut builder, *call_depth, subcalls)
        }
    }

    #[ignore]
    #[test]
    fn stored_payment_by_hash_to_stored_session() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_contract(current_contract_hash.into()); *call_depth];

            execute_stored_payment_by_package_name(&mut builder, *call_depth, subcalls)
        }
    }

    #[ignore]
    #[test]
    fn stored_versioned_payment_by_hash_to_stored_contract() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);
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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

            let subcalls = vec![super::stored_contract(current_contract_hash.into()); *call_depth];

            execute_stored_payment_by_contract_name(&mut builder, *call_depth, subcalls)
        }
    }

    #[ignore]
    #[test]
    fn stored_payment_by_hash_to_stored_contract() {
        for call_depth in DEPTHS {
            let mut builder = super::setup();
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);

            let current_contract_package_hash =
                default_entity.get_package_hash(CONTRACT_PACKAGE_NAME);

            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
            let default_account = builder
                .get_entity_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
                .unwrap();
            let named_keys = builder.get_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR);

            let default_entity = EntityWithKeys::new(default_account, named_keys);
            let current_contract_hash = default_entity.get_entity_hash(CONTRACT_NAME);

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
