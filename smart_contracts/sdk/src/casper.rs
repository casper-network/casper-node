#[cfg(all(not(target_arch = "wasm32"), feature = "std"))]
pub mod native;

use crate::{
    abi::{CasperABI, EnumVariant},
    prelude::{
        ffi::c_void,
        marker::PhantomData,
        mem::MaybeUninit,
        ptr::{self, NonNull},
    },
    reserve_vec_space,
    serializers::borsh::{BorshDeserialize, BorshSerialize},
    types::{Address, CallError},
    Message, ToCallData,
};

use casper_contract_sdk_sys::casper_env_info;
use casper_executor_wasm_common::{
    env_info::EnvInfo,
    error::{result_from_code, CommonResult, HOST_ERROR_SUCCESS},
    flags::ReturnFlags,
    keyspace::{Keyspace, KeyspaceTag},
};

/// Print a message.
#[inline]
pub fn print(msg: &str) {
    unsafe { casper_contract_sdk_sys::casper_print(msg.as_ptr(), msg.len()) };
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

/// Provided callback should ensure that it can provide a pointer that can store `size` bytes.
/// Function returns last pointer after writing data, or None otherwise.
pub fn copy_input_into<F: FnOnce(usize) -> Option<ptr::NonNull<u8>>>(
    alloc: Option<F>,
) -> Option<NonNull<u8>> {
    let ret = unsafe {
        casper_contract_sdk_sys::casper_copy_input(
            alloc_callback::<F>,
            &alloc as *const _ as *mut c_void,
        )
    };
    NonNull::<u8>::new(ret)
}

/// Copy input data into a vector.
pub fn copy_input() -> Vec<u8> {
    let mut vec = Vec::new();
    let last_ptr = copy_input_into(Some(|size| reserve_vec_space(&mut vec, size)));
    match last_ptr {
        Some(_last_ptr) => vec,
        None => {
            // TODO: size of input was 0, we could properly deal with this case by not calling alloc
            // cb if size==0
            Vec::new()
        }
    }
}

/// Provided callback should ensure that it can provide a pointer that can store `size` bytes.
pub fn copy_input_to(dest: &mut [u8]) -> Option<&[u8]> {
    let last_ptr = copy_input_into(Some(|size| {
        if size > dest.len() {
            None
        } else {
            // SAFETY: `dest` is guaranteed to be non-null and large enough to hold `size`
            // bytes.
            Some(unsafe { ptr::NonNull::new_unchecked(dest.as_mut_ptr()) })
        }
    }));

    let end_ptr = last_ptr?;
    let length = unsafe { end_ptr.as_ptr().offset_from(dest.as_mut_ptr()) };
    let length: usize = length.try_into().unwrap();
    Some(&dest[..length])
}

/// Return from the contract.
pub fn ret(flags: ReturnFlags, data: Option<&[u8]>) {
    let (data_ptr, data_len) = match data {
        Some(data) => (data.as_ptr(), data.len()),
        None => (ptr::null(), 0),
    };
    unsafe { casper_contract_sdk_sys::casper_return(flags.bits(), data_ptr, data_len) };
    #[cfg(target_arch = "wasm32")]
    unreachable!()
}

/// Read from the global state.
pub fn read<F: FnOnce(usize) -> Option<ptr::NonNull<u8>>>(
    key: Keyspace,
    f: F,
) -> Result<Option<()>, CommonResult> {
    let (key_space, key_bytes) = match key {
        Keyspace::State => (KeyspaceTag::State as u64, &[][..]),
        Keyspace::Context(key_bytes) => (KeyspaceTag::Context as u64, key_bytes),
        Keyspace::NamedKey(key_bytes) => (KeyspaceTag::NamedKey as u64, key_bytes.as_bytes()),
        Keyspace::PaymentInfo(payload) => (KeyspaceTag::PaymentInfo as u64, payload.as_bytes()),
    };

    let mut info = casper_contract_sdk_sys::ReadInfo {
        data: ptr::null(),
        size: 0,
    };

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
        casper_contract_sdk_sys::casper_read(
            key_space,
            key_bytes.as_ptr(),
            key_bytes.len(),
            &mut info as *mut casper_contract_sdk_sys::ReadInfo,
            alloc_cb::<F>,
            ctx,
        )
    };

    match result_from_code(ret) {
        Ok(()) => Ok(Some(())),
        Err(CommonResult::NotFound) => Ok(None),
        Err(err) => Err(err),
    }
}

