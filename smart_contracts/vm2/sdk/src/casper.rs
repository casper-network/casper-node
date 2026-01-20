pub mod altbn128;
#[cfg(all(not(target_arch = "wasm32"), feature = "std"))]
pub mod native;

use core::cell::RefCell;

#[cfg(all(not(target_arch = "wasm32"), feature = "std"))]
use crate::abi::{ABITypeInfo, CasperABI, EnumVariant};
use crate::{
    compat::types::{CLType, CLTyped},
    log,
    prelude::{ffi::c_void, marker::PhantomData, ptr, Vec},
    reserve_vec_space,
    serializers::borsh::{BorshDeserialize, BorshSerialize},
    types::{
        Address, CallError, ControlFunctionOption, CryptoFunctionOption, EmitFunctionOption,
        GlobalStateFunctionOption, HashAlgorithm, IOFunctionOption, PublicKey,
    },
    FieldStateAccess, Message, ToCallData,
};
use casper_contract_macros::TypeUid;

use crate::types::{EntityAddr, SystemContractOption};
use casper_contract_sdk_sys::{CreateResult, EnvInfo};
use casper_executor_wasm_common::{
    error::{result_from_code, HostResult, HOST_ERROR_SUCCESS},
    flags::ReturnFlags,
    keyspace::{ContextAddr, Keyspace, StateAddrInner},
};

/// Print a message.
#[inline]
pub fn print(msg: &str) -> Result<(), HostResult> {
    let option = EmitFunctionOption::PrintStd;
    let res = call_ffi(option.into(), msg.as_bytes(), Some(|_| None));
    result_from_code(res)
}

pub enum Alloc<F: FnOnce(usize) -> Option<ptr::NonNull<u8>>> {
    Callback(F),
    Static(ptr::NonNull<u8>),
}

extern "C" fn alloc_callback<F: FnOnce(usize) -> Option<ptr::NonNull<u8>>>(
    len: usize,
    ctx: *mut c_void,
) -> *mut u8 {
    let opt_closure = ctx.cast::<Option<F>>();
    let allocated_ptr = unsafe { (*opt_closure).take().unwrap()(len) };
    match allocated_ptr {
        Some(ptr) => ptr.as_ptr(),
        None => ptr::null_mut(),
    }
}

/// Copy input data into a vector.
pub fn copy_input() -> Vec<u8> {
    let ret = {
        let (output_data, result_code) = casper_ffi(IOFunctionOption::CopyInput.into(), &[]);
        call_result_from_code(result_code).map(|()| match output_data {
            Some(data) => data,
            None => panic!("Couldn't deserialize output"),
        })
    };

    match ret {
        Ok(data) => data,
        Err(err) => panic!("Failed to copy input: {:?}", err),
    }
}

/// Return from the contract.
pub fn ret(flags: ReturnFlags, data: Option<&[u8]>) -> ! {
    let args = (flags.bits(), data);
    let arg_bytes = borsh::to_vec(&args).expect("Expected borsh to work");
    let _ = casper_ffi(IOFunctionOption::Return.into(), &arg_bytes);

    unreachable!()
}

pub fn revert(message: &str) -> ! {
    let message_bytes = message.as_bytes();
    let arg_bytes = borsh::to_vec(&(message_bytes)).expect("Expected borsh to work");

    let _ = casper_ffi(IOFunctionOption::Revert.into(), &arg_bytes);

    unreachable!()
}

/// Read from the global state.
pub fn read<F: FnOnce(usize) -> Option<ptr::NonNull<u8>>>(
    key: Keyspace,
    f: F,
) -> Result<Option<()>, HostResult> {
    let input_data = key.to_host_input_data().expect("Expected borsh to work");

    extern "C" fn alloc_cb<F: FnOnce(usize) -> Option<ptr::NonNull<u8>>>(
        len: usize,
        ctx: *mut c_void,
    ) -> *mut u8 {
        let opt_closure = ctx as *mut Option<F>;
        let allocated_ptr = unsafe { (*opt_closure).take().unwrap()(len) };
        match allocated_ptr {
            Some(mut ptr) => unsafe { ptr.as_mut() },
            None => ptr::null_mut(),
        }
    }

    let ctx = &Some(f) as *const _ as *mut _;

    let ret = unsafe {
        casper_contract_sdk_sys::casper_ffi(
            GlobalStateFunctionOption::Read.into(),
            input_data.as_ptr(),
            input_data.len(),
            alloc_cb::<F>,
            ctx,
        )
    };

    match result_from_code(ret) {
        Ok(()) => Ok(Some(())),
        Err(HostResult::NotFound) => Ok(None),
        Err(err) => Err(err),
    }
}

