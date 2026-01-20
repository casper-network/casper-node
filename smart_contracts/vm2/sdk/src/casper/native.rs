use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    fmt,
    panic::UnwindSafe,
    ptr::{self, NonNull},
    slice,
    sync::{Arc, Mutex},
};

use crate::{
    linkme::distributed_slice,
    types::{
        ControlFunctionOption, CryptoFunctionOption, DelegatorKind, EmitFunctionOption, EntityAddr,
        GlobalStateFunctionOption, HashAlgorithm, IOFunctionOption, PublicKey, Reservation,
        SystemContractOption,
    },
};
use borsh::BorshSerialize;
use bytes::Bytes;
use casper_contract_sdk_sys::{CreateResult, EnvInfo};
use casper_executor_wasm_common::{error::HOST_ERROR_SUCCESS, keyspace::Keyspace};

use super::Entity;

#[repr(C)]
pub struct Param {
    pub name_ptr: *const u8,
    pub name_len: usize,
}

/// The kind of export that is being registered.
///
/// This is used to identify the type of export and its name.
///
/// Depending on the location of given function it may be registered as a:
///
/// * `SmartContract` (if it's part of a `impl Contract` block),
/// * `TraitImpl` (if it's part of a `impl Trait for Contract` block),
/// * `Function` (if it's a standalone function).
///
/// This is used to dispatch exports under native code i.e. you want to write a test that calls
/// "foobar" regardless of location.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntryPointKind {
    /// Smart contract.
    ///
    /// This is used to identify the smart contract and its name.
    ///
    /// The `struct_name` is the name of the smart contract that is being registered.
    /// The `name` is the name of the function that is being registered.
    SmartContract {
        struct_name: &'static str,
        name: &'static str,
    },
    /// Trait implementation.
    ///
    /// This is used to identify the trait implementation and its name.
    ///
    /// The `trait_name` is the name of the trait that is being implemented.
    /// The `impl_name` is the name of the implementation.
    /// The `name` is the name of the function that is being implemented.
    TraitImpl {
        trait_name: &'static str,
        impl_name: &'static str,
        name: &'static str,
    },
    /// Function export.
    ///
    /// This is used to identify the function export and its name.
    ///
    /// The `name` is the name of the function that is being exported.
    Function { name: &'static str },
}

impl EntryPointKind {
    pub fn name(&self) -> &'static str {
        match self {
            EntryPointKind::SmartContract { name, .. }
            | EntryPointKind::TraitImpl { name, .. }
            | EntryPointKind::Function { name } => name,
        }
    }
}

/// Export is a structure that contains information about the exported function.
///
/// This is used to register the export and its name and physical location in the smart contract
/// source code.
pub struct EntryPoint {
    /// The kind of entry point that is being registered.
    pub kind: EntryPointKind,
    pub fptr: fn() -> (),
    pub module_path: &'static str,
    pub file: &'static str,
    pub line: u32,
}

#[distributed_slice]
#[linkme(crate = crate::linkme)]
pub static ENTRY_POINTS: [EntryPoint];

impl fmt::Debug for EntryPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            kind,
            fptr: _,
            module_path,
            file,
            line,
        } = self;

        f.debug_struct("Export")
            .field("kind", kind)
            .field("fptr", &"<fptr>")
            .field("module_path", module_path)
            .field("file", file)
            .field("line", line)
            .finish()
    }
}