/// Write to the global state.
pub fn write(key: Keyspace, value: &[u8]) -> Result<(), CommonResult> {
    let (key_space, key_bytes) = match key {
        Keyspace::State => (KeyspaceTag::State as u64, &[][..]),
        Keyspace::Context(key_bytes) => (KeyspaceTag::Context as u64, key_bytes),
        Keyspace::NamedKey(key_bytes) => (KeyspaceTag::NamedKey as u64, key_bytes.as_bytes()),
        Keyspace::PaymentInfo(payload) => (KeyspaceTag::PaymentInfo as u64, payload.as_bytes()),
    };
    let ret = unsafe {
        casper_contract_sdk_sys::casper_write(
            key_space,
            key_bytes.as_ptr(),
            key_bytes.len(),
            value.as_ptr(),
            value.len(),
        )
    };
    result_from_code(ret)
}

/// Remove from the global state.
pub fn remove(key: Keyspace) -> Result<(), CommonResult> {
    let (key_space, key_bytes) = match key {
        Keyspace::State => (KeyspaceTag::State as u64, &[][..]),
        Keyspace::Context(key_bytes) => (KeyspaceTag::Context as u64, key_bytes),
        Keyspace::NamedKey(key_bytes) => (KeyspaceTag::NamedKey as u64, key_bytes.as_bytes()),
        Keyspace::PaymentInfo(payload) => (KeyspaceTag::PaymentInfo as u64, payload.as_bytes()),
    };
    let ret = unsafe {
        casper_contract_sdk_sys::casper_remove(key_space, key_bytes.as_ptr(), key_bytes.len())
    };
    result_from_code(ret)
}

/// Create a new contract instance.
pub fn create(
    code: Option<&[u8]>,
    transferred_value: u64,
    constructor: Option<&str>,
    input_data: Option<&[u8]>,
    seed: Option<&[u8; 32]>,
) -> Result<casper_contract_sdk_sys::CreateResult, CallError> {
    let (code_ptr, code_size): (*const u8, usize) = match code {
        Some(code) => (code.as_ptr(), code.len()),
        None => (ptr::null(), 0),
    };

    let mut result = MaybeUninit::uninit();

    let call_error = unsafe {
        casper_contract_sdk_sys::casper_create(
            code_ptr,
            code_size,
            transferred_value,
            constructor.map(|s| s.as_ptr()).unwrap_or(ptr::null()),
            constructor.map(|s| s.len()).unwrap_or(0),
            input_data.map(|s| s.as_ptr()).unwrap_or(ptr::null()),
            input_data.map(|s| s.len()).unwrap_or(0),
            seed.map(|s| s.as_ptr()).unwrap_or(ptr::null()),
            seed.map(|s| s.len()).unwrap_or(0),
            result.as_mut_ptr(),
        )
    };

    if call_error == 0 {
        let result = unsafe { result.assume_init() };
        Ok(result)
    } else {
        Err(CallError::try_from(call_error).expect("Unexpected error code"))
    }
}