/// Write to the global state.
pub fn write(key: Keyspace, value: &[u8]) -> Result<(), HostResult> {
    let mut input_data = key.to_host_input_data().expect("Expected borsh to work");
    borsh::to_writer(&mut input_data, value).expect("Expected borsh to work");

    extern "C" fn alloc_cb(_len: usize, _ctx: *mut c_void) -> *mut u8 {
        // Write shouldn't have any output data and should not return anything
        ptr::null_mut()
    }

    let ctx = &None::<u8> as *const _ as *mut _;
    let ret = unsafe {
        casper_contract_sdk_sys::casper_ffi(
            GlobalStateFunctionOption::Write.into(),
            input_data.as_ptr(),
            input_data.len(),
            alloc_cb,
            ctx,
        )
    };
    result_from_code(ret)
}

/// Write to the global state.
pub fn store_package_under_key(named_key: &str) -> Result<(), HostResult> {
    let input_data = borsh::to_vec(named_key).expect("Expected borsh to work");
    extern "C" fn alloc_cb(_len: usize, _ctx: *mut c_void) -> *mut u8 {
        // Write shouldn't have any output data and should not return anything
        ptr::null_mut()
    }
    let ctx = &None::<u8> as *const _ as *mut _;
    let ret = unsafe {
        casper_contract_sdk_sys::casper_ffi(
            GlobalStateFunctionOption::StorePackageUnderKey.into(),
            input_data.as_ptr(),
            input_data.len(),
            alloc_cb,
            ctx,
        )
    };
    result_from_code(ret)
}

/// Remove from the global state.
pub fn remove(key: Keyspace) -> Result<(), HostResult> {
    let input_data = key.to_host_input_data().expect("Expected borsh to work");

    extern "C" fn alloc_cb(_len: usize, _ctx: *mut c_void) -> *mut u8 {
        // Write shouldn't have any output data and should not return anything
        ptr::null_mut()
    }

    let ctx = &None::<u8> as *const _ as *mut _;
    let ret = unsafe {
        casper_contract_sdk_sys::casper_ffi(
            GlobalStateFunctionOption::Remove.into(),
            input_data.as_ptr(),
            input_data.len(),
            alloc_cb,
            ctx,
        )
    };
    result_from_code(ret)
}

/// Create a new contract instance.
pub fn create(
    code: Option<&[u8]>,
    transferred_value: u64,
    constructor: Option<&str>,
    constructor_data: Option<&[u8]>,
    seed: Option<&[u8; 32]>,
    bundle_data: Option<&[u8]>,
) -> Result<CreateResult, CallError> {
    let input_data = borsh::to_vec(&(
        transferred_value,
        code,
        seed,
        constructor,
        constructor_data,
        bundle_data,
    ))
    .expect("Expected borsh to work");
    let (output, exit_code) = casper_ffi(GlobalStateFunctionOption::Create.into(), &input_data);
    match exit_code {
        HOST_ERROR_SUCCESS => match output {
            Some(output) => borsh::from_slice(&output).map_err(|_| CallError::InvalidOutput),
            None => Err(CallError::InvalidOutput),
        },
        other_status => {
            let Ok(error) = CallError::try_from(other_status) else {
                panic!("Couldn't interpret error from host: {}", other_status);
            };
            Err(error)
        }
    }
}

pub(crate) fn call_ffi<F: FnOnce(usize) -> Option<ptr::NonNull<u8>>>(
    ffi_opt: u32,
    input_data: &[u8],
    alloc: Option<F>,
) -> u32 {
    unsafe {
        casper_contract_sdk_sys::casper_ffi(
            ffi_opt,
            input_data.as_ptr(),
            input_data.len(),
            alloc_callback::<F>,
            &alloc as *const _ as *mut _,
        )
    }
}

pub(crate) fn call_result_from_code(result_code: u32) -> Result<(), CallError> {
    if result_code == HOST_ERROR_SUCCESS {
        Ok(())
    } else {
        Err(CallError::try_from(result_code)
            .unwrap_or_else(|_| panic!("Unexpected error code: {}", result_code)))
    }
}