#[derive(Debug)]
pub enum NativeTrap {
    Panic(Box<dyn std::any::Any + Send + 'static>),
}

impl NativeTrap {
    pub fn downcast_value<T: std::any::Any>(&self) -> Option<&T> {
        match self {
            NativeTrap::Panic(any) => any.downcast_ref::<T>(),
        }
    }
}
pub type Container = BTreeMap<u64, BTreeMap<Bytes, Bytes>>;

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct NativeParam(pub(crate) String);

impl From<&Param> for NativeParam {
    fn from(val: &Param) -> Self {
        let name =
            String::from_utf8_lossy(unsafe { slice::from_raw_parts(val.name_ptr, val.name_len) })
                .into_owned();
        NativeParam(name)
    }
}

#[derive(Clone, Debug)]
pub struct ExpectedCall {
    /// If None, the input data will not be checked
    input_match: Option<Vec<u8>>,
    /// If None, the ffi opt will not be checked
    ffi_opt_match: Option<u32>,
    output_data: Option<Vec<u8>>,
    result_code: u32,
}

#[derive(BorshSerialize)]
pub struct CreateInputExpectation<'a> {
    code: Option<&'a [u8]>,
    transferred_value: u64,
    constructor: Option<&'a str>,
    constructor_data: Option<&'a [u8]>,
    seed: Option<&'a [u8; 32]>,
    bundle_data: Option<&'a [u8]>,
}

#[derive(BorshSerialize)]
pub struct UpgradeInputExpectation<'a> {
    code: &'a [u8],
    entry_point: Option<&'a str>,
    input_data: Option<&'a [u8]>,
}

impl ExpectedCall {
    pub fn new(
        input_match: Option<Vec<u8>>,
        ffi_opt_match: Option<u32>,
        output_data: Option<Vec<u8>>,
        result_code: u32,
    ) -> Self {
        Self {
            input_match,
            ffi_opt_match,
            output_data,
            result_code,
        }
    }

    pub fn success(
        input_match: Option<Vec<u8>>,
        ffi_opt_match: Option<u32>,
        output_data: Option<Vec<u8>>,
    ) -> Self {
        Self::new(input_match, ffi_opt_match, output_data, 0)
    }

    pub fn expect_transfer(
        input_expectation: Option<(&EntityAddr, u64)>,
        result_code: u32,
    ) -> Self {
        let input = input_expectation
            .map(|(entity_addr, amount)| borsh::to_vec(&(entity_addr, amount)).unwrap());
        Self::new(
            input,
            Some(SystemContractOption::Transfer as u32),
            Some(vec![]),
            result_code,
        )
    }

    pub fn expect_transfer_purse(
        input_expectation: Option<([u8; 32], u64)>,
        result_code: u32,
    ) -> Self {
        let input = input_expectation
            .map(|(target_purse, amount)| borsh::to_vec(&(target_purse, amount)).unwrap());
        Self::new(
            input,
            Some(SystemContractOption::TransferPurse as u32),
            Some(vec![]),
            result_code,
        )
    }

    pub fn expect_burn(input_expectation: Option<u64>, result_code: u32) -> Self {
        let input = input_expectation.map(|amount| borsh::to_vec(&amount).unwrap());
        Self::new(
            input,
            Some(SystemContractOption::Burn as u32),
            Some(vec![]),
            result_code,
        )
    }

    pub fn expect_activate_bid(input_expectation: Option<&PublicKey>, result_code: u32) -> Self {
        let input = input_expectation.map(|input| borsh::to_vec(&input).unwrap());
        Self::new(
            input,
            Some(SystemContractOption::ActivateBid as u32),
            Some(vec![]),
            result_code,
        )
    }

    pub fn expect_bid(
        input_expectation: Option<(&PublicKey, u8, u64, u64, u64, u32)>,
        result_code: u32,
    ) -> Self {
        let input = input_expectation.map(|input| borsh::to_vec(&input).unwrap());
        Self::new(
            input,
            Some(SystemContractOption::Bid as u32),
            Some(vec![]),
            result_code,
        )
    }

    pub fn expect_withdraw(input_expectation: Option<(&PublicKey, u64)>, result_code: u32) -> Self {
        let input = input_expectation.map(|input| borsh::to_vec(&input).unwrap());
        Self::new(
            input,
            Some(SystemContractOption::Withdraw as u32),
            Some(vec![]),
            result_code,
        )
    }

    pub fn expect_delegate(
        input_expectation: Option<(&DelegatorKind, &PublicKey, u64)>,
        result_code: u32,
    ) -> Self {
        let input = input_expectation.map(|input| borsh::to_vec(&input).unwrap());
        Self::new(
            input,
            Some(SystemContractOption::Delegate as u32),
            Some(vec![]),
            result_code,
        )
    }

