#[cfg(all(not(target_arch = "wasm32"), feature = "std"))]
pub mod native;
use crate::prelude::Vec;
use crate::prelude::{
    ffi::c_void,
    marker::PhantomData,
    mem::MaybeUninit,
    ptr::{self, NonNull},
};
use crate::serializers::borsh::{BorshDeserialize, BorshSerialize};

use casper_sdk_sys::casper_env_caller;
use vm_common::{
    flags::ReturnFlags,
    keyspace::{Keyspace, KeyspaceTag},
};

use crate::{
    abi::{CasperABI, EnumVariant},
    reserve_vec_space,
    types::{Address, CallError, Entry},
    ToCallData,
};

#[derive(Debug)]
pub enum Error {
    Foo,
    Bar,
}

pub fn casper_print(msg: &str) {
    unsafe { casper_sdk_sys::casper_print(msg.as_ptr(), msg.len()) };
}

pub enum Alloc<F: FnOnce(usize) -> Option<ptr::NonNull<u8>>> {
    Callback(F),
    Static(ptr::NonNull<u8>),
}

pub fn casper_env_read<F: FnOnce(usize) -> Option<ptr::NonNull<u8>>>(
    env_path: &[u64],
    func: F,
) -> Option<NonNull<u8>> {
    let ret = unsafe {
        casper_sdk_sys::casper_env_read(
            env_path.as_ptr(),
            env_path.len(),
            Some(alloc_callback::<F>),
            &func as *const _ as *mut c_void,
        )
    };

    NonNull::<u8>::new(ret)
}

pub fn casper_env_read_into(env_path: &[u64], dest: &mut [u8]) -> Option<NonNull<u8>> {
    let ret = unsafe {
        casper_sdk_sys::casper_env_read(
            env_path.as_ptr(),
            env_path.len(),
            None,
            dest.as_mut_ptr() as *mut c_void,
        )
    };

    NonNull::<u8>::new(ret)
}

extern "C" fn alloc_callback<F: FnOnce(usize) -> Option<ptr::NonNull<u8>>>(
    len: usize,
    ctx: *mut c_void,
) -> *mut u8 {
    // dbg!(&len);
    let opt_closure = ctx as *mut Option<F>;
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
        casper_sdk_sys::casper_copy_input(alloc_callback::<F>, &alloc as *const _ as *mut c_void)
    };
    NonNull::<u8>::new(ret)
}