/// Call a host function.
pub fn casper_ffi(ffi_opt: u32, input_data: &[u8]) -> (Option<Vec<u8>>, u32) {
    let mut output = None;
    let result_code = call_ffi(
        ffi_opt,
        input_data,
        Some(|size| {
            let mut vec = Vec::new();
            reserve_vec_space(&mut vec, size);
            let result = Some(unsafe { ptr::NonNull::new_unchecked(vec.as_mut_ptr()) });
            output = Some(vec);
            result
        }),
    );
    (output, result_code)
}

/// Call a contract.
pub fn casper_call(
    address: &Address,
    transferred_value: u64,
    entry_point: &str,
    input_data: &[u8],
    contract_version: Option<u32>,
    contract_protocol_version_major: Option<u32>,
) -> (Option<Vec<u8>>, Result<(), CallError>) {
    let input_data = borsh::to_vec(&(
        address,
        input_data,
        entry_point,
        transferred_value,
        contract_version,
        contract_protocol_version_major,
    ))
    .expect("borsh");
    let (output_data, result_code) = casper_ffi(ControlFunctionOption::Call.into(), &input_data);
    (output_data, call_result_from_code(result_code))
}

/// Upgrade the contract.
pub fn upgrade(
    code: &[u8],
    entry_point: Option<&str>,
    input_data: Option<&[u8]>,
) -> Result<(), CallError> {
    let input_data =
        borsh::to_vec(&(code, entry_point, input_data)).expect("Expected borsh to work");
    let (_output_data, result_code) =
        casper_ffi(ControlFunctionOption::Upgrade.into(), &input_data);
    call_result_from_code(result_code)
}

/// Read from the global state into a vector.
pub fn read_into_vec(key: Keyspace) -> Result<Option<Vec<u8>>, HostResult> {
    let mut vec = Vec::new();
    let out = read(key, |size| reserve_vec_space(&mut vec, size))?.map(|()| vec);
    Ok(out)
}

/// Read full contract state using macro-generated field methods
pub fn read_contract_state<T: FieldStateAccess>() -> Result<T, HostResult> {
    T::read_state_from_fields()
}

/// Write full contract state using macro-generated field methods
pub fn write_contract_state<T: FieldStateAccess>(state: &T) -> Result<(), HostResult> {
    state.write_state_to_fields()
}

const STATE_INITIALIZED_FIELD: &str = "STATE_INITIALIZED";

pub fn is_contract_state_initialized() -> Result<bool, HostResult> {
    let state_addr = StateAddrInner::new(STATE_INITIALIZED_FIELD);
    let keyspace = Keyspace::Context(ContextAddr::from(state_addr));
    let state_initialized = read_into_vec(keyspace)?;

    match state_initialized {
        Some(data) => {
            let initialized: bool = borsh::from_slice(&data).expect("Expected borsh to work");
            Ok(initialized)
        }
        None => Ok(false),
    }
}

pub fn mark_contract_initialization_state(initialized: bool) -> Result<(), HostResult> {
    let state_addr = StateAddrInner::new(STATE_INITIALIZED_FIELD);
    let keyspace = Keyspace::Context(ContextAddr::from(state_addr));
    let data = borsh::to_vec(&initialized).expect("Expected borsh to work");
    write(keyspace, &data)?;
    Ok(())
}

#[derive(Debug)]
pub struct CallResult<T: ToCallData> {
    pub data: Option<Vec<u8>>,
    pub result: Result<(), CallError>,
    pub marker: PhantomData<T>,
}

impl<T: ToCallData> CallResult<T> {
    pub fn into_result<'a>(self) -> Result<T::Return<'a>, CallError>
    where
        <T as ToCallData>::Return<'a>: BorshDeserialize,
    {
        match self.result {
            Ok(()) | Err(CallError::CalleeRolledBack) => {
                let data = self.data.unwrap_or_default();
                Ok(borsh::from_slice(&data).unwrap())
            }
            Err(call_error) => Err(call_error),
        }
    }

    pub fn did_rollback(&self) -> bool {
        self.result == Err(CallError::CalleeRolledBack)
    }
}

/// Call a contract.
pub fn call<T: ToCallData>(
    contract_address: &Address,
    transferred_value: u64,
    call_data: T,
    contract_version: Option<u32>,
    contract_protocol_version_major: Option<u32>,
) -> Result<CallResult<T>, CallError> {
    let input_data = call_data.input_data().unwrap_or_default();

    let (maybe_data, result_code) = casper_call(
        contract_address,
        transferred_value,
        call_data.entry_point(),
        &input_data,
        contract_version,
        contract_protocol_version_major,
    );
    match result_code {
        Ok(()) | Err(CallError::CalleeRolledBack) => Ok(CallResult::<T> {
            data: maybe_data,
            result: result_code,
            marker: PhantomData,
        }),
        Err(error) => Err(error),
    }
}