    pub fn expect_undelegate(
        input_expectation: Option<(&DelegatorKind, &PublicKey, u64)>,
        result_code: u32,
    ) -> Self {
        let input = input_expectation.map(|input| borsh::to_vec(&input).unwrap());
        Self::new(
            input,
            Some(SystemContractOption::Undelegate as u32),
            Some(vec![]),
            result_code,
        )
    }

    pub fn expect_redelegate(
        input_expectation: Option<(&DelegatorKind, &PublicKey, u64)>,
        result_code: u32,
    ) -> Self {
        let input = input_expectation.map(|input| borsh::to_vec(&input).unwrap());
        Self::new(
            input,
            Some(SystemContractOption::Redelegate as u32),
            Some(vec![]),
            result_code,
        )
    }

    pub fn expect_add_reservation(
        input_expectation: Option<&Reservation>,
        result_code: u32,
    ) -> Self {
        let input = input_expectation.map(|input| borsh::to_vec(&input).unwrap());
        Self::new(
            input,
            Some(SystemContractOption::AddReservation as u32),
            Some(vec![]),
            result_code,
        )
    }

    pub fn expect_cancel_reservation(
        input_expectation: Option<(&PublicKey, &DelegatorKind)>,
        result_code: u32,
    ) -> Self {
        let input = input_expectation.map(|input| borsh::to_vec(&input).unwrap());
        Self::new(
            input,
            Some(SystemContractOption::CancelReservation as u32),
            Some(vec![]),
            result_code,
        )
    }

    pub fn expect_change_public_key(
        input_expectation: Option<(&PublicKey, &PublicKey)>,
        result_code: u32,
    ) -> Self {
        let input = input_expectation.map(|input| borsh::to_vec(&input).unwrap());
        Self::new(
            input,
            Some(SystemContractOption::ChangePublicKey as u32),
            Some(vec![]),
            result_code,
        )
    }

    pub fn expect_print(text: &str) -> Self {
        Self::success(
            Some(text.as_bytes().to_vec()),
            Some(EmitFunctionOption::PrintStd as u32),
            Some(vec![]),
        )
    }

    pub fn expect_native(input_expectation: Option<(String, Vec<u8>)>, result_code: u32) -> Self {
        let input = input_expectation
            .map(|(topic_name, message)| borsh::to_vec(&(topic_name, message)).unwrap());
        Self::new(
            input,
            Some(EmitFunctionOption::Native as u32),
            Some(vec![]),
            result_code,
        )
    }

    pub fn expect_write(input: Option<(&Keyspace, &[u8])>) -> Self {
        Self::expect_write_with_result_code(input, HOST_ERROR_SUCCESS)
    }

    pub fn expect_write_with_result_code(
        input_expectation: Option<(&Keyspace, &[u8])>,
        result_code: u32,
    ) -> Self {
        let input = input_expectation.map(|(key, value)| {
            let mut input_data = key.to_host_input_data().unwrap();
            borsh::to_writer(&mut input_data, value).unwrap();
            input_data
        });

        Self::new(
            input,
            Some(GlobalStateFunctionOption::Write as u32),
            Some(vec![]),
            result_code,
        )
    }

    pub fn expect_read(
        input_expectation: Option<&Keyspace>,
        maybe_output: Option<&[u8]>,
        result_code: u32,
    ) -> Self {
        let input_data = input_expectation.map(|x| x.to_host_input_data().unwrap());
        Self::new(
            input_data,
            Some(GlobalStateFunctionOption::Read as u32),
            maybe_output.map(|x| x.to_vec()),
            result_code,
        )
    }

    pub fn expect_create(
        input_expectation: Option<CreateInputExpectation>,
        output: Option<CreateResult>,
        result_code: u32,
    ) -> Self {
        let input_data = input_expectation.map(|input| borsh::to_vec(&input).unwrap());
        Self::new(
            input_data,
            Some(GlobalStateFunctionOption::Create as u32),
            output.map(|x| borsh::to_vec(&x).unwrap()),
            result_code,
        )
    }