pub fn casper_copy_input() -> Vec<u8> {
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

pub fn copy_input_dest(dest: &mut [u8]) -> Option<&[u8]> {
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

pub fn casper_return(flags: ReturnFlags, data: Option<&[u8]>) {
    let (data_ptr, data_len) = match data {
        Some(data) => (data.as_ptr(), data.len()),
        None => (ptr::null(), 0),
    };
    unsafe { casper_sdk_sys::casper_return(flags.bits(), data_ptr, data_len) };
    #[cfg(target_arch = "wasm32")]
    unreachable!()
}

pub fn casper_read<F: FnOnce(usize) -> Option<ptr::NonNull<u8>>>(
    key: Keyspace,
    f: F,
) -> Result<Option<Entry>, Error> {
    let (key_space, key_bytes) = match key {
        Keyspace::State => (KeyspaceTag::State as u64, &[][..]),
        Keyspace::Context(key_bytes) => (KeyspaceTag::Context as u64, key_bytes),
        Keyspace::NamedKey(key_bytes) => (KeyspaceTag::NamedKey as u64, key_bytes.as_bytes()),
    };

    let mut info = casper_sdk_sys::ReadInfo {
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
        casper_sdk_sys::casper_read(
            key_space,
            key_bytes.as_ptr(),
            key_bytes.len(),
            &mut info as *mut casper_sdk_sys::ReadInfo,
            alloc_cb::<F>,
            ctx,
        )
    };

    if ret == 0 {
        Ok(Some(Entry(())))
    } else if ret == 1 {
        Ok(None)
    } else {
        Err(Error::Foo)
    }
}

pub fn casper_write(key: Keyspace, value: &[u8]) -> Result<(), Error> {
    let (key_space, key_bytes) = match key {
        Keyspace::State => (KeyspaceTag::State as u64, &[][..]),
        Keyspace::Context(key_bytes) => (KeyspaceTag::Context as u64, key_bytes),
        Keyspace::NamedKey(key_bytes) => (KeyspaceTag::NamedKey as u64, key_bytes.as_bytes()),
    };
    let _ret = unsafe {
        casper_sdk_sys::casper_write(
            key_space,
            key_bytes.as_ptr(),
            key_bytes.len(),
            value.as_ptr(),
            value.len(),
        )
    };
    Ok(())
}

pub fn casper_create(
    code: Option<&[u8]>,
    value: u64,
    constructor: Option<&str>,
    input_data: Option<&[u8]>,
) -> Result<casper_sdk_sys::CreateResult, CallError> {
    let (code_ptr, code_size): (*const u8, usize) = match code {
        Some(code) => (code.as_ptr(), code.len()),
        None => (ptr::null(), 0),
    };

    let mut result = MaybeUninit::uninit();

    let call_error = unsafe {
        casper_sdk_sys::casper_create(
            code_ptr,
            code_size,
            value,
            constructor.map(|s| s.as_ptr()).unwrap_or(ptr::null()),
            constructor.map(|s| s.len()).unwrap_or(0),
            input_data.map(|s| s.as_ptr()).unwrap_or(ptr::null()),
            input_data.map(|s| s.len()).unwrap_or(0),
            result.as_mut_ptr(),
        )
    };

    if call_error == 0 {
        let result = unsafe { result.assume_init() };
        Ok(result.into())
    } else {
        Err(CallError::try_from(call_error).expect("Unexpected error code"))
    }
}

pub(crate) fn call_into<F: FnOnce(usize) -> Option<ptr::NonNull<u8>>>(
    address: &Address,
    value: u64,
    entry_point: &str,
    input_data: &[u8],
    alloc: Option<F>,
) -> Result<(), CallError> {
    let result_code = unsafe {
        casper_sdk_sys::casper_call(
            address.as_ptr(),
            address.len(),
            value,
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
    if result_code == 0 {
        Ok(())
    } else {
        Err(CallError::try_from(result_code).expect("Unexpected error code"))
    }
}

pub fn casper_call(
    address: &Address,
    value: u64,
    entry_point: &str,
    input_data: &[u8],
) -> (Option<Vec<u8>>, Result<(), CallError>) {
    let mut output = None;
    let result_code = call_into(
        address,
        value,
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

pub fn casper_upgrade(
    code: &[u8],
    entry_point: Option<&str>,
    input_data: Option<&[u8]>,
) -> Result<(), CallError> {
    let code_ptr = code.as_ptr();
    let code_size = code.len();
    let entry_point_ptr = entry_point.map(|s| s.as_ptr()).unwrap_or(ptr::null());
    let entry_point_size = entry_point.map(|s| s.len()).unwrap_or(0);
    let input_ptr = input_data.map(|s| s.as_ptr()).unwrap_or(ptr::null());
    let input_size = input_data.map(|s| s.len()).unwrap_or(0);

    let result_code = unsafe {
        casper_sdk_sys::casper_upgrade(
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

pub fn read_vec(key: Keyspace) -> Option<Vec<u8>> {
    let mut vec = Vec::new();
    let out = casper_read(key, |size| reserve_vec_space(&mut vec, size)).unwrap();
    match out {
        Some(_input) => Some(vec),
        None => None,
    }
}

pub fn has_state() -> Result<bool, Error> {
    // TODO: Host side optimized `casper_exists` to check if given entry exists in the global state.
    let mut vec = Vec::new();
    let read_info = casper_read(Keyspace::State, |size| reserve_vec_space(&mut vec, size))?;
    match read_info {
        Some(_input) => Ok(true),
        None => Ok(false),
    }
}

pub fn read_state<T: Default + BorshDeserialize>() -> Result<T, Error> {
    let mut vec = Vec::new();
    let read_info = casper_read(Keyspace::State, |size| reserve_vec_space(&mut vec, size))?;
    match read_info {
        Some(_input) => Ok(borsh::from_slice(&vec).unwrap()),
        None => Ok(T::default()),
    }
}

pub fn write_state<T: BorshSerialize>(state: &T) -> Result<(), Error> {
    let new_state = borsh::to_vec(state).unwrap();
    casper_write(Keyspace::State, &new_state)?;
    Ok(())
}

/// TODO: Remove once procedural macros are improved, this is just to save the boilerplate when
/// doing things manually.
pub fn start<Args: BorshDeserialize, Ret: BorshSerialize>(mut func: impl FnMut(Args) -> Ret) {
    let input = casper_copy_input();
    let args: Args = BorshDeserialize::try_from_slice(&input).unwrap();
    let result = func(args);
    let serialized_result = borsh::to_vec(&result).unwrap();
    casper_return(ReturnFlags::empty(), Some(serialized_result.as_slice()));
}

pub fn start_noret<Args: BorshDeserialize, Ret: BorshSerialize>(
    func: impl FnOnce(Args) -> Ret,
) -> Ret {
    let input = casper_copy_input();
    let args: Args = BorshDeserialize::try_from_slice(&input).unwrap();
    func(args)
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

pub fn call<T: ToCallData>(
    contract_address: &Address,
    value: u64,
    call_data: T,
) -> Result<CallResult<T>, CallError> {
    let input_data = call_data.input_data().unwrap_or_default();

    let (maybe_data, result_code) = casper_call(
        contract_address,
        value,
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

pub fn get_caller() -> Entity {
    let mut addr = MaybeUninit::<Address>::uninit();
    let _dest = unsafe { NonNull::new_unchecked(addr.as_mut_ptr() as *mut u8) };

    let mut entity_kind = MaybeUninit::<u32>::uninit();

    // Pointer to the end of written bytes
    let _out_ptr =
        unsafe { casper_env_caller(addr.as_mut_ptr() as *mut _, 32, entity_kind.as_mut_ptr()) };

    // let address = unsafe { addr.assume_init() };
    let entity_kind = unsafe { entity_kind.assume_init() };

    match entity_kind {
        0 => Entity::Account(unsafe { addr.assume_init() }),
        1 => Entity::Contract(unsafe { addr.assume_init() }),
        _ => panic!("Unknown entity kind"),
    }
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
    pub(crate) fn tag(&self) -> u32 {
        match self {
            Entity::Account(_) => 0,
            Entity::Contract(_) => 1,
        }
    }

    /// Get the address of the entity.
    pub(crate) fn as_ptr(&self) -> *const u8 {
        match self {
            Entity::Account(addr) => addr.as_ptr(),
            Entity::Contract(addr) => addr.as_ptr(),
        }
    }

    /// Get the length of the address of the entity.
    pub(crate) fn len(&self) -> usize {
        match self {
            Entity::Account(addr) => addr.len(),
            Entity::Contract(addr) => addr.len(),
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
pub fn get_balance_of(entity_kind: &Entity) -> u64 {
    let (kind, addr) = match entity_kind {
        Entity::Account(addr) => (0, addr),
        Entity::Contract(addr) => (1, addr),
    };
    unsafe { casper_sdk_sys::casper_env_balance(kind, addr.as_ptr(), addr.len()) }
}

/// Get the value passed to the contract.
pub fn get_value() -> u64 {
    unsafe { casper_sdk_sys::casper_env_value() }
}

/// Transfer tokens from the current contract to another account or contract.
pub fn casper_transfer(target: &Entity, amount: u64) -> Result<(), CallError> {
    let result_code = unsafe {
        casper_sdk_sys::casper_transfer(target.tag(), target.as_ptr(), target.len(), amount)
    };
    call_result_from_code(result_code)
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use bytes::Bytes;
    use vm_common::flags::ReturnFlags;

    use crate::host::native::NativeTrap;

    use super::{
        native::{dispatch_with, Environment},
        start,
    };

    #[test]
    fn foo() {
        let dispatch_result =
            dispatch_with(Environment::default(), || start(|_arg: ()| {})).unwrap_err();
        assert_eq!(
            dispatch_result,
            NativeTrap::Return(ReturnFlags::empty(), Bytes::new())
        );
    }
}