#[derive(Debug)]
pub enum GetEnvInfoError {
    NoData,
    EnvInfoUnparseable,
    UnexpectedResultCode(u32),
}

thread_local! {
    /// Env info cache for the current execution
    static ENV_INFO: RefCell<Option<EnvInfo>> = const {RefCell::new(None)};
}

/// Get the environment info.
pub fn get_env_info() -> Result<EnvInfo, GetEnvInfoError> {
    // The assumption is that fetching env_info data is idempotent in the
    // scope of one wasm execution
    ENV_INFO.with_borrow_mut(|el| {
        if let Some(env_info) = el {
            Ok(env_info.clone())
        } else {
            let (output_data, res) = casper_ffi(GlobalStateFunctionOption::GetInfo.into(), &[]);
            if res == 0 {
                if let Some(output_data) = output_data {
                    let env_info_res: Result<EnvInfo, GetEnvInfoError> =
                        borsh::from_slice(&output_data)
                            .map_err(|_| GetEnvInfoError::EnvInfoUnparseable);
                    if let Ok(env_info) = &env_info_res {
                        *el = Some(env_info.clone())
                    }
                    env_info_res
                } else {
                    Err(GetEnvInfoError::NoData)
                }
            } else {
                Err(GetEnvInfoError::UnexpectedResultCode(res))
            }
        }
    })
}

/// Get the caller.
#[must_use]
pub fn get_caller() -> Entity {
    let info = get_env_info().expect("expected get_env_info to yield data");
    Entity::from_parts(info.caller_kind, info.caller_addr).expect("Invalid caller kind")
}

#[must_use]
pub fn get_callee() -> Entity {
    let info = get_env_info().expect("expected get_env_info to yield data");
    Entity::from_parts(info.callee_kind, info.callee_addr).expect("Invalid callee kind")
}

/// Enum representing either an account or a contract.
#[derive(
    BorshSerialize,
    BorshDeserialize,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    TypeUid,
)]
#[type_uid(crate = "crate::common::type_uid")]
pub enum Entity {
    Account([u8; 32]),
    Contract([u8; 32]),
}

impl CLTyped for Entity {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

impl Entity {
    /// Get the tag of the entity.
    #[must_use]
    pub fn tag(&self) -> u32 {
        match self {
            Entity::Account(_) => 0,
            Entity::Contract(_) => 1,
        }
    }

    #[must_use]
    pub fn from_parts(tag: u32, address: [u8; 32]) -> Option<Self> {
        match tag {
            0 => Some(Self::Account(address)),
            1 => Some(Self::Contract(address)),
            _ => None,
        }
    }

    #[must_use]
    pub fn address(&self) -> &Address {
        match self {
            Entity::Account(addr) | Entity::Contract(addr) => addr,
        }
    }

    #[must_use]
    pub fn is_account(&self) -> bool {
        match self {
            Entity::Account(_) => true,
            Entity::Contract(_) => false,
        }
    }

    #[must_use]
    pub fn is_contract(&self) -> bool {
        match self {
            Entity::Account(_) => false,
            Entity::Contract(_) => true,
        }
    }

    #[must_use]
    pub fn entity_addr(&self) -> EntityAddr {
        match self {
            Self::Contract(addr) => EntityAddr::SmartContract(*addr),
            Self::Account(addr) => EntityAddr::Account(*addr),
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "std"))]
impl CasperABI for Entity {
    fn visit(visitor: &mut dyn crate::abi::ABIVisitor) {
        visitor.accept(ABITypeInfo::from_abi_type::<Self>());
        <[u8; 32]>::visit(visitor);
    }
    fn declaration() -> crate::abi::AbiDeclaration {
        "Entity".into()
    }