    pub fn expect_remove(input: Option<Keyspace>) -> Self {
        Self::expect_remove_with_result_code(input, HOST_ERROR_SUCCESS)
    }

    pub fn expect_remove_with_result_code(
        input_expectation: Option<Keyspace>,
        result_code: u32,
    ) -> Self {
        let input = input_expectation.map(|key| key.to_host_input_data().unwrap());
        Self::new(
            input,
            Some(GlobalStateFunctionOption::Remove as u32),
            Some(vec![]),
            result_code,
        )
    }

    pub fn expect_get_balance(
        input_expectation: Option<(u32, [u8; 32])>,
        output: Option<u64>,
        result_code: u32,
    ) -> Self {
        let input = input_expectation.map(|input| borsh::to_vec(&input).unwrap());
        Self::new(
            input,
            Some(GlobalStateFunctionOption::GetBalance as u32),
            output.map(|v| v.to_le_bytes().to_vec()),
            result_code,
        )
    }

    pub fn expect_return(
        input_expectation: Option<(u32, Option<Vec<u8>>)>,
        return_code: u32,
    ) -> Self {
        let input = input_expectation.map(|input| borsh::to_vec(&input).unwrap());

        Self::new(
            input,
            Some(IOFunctionOption::Return as u32),
            None,
            return_code,
        )
    }

    pub fn expect_revert(input_expectation: Option<&[u8]>, return_code: u32) -> Self {
        Self::new(
            input_expectation.map(|x| borsh::to_vec(&x).unwrap()),
            Some(IOFunctionOption::Revert as u32),
            None,
            return_code,
        )
    }

    pub fn expect_call(
        input_expectation: Option<([u8; 32], &[u8], &str, u64)>,
        output: Option<&[u8]>,
        return_code: u32,
    ) -> Self {
        Self::new(
            input_expectation.map(|x| borsh::to_vec(&x).unwrap()),
            Some(ControlFunctionOption::Call as u32),
            output.map(|x| x.to_vec()),
            return_code,
        )
    }

    pub fn expect_upgrade(
        input_expectation: Option<UpgradeInputExpectation>,
        return_code: u32,
    ) -> Self {
        Self::new(
            input_expectation.map(|x| borsh::to_vec(&x).unwrap()),
            Some(ControlFunctionOption::Call as u32),
            None,
            return_code,
        )
    }

    pub fn expect_get_info(output: Option<EnvInfo>) -> Self {
        Self::expect_get_info_with_result_code(output, HOST_ERROR_SUCCESS)
    }

    pub fn expect_get_info_with_result_code(output: Option<EnvInfo>, result_code: u32) -> Self {
        let output_data = output
            .map(|out| {
                borsh::to_vec(&(
                    out.protocol_version_major,
                    out.protocol_version_minor,
                    out.protocol_version_patch,
                    out.block_height,
                    out.block_time,
                    out.parent_block_hash,
                    out.transferred_value,
                    out.caller_addr,
                    out.caller_kind,
                    out.callee_addr,
                    out.callee_kind,
                ))
                .unwrap()
            })
            .unwrap_or_default();
        Self::new(
            Some(vec![]),
            Some(GlobalStateFunctionOption::GetInfo as u32),
            Some(output_data),
            result_code,
        )
    }

    pub fn expect_copy_input(input_to_copy: &[u8]) -> Self {
        Self::expect_copy_input_with_result_code(input_to_copy, HOST_ERROR_SUCCESS)
    }

    pub fn expect_copy_input_with_result_code(input_to_copy: &[u8], result_code: u32) -> Self {
        Self::new(
            Some(vec![]),
            Some(IOFunctionOption::CopyInput as u32),
            Some(input_to_copy.to_vec()),
            result_code,
        )
    }

    pub fn expect_generic_hash(
        input_expectation: Option<(&[u8], HashAlgorithm)>,
        output: Option<Vec<u8>>,
        result_code: u32,
    ) -> Self {
        let input = input_expectation.map(|(input_data, input_algorithm)| {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&(input_algorithm as u32).to_le_bytes());
            bytes.extend_from_slice(&(input_data.len() as u32).to_le_bytes());
            bytes.extend_from_slice(input_data);
            bytes
        });
        Self::new(
            input,
            Some(CryptoFunctionOption::GenericHash as u32),
            output,
            result_code,
        )
    }
}