pub(crate) fn call_into<F: FnOnce(usize) -> Option<ptr::NonNull<u8>>>(
    address: &Address,
    transferred_value: u64,
    entry_point: &str,
    input_data: &[u8],
    alloc: Option<F>,
) -> Result<(), CallError> {
    let result_code = unsafe {
        casper_contract_sdk_sys::casper_call(
            address.as_ptr(),
            address.len(),
            transferred_value,
            entry_point.as_ptr(),
            entry_point.len(),
            input_data.as_ptr(),
            input_data.len(),
            alloc_callback::<F>,
            &alloc as *const _ as *mut _,
        )
    };
    call_result_from_code(result_code)
}

fn call_result_from_code(result_code: u32) -> Result<(), CallError> {
    if result_code == HOST_ERROR_SUCCESS {
        Ok(())
    } else {
        Err(CallError::try_from(result_code).expect("Unexpected error code"))
    }
}

/// Call a contract.
pub fn casper_call(
    address: &Address,
    transferred_value: u64,
    entry_point: &str,
    input_data: &[u8],
) -> (Option<Vec<u8>>, Result<(), CallError>) {
    let mut output = None;
    let result_code = call_into(
        address,
        transferred_value,
        entry_point,
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

/// Upgrade the contract.
pub fn upgrade(
    code: &[u8],
    entry_point: Option<&str>,
    input_data: Option<&[u8]>,
) -> Result<(), CallError> {
    let code_ptr = code.as_ptr();
    let code_size = code.len();
    let entry_point_ptr = entry_point.map(str::as_ptr).unwrap_or(ptr::null());
    let entry_point_size = entry_point.map(str::len).unwrap_or(0);
    let input_ptr = input_data.map(|s| s.as_ptr()).unwrap_or(ptr::null());
    let input_size = input_data.map(|s| s.len()).unwrap_or(0);

    let result_code = unsafe {
        casper_contract_sdk_sys::casper_upgrade(
            code_ptr,
            code_size,
            entry_point_ptr,
            entry_point_size,
            input_ptr,
            input_size,
        )
    };
    match call_result_from_code(result_code) {
        Ok(()) => Ok(()),
        Err(err) => Err(err),
    }
}

/// Read from the global state into a vector.
pub fn read_into_vec(key: Keyspace) -> Result<Option<Vec<u8>>, CommonResult> {
    let mut vec = Vec::new();
    let out = read(key, |size| reserve_vec_space(&mut vec, size))?.map(|()| vec);
    Ok(out)
}

/// Read from the global state into a vector.
pub fn has_state() -> Result<bool, CommonResult> {
    // TODO: Host side optimized `casper_exists` to check if given entry exists in the global state.
    let mut vec = Vec::new();
    let read_info = read(Keyspace::State, |size| reserve_vec_space(&mut vec, size))?;
    match read_info {
        Some(()) => Ok(true),
        None => Ok(false),
    }
}

/// Read state from the global state.
pub fn read_state<T: Default + BorshDeserialize>() -> Result<T, CommonResult> {
    let mut vec = Vec::new();
    let read_info = read(Keyspace::State, |size| reserve_vec_space(&mut vec, size))?;
    match read_info {
        Some(()) => Ok(borsh::from_slice(&vec).unwrap()),
        None => Ok(T::default()),
    }
}

/// Write state to the global state.
pub fn write_state<T: BorshSerialize>(state: &T) -> Result<(), CommonResult> {
    let new_state = borsh::to_vec(state).unwrap();
    write(Keyspace::State, &new_state)?;
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
            Ok(()) | Err(CallError::CalleeReverted) => {
                let data = self.data.unwrap_or_default();
                Ok(borsh::from_slice(&data).unwrap())
            }
            Err(call_error) => Err(call_error),
        }
    }

    pub fn did_revert(&self) -> bool {
        self.result == Err(CallError::CalleeReverted)
    }
}