    fn definition() -> crate::abi::Definition {
        crate::abi::Definition::Enum {
            items: vec![
                EnumVariant {
                    name: "Account".into(),
                    discriminant: 0,
                    decl: Some(casper_executor_wasm_common::type_uid::of::<[u8; 32]>().into()),
                },
                EnumVariant {
                    name: "Contract".into(),
                    discriminant: 1,
                    decl: Some(casper_executor_wasm_common::type_uid::of::<[u8; 32]>().into()),
                },
            ],
        }
    }
}

/// Get the balance of an account or contract.
#[must_use]
pub fn get_balance_of(entity_kind: &Entity) -> u64 {
    let (kind, addr) = match entity_kind {
        Entity::Account(addr) => (0, addr),
        Entity::Contract(addr) => (1, addr),
    };

    let input_data = borsh::to_vec(&(kind, addr)).expect("Expected borsh to work");
    let (output_data, res) = casper_ffi(GlobalStateFunctionOption::GetBalance.into(), &input_data);
    if res == 0 {
        output_data
            .map(|bytes| match borsh::from_slice::<[u8; 8]>(&bytes) {
                Ok(bytes) => u64::from_le_bytes(bytes),
                Err(_) => panic!("Unexpected format of balance from host"),
            })
            .unwrap_or_default()
    } else {
        0
    }
}

/// Get the transferred token value passed to the contract.
#[must_use]
pub fn transferred_value() -> u64 {
    let info = get_env_info().expect("expected get_env_info to yield data");
    info.transferred_value
}

/// Transfer tokens from the current contract to another account or contract.
pub fn transfer(target: &EntityAddr, amount: u64) -> Result<(), CallError> {
    log!("transfer entity_addr {:?}", target);
    let bytes = match borsh::to_vec(&(target, amount)) {
        Ok(bytes) => bytes,
        Err(_err) => return Err(CallError::CalleeTrapped),
    };
    let opt = SystemContractOption::Transfer.into();
    let (_ret, result_code) = casper_ffi(opt, &bytes);
    call_result_from_code(result_code)
}

/// Get the current block time.
#[inline]
pub fn get_block_time() -> u64 {
    let info = get_env_info().expect("expected get_env_info to yield data");
    info.block_time
}

#[derive(PartialEq, Debug)]
pub enum GenericHashError {
    HostResult(HostResult),
    NoData,
    UnparseableHostOutput,
}

#[inline]
pub fn generic_hash(data: &[u8], algorithm: HashAlgorithm) -> Result<[u8; 32], GenericHashError> {
    let mut input_data = Vec::new();
    input_data.extend_from_slice(&(algorithm as u32).to_le_bytes());
    input_data.extend_from_slice(&(data.len() as u32).to_le_bytes());
    input_data.extend_from_slice(data);

    let (output_data, result_code) =
        casper_ffi(CryptoFunctionOption::GenericHash.into(), &input_data);

    match result_from_code(result_code) {
        Ok(_) => {
            let output_data = output_data.ok_or(GenericHashError::NoData)?;
            borsh::from_slice(&output_data).map_err(|_| GenericHashError::UnparseableHostOutput)
        }
        Err(err) => Err(GenericHashError::HostResult(err)),
    }
}

#[cfg_attr(test, derive(PartialEq))]
#[derive(Debug)]
pub enum RecoverSecp256K1Error {
    HostResult(HostResult),
    NoData,
    UnparseableHostOutput,
}

#[inline]
pub fn recover_secp256k1(
    message: &[u8],
    signature: &[u8],
    recovery_id: u32,
) -> Result<PublicKey, RecoverSecp256K1Error> {
    let input_data =
        borsh::to_vec(&(recovery_id, message, signature)).expect("Expected borsh to work");

    let (output_data, result_code) =
        casper_ffi(CryptoFunctionOption::RecoverSecp256K1.into(), &input_data);
    match result_from_code(result_code) {
        Ok(_) => {
            let output_data = output_data.ok_or(RecoverSecp256K1Error::NoData)?;
            let bytes = borsh::from_slice::<[u8; 34]>(&output_data)
                .map_err(|_| RecoverSecp256K1Error::UnparseableHostOutput)?;
            let secp_bytes = bytes[1..].try_into().unwrap();
            Ok(PublicKey::Secp256k1(secp_bytes))
        }
        Err(err) => Err(RecoverSecp256K1Error::HostResult(err)),
    }
}

#[doc(hidden)]
pub fn emit(topic: &str, payload: &[u8]) -> Result<(), HostResult> {
    let input_data = borsh::to_vec(&(topic, payload)).expect("Expected borsh to work");
    let (_output_data, result_code) = casper_ffi(EmitFunctionOption::Native.into(), &input_data);
    result_from_code(result_code)
}

/// Emit a message.
pub fn emit_message<M>(message: M) -> Result<(), HostResult>
where
    M: Message,
{
    let topic = M::TOPIC;
    let payload = message.payload();
    emit(topic, &payload)
}