pub trait Environment {
    /// # Safety
    /// Implementations of this function potentially can dereference `input_ptr` reading
    /// `input_size` bytes of memory.
    unsafe fn casper_ffi(
        &self,
        ffi_opt: u32,
        input_ptr: *const u8,
        input_size: usize,
        alloc: extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8,
        alloc_ctx: *const core::ffi::c_void,
    ) -> u32;

    /// This function will be called test execution to perform any actions required for
    /// teardown
    fn teardown(&self);
}

#[derive(Clone, Debug)]
pub struct EnvironmentMock {
    expected_calls: Arc<Mutex<VecDeque<ExpectedCall>>>,
}

impl Environment for EnvironmentMock {
    unsafe fn casper_ffi(
        &self,
        ffi_opt: u32,
        input_ptr: *const u8,
        input_size: usize,
        alloc: extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8,
        alloc_ctx: *const core::ffi::c_void,
    ) -> u32 {
        let expectation = self.deque_expectation().unwrap_or_else(|| {
            panic!(
                "Trying to call `casper_ffi` (ffi_opt={}) without enqueued mock results",
                ffi_opt
            )
        });
        if let Some(expected_ffi_opt) = expectation.ffi_opt_match {
            assert_eq!(
                expected_ffi_opt, ffi_opt,
                "Expected casper_ffi to be called with ffi_opt={}. got {}",
                expected_ffi_opt, ffi_opt
            );
        }
        if let Some(expected_input) = expectation.input_match {
            let input = if input_size > 0 {
                if input_ptr.is_null() {
                    panic!("Trying to dereference a null pointer")
                }
                unsafe { slice::from_raw_parts(input_ptr, input_size) }.to_owned()
            } else {
                vec![]
            };
            assert_eq!(expected_input, input);
        }
        if let Some(output_data) = expectation.output_data {
            let ptr = NonNull::new(alloc(output_data.len(), alloc_ctx as _));
            if let Some(ptr) = ptr {
                unsafe {
                    ptr::copy_nonoverlapping(output_data.as_ptr(), ptr.as_ptr(), output_data.len());
                }
            }
        }

        expectation.result_code
    }

    fn teardown(&self) {
        self.assert_no_expectations_left()
    }
}

impl Default for EnvironmentMock {
    fn default() -> Self {
        Self {
            expected_calls: Arc::new(Mutex::new(VecDeque::new())),
        }
    }
}

pub const DEFAULT_ADDRESS: Entity = Entity::Account([42; 32]);

impl EnvironmentMock {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_expectation(&self, expected_call: ExpectedCall) {
        let mut guard = self
            .expected_calls
            .lock()
            .expect("Expected the mutex to not be poisoned");
        guard.push_back(expected_call);
    }

    pub fn deque_expectation(&self) -> Option<ExpectedCall> {
        let mut guard = self
            .expected_calls
            .lock()
            .expect("Expected the mutex to not be poisoned");
        guard.pop_front()
    }

    fn assert_no_expectations_left(&self) {
        let guard = self
            .expected_calls
            .lock()
            .expect("Expected the mutex to not be poisoned");
        assert!(guard.is_empty())
    }
}

thread_local! {
    static CURRENT_ENV: RefCell<Option<Arc<dyn Environment>>> = RefCell::new(None);
}

pub fn with_environment<T>(f: impl FnOnce(&dyn Environment) -> T) -> T {
    CURRENT_ENV.with_borrow(|env| match env {
        Some(env) => f(env.as_ref()),
        None => {
            panic!("Couldn't execute with_environment since there is no environment")
        }
    })
}

/// This function runs a lambda capturing any panics. It then wraps the panic in a `NativeTrap` and
/// resturns. This function replaces default panic hook, so it will muffle any stack traces of error
/// messages.
pub fn run_expecting_panic<F, T>(func: F) -> Result<T, NativeTrap>
where
    F: FnOnce() -> T + Send + UnwindSafe,
{
    use std::panic;
    let call_result = panic::catch_unwind(func);
    match call_result {
        Ok(t) => Ok(t),
        Err(error) => Err(NativeTrap::Panic(error)),
    }
}