/// Call a contract.
pub fn call<T: ToCallData>(
    contract_address: &Address,
    transferred_value: u64,
    call_data: T,
) -> Result<CallResult<T>, CallError> {
    let input_data = call_data.input_data().unwrap_or_default();

    let (maybe_data, result_code) = casper_call(
        contract_address,
        transferred_value,
        call_data.entry_point(),
        &input_data,
    );
    match result_code {
        Ok(()) | Err(CallError::CalleeReverted) => Ok(CallResult::<T> {
            data: maybe_data,
            result: result_code,
            marker: PhantomData,
        }),
        Err(error) => Err(error),
    }
}

/// Get the environment info.
pub fn get_env_info() -> EnvInfo {
    let ret = {
        let mut info = MaybeUninit::<EnvInfo>::uninit();

        let ret = unsafe { casper_env_info(info.as_mut_ptr().cast(), size_of::<EnvInfo>() as u32) };
        result_from_code(ret).map(|()| {
            // SAFETY: The size of `EnvInfo` is known and the pointer is valid.
            unsafe { info.assume_init() }
        })
    };

    match ret {
        Ok(info) => info,
        Err(err) => panic!("Failed to get environment info: {:?}", err),
    }
}

/// Get the caller.
#[must_use]
pub fn get_caller() -> Entity {
    let info = get_env_info();
    Entity::from_parts(info.caller_kind, info.caller_addr).expect("Invalid caller kind")
}

#[must_use]
pub fn get_callee() -> Entity {
    let info = get_env_info();
    Entity::from_parts(info.callee_kind, info.callee_addr).expect("Invalid callee kind")
}

/// Enum representing either an account or a contract.
#[derive(
    BorshSerialize, BorshDeserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
pub enum Entity {
    Account([u8; 32]),
    Contract([u8; 32]),
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
}

impl CasperABI for Entity {
    fn populate_definitions(definitions: &mut crate::abi::Definitions) {
        definitions.populate_one::<[u8; 32]>();
    }

    fn declaration() -> crate::abi::Declaration {
        "Entity".into()
    }

    fn definition() -> crate::abi::Definition {
        crate::abi::Definition::Enum {
            items: vec![
                EnumVariant {
                    name: "Account".into(),
                    discriminant: 0,
                    decl: <[u8; 32] as CasperABI>::declaration(),
                },
                EnumVariant {
                    name: "Contract".into(),
                    discriminant: 1,
                    decl: <[u8; 32] as CasperABI>::declaration(),
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
    let mut output: MaybeUninit<u64> = MaybeUninit::uninit();
    let ret = unsafe {
        casper_contract_sdk_sys::casper_env_balance(
            kind,
            addr.as_ptr(),
            addr.len(),
            output.as_mut_ptr().cast(),
        )
    };
    if ret == 1 {
        unsafe { output.assume_init() }
    } else {
        0
    }
}

/// Get the transferred token value passed to the contract.
#[must_use]
pub fn transferred_value() -> u64 {
    let info = get_env_info();
    info.transferred_value
}

/// Transfer tokens from the current contract to another account or contract.
pub fn transfer(target_account: &Address, amount: u64) -> Result<(), CallError> {
    let amount: *const c_void = &amount as *const _ as *const c_void;
    let result_code = unsafe {
        casper_contract_sdk_sys::casper_transfer(
            target_account.as_ptr(),
            target_account.len(),
            amount,
        )
    };
    call_result_from_code(result_code)
}

/// Get the current block time.
#[inline]
pub fn get_block_time() -> u64 {
    let info = get_env_info();
    info.block_time
}

#[doc(hidden)]
pub fn emit_raw(topic: &str, payload: &[u8]) -> Result<(), CommonResult> {
    let ret = unsafe {
        casper_contract_sdk_sys::casper_emit(
            topic.as_ptr(),
            topic.len(),
            payload.as_ptr(),
            payload.len(),
        )
    };
    result_from_code(ret)
}

/// Emit a message.
pub fn emit<M>(message: M) -> Result<(), CommonResult>
where
    M: Message,
{
    let topic = M::TOPIC;
    let payload = message.payload();
    emit_raw(topic, &payload)
}