pub fn with_env<T: Environment + 'static, F>(new_env: Arc<T>, func: F)
where
    F: FnOnce(),
{
    CURRENT_ENV.with_borrow_mut(|env| *env = Some(new_env));
    func();
}

mod symbols {
    use crate::casper::native::with_environment;

    #[no_mangle]
    pub extern "C" fn casper_ffi(
        ffi_opt: u32,
        input_ptr: *const u8,
        input_size: usize,
        alloc: extern "C" fn(usize, *mut core::ffi::c_void) -> *mut u8,
        alloc_ctx: *const core::ffi::c_void,
    ) -> u32 {
        with_environment(|mock| unsafe {
            mock.casper_ffi(ffi_opt, input_ptr, input_size, alloc, alloc_ctx)
        })
    }
}

#[cfg(test)]
mod tests {
    use casper_executor_wasm_common::{
        error::{HostResult, HOST_ERROR_INVALID_INPUT},
        flags::ReturnFlags,
        keyspace::Keyspace,
    };

    use crate::casper;

    use super::*;

    #[test]
    fn foo() {
        let env = Arc::new(EnvironmentMock::new());
        with_env(env.clone(), || {
            env.add_expectation(ExpectedCall::expect_print("Hello"));
            let _ = casper::print("Hello");

            let key = Keyspace::NamedValue("abc");
            env.add_expectation(ExpectedCall::expect_write(Some((&key, b"value 1"))));
            casper::write(key, b"value 1").unwrap();

            let key = Keyspace::NamedValue("abc");
            env.add_expectation(ExpectedCall::expect_write_with_result_code(
                Some((&key, b"value 1")),
                HOST_ERROR_INVALID_INPUT,
            ));
            assert_eq!(
                casper::write(key, b"value 1"),
                Err(HostResult::InvalidInput)
            );

            let key_3 = Keyspace::NamedValue("abc");
            env.add_expectation(ExpectedCall::expect_read(
                Some(&key_3),
                Some(b"value 2"),
                HOST_ERROR_SUCCESS,
            ));
            assert_eq!(casper::read_into_vec(key_3), Ok(Some(b"value 2".to_vec())));

            let key_4 = Keyspace::NamedValue("abc2");
            env.add_expectation(ExpectedCall::expect_read(
                Some(&key_4),
                Some(&[5]),
                HOST_ERROR_SUCCESS,
            ));
            assert_eq!(casper::read_into_vec(key_4), Ok(Some(vec![5])));

            let key_5 = Keyspace::NamedValue("abc3");
            env.add_expectation(ExpectedCall::expect_read(
                Some(&key_5),
                None,
                HOST_ERROR_INVALID_INPUT,
            ));
            assert_eq!(casper::read_into_vec(key_5), Err(HostResult::InvalidInput));

            env.add_expectation(ExpectedCall::expect_get_info(Some(EnvInfo {
                protocol_version_major: 2,
                protocol_version_minor: 1,
                protocol_version_patch: 0,
                block_height: 100100,
                block_time: 200200,
                parent_block_hash: [1; 32],
                transferred_value: 123,
                caller_addr: [2; 32],
                caller_kind: 1,
                callee_addr: [3; 32],
                callee_kind: 2,
            })));

            assert_eq!(casper::get_caller(), Entity::Contract([2; 32]));
        });
    }

    #[test]
    fn test_returns() {
        let env = Arc::new(EnvironmentMock::new());
        with_env(env.clone(), || {
            env.add_expectation(ExpectedCall::expect_return(
                Some((1, Some([1, 2, 3].to_vec()))),
                HOST_ERROR_SUCCESS,
            ));
            let res = run_expecting_panic(|| casper::ret(ReturnFlags::ROLLBACK, Some(&[1, 2, 3])));
            assert!(res.is_err());
        });
    }
}
