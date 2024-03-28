// #![feature(wasm_import_memory)]

// #[linkage = "--import-memory"]

pub mod abi;
#[cfg(feature = "cli")]
pub mod cli;
pub mod collections;
pub mod host;
pub mod schema;
pub mod types;

use std::{io, marker::PhantomData, ptr::NonNull};

use borsh::{BorshDeserialize, BorshSerialize};
pub use casper_sdk_sys as sys;
use host::CallResult;
use types::{Address, CallError};

#[cfg(not(target_arch = "wasm32"))]
pub use ctor::ctor;

#[cfg(target_arch = "wasm32")]
fn hook_impl(info: &std::panic::PanicInfo) {
    let msg = info.to_string();
    host::casper_print(&msg);
}

#[cfg(target_arch = "wasm32")]
#[inline]
pub fn set_panic_hook() {
    use std::sync::Once;
    static SET_HOOK: Once = Once::new();
    SET_HOOK.call_once(|| {
        std::panic::set_hook(Box::new(hook_impl));
    });
}

pub fn reserve_vec_space(vec: &mut Vec<u8>, size: usize) -> Option<NonNull<u8>> {
    if size == 0 {
        None
    } else {
        *vec = Vec::with_capacity(size);
        unsafe {
            vec.set_len(size);
        }
        NonNull::new(vec.as_mut_ptr())
    }
}

pub trait ContractRef {
    fn new() -> Self;
}

pub trait ToCallData {
    const SELECTOR: Selector;
    type Return<'a>;

    fn input_data(&self) -> Option<Vec<u8>>;
}

/// To derive this contract you have to use `#[casper]` macro on top of impl block.
///
/// This proc macro handles generation of a manifest.
pub trait Contract {
    type Ref: ContractRef;

    fn name() -> &'static str;
    fn create<T: ToCallData>(call_data: T) -> Result<ContractHandle<Self::Ref>, CallError>;
    fn default_create() -> Result<ContractHandle<Self::Ref>, CallError>;
}

#[derive(Debug)]
pub enum Access {
    Private,
    Public,
}

#[derive(Debug)]
pub enum ApiError {
    Error1,
    Error2,
    MissingArgument,
    Io(io::Error),
}

// A println! like macro that calls `host::print` function.
#[cfg(target_arch = "wasm32")]
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => ({
        $crate::host::casper_print(&format!($($arg)*));
    })
}

#[cfg(not(target_arch = "wasm32"))]
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => ({
        eprintln!("📝 {}", format!($($arg)*));
    })
}

#[macro_export]
macro_rules! revert {
    () => {{
        casper_sdk::host::casper_return(vm_common::flags::ReturnFlags::REVERT, None);
        unreachable!()
    }};
    ($arg:expr) => {{
        let value = $arg;
        let data = borsh::to_vec(&value).expect("Revert value should serialize");
        casper_sdk::host::casper_return(
            vm_common::flags::ReturnFlags::REVERT,
            Some(data.as_slice()),
        );
        #[allow(unreachable_code)]
        value
    }};
}

pub trait UnwrapOrRevert<T> {
    /// Unwraps the value into its inner type or calls [`runtime::revert`] with a
    /// predetermined error code on failure.
    fn unwrap_or_revert(self) -> T;
}

impl<T, E> UnwrapOrRevert<T> for Result<T, E>
where
    E: BorshSerialize,
{
    fn unwrap_or_revert(self) -> T {
        self.unwrap_or_else(|error| {
            let error_data = borsh::to_vec(&error).expect("Revert value should serialize");
            host::casper_return(
                vm_common::flags::ReturnFlags::REVERT,
                Some(error_data.as_slice()),
            );
            unreachable!("Support for unwrap_or_revert")
        })
    }
}

#[derive(Debug)]
pub struct ContractHandle<T: ContractRef> {
    contract_address: Address,
    marker: PhantomData<T>,
}

impl<T: ContractRef> ContractHandle<T> {
    pub const fn from_address(contract_address: Address) -> Self {
        ContractHandle {
            contract_address,
            marker: PhantomData,
        }
    }

    pub fn build_call(&self) -> CallBuilder<T> {
        CallBuilder {
            address: self.contract_address,
            marker: PhantomData,
            value: None,
        }
    }

    /// A shorthand form to call contracts with default settings.
    #[inline]
    pub fn call<'a, CallData: ToCallData>(
        &self,
        func: impl FnOnce(T) -> CallData,
    ) -> Result<CallData::Return<'a>, CallError>
    where
        CallData::Return<'a>: BorshDeserialize + Clone,
    {
        self.build_call().call(func)
    }

    /// A shorthand form to call contracts with default settings.
    #[inline]
    pub fn try_call<'a, CallData: ToCallData>(
        &self,
        func: impl FnOnce(T) -> CallData,
    ) -> Result<CallResult<CallData>, CallError> {
        self.build_call().try_call(func)
    }

    pub fn contract_address(&self) -> Address {
        self.contract_address
    }
}

pub struct CallBuilder<T: ContractRef> {
    address: Address,
    value: Option<u64>,
    marker: PhantomData<T>,
}

impl<T: ContractRef> CallBuilder<T> {
    pub fn new(address: Address) -> Self {
        CallBuilder {
            address,
            value: None,
            marker: PhantomData,
        }
    }

    pub fn with_value(mut self, value: u64) -> Self {
        self.value = Some(value);
        self
    }

    /// Casts the call builder to a different contract reference.
    pub fn cast<U: ContractRef>(self) -> CallBuilder<U> {
        CallBuilder {
            address: self.address,
            value: self.value,
            marker: PhantomData,
        }
    }

    pub fn try_call<'a, CallData: ToCallData>(
        &self,
        func: impl FnOnce(T) -> CallData,
    ) -> Result<CallResult<CallData>, CallError> {
        let inst = T::new();
        let call_data = func(inst);
        host::call(&self.address, self.value.unwrap_or(0), call_data)
    }

    pub fn call<'a, CallData: ToCallData>(
        &self,
        func: impl FnOnce(T) -> CallData,
    ) -> Result<CallData::Return<'a>, CallError>
    where
        CallData::Return<'a>: BorshDeserialize + Clone,
    {
        let inst = T::new();
        let call_data = func(inst);
        let call_result = host::call(&self.address, self.value.unwrap_or(0), call_data)?;
        Ok(call_result.into_return_value())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code)]
    struct MyContract;

    #[derive(BorshSerialize)]
    struct DoSomethingArg {
        foo: u64,
    }

    impl ToCallData for DoSomethingArg {
        const SELECTOR: Selector = Selector(1);
        type Return<'a> = ();
        fn input_data(&self) -> Option<Vec<u8>> {
            Some(borsh::to_vec(self).expect("Serialization should work"))
        }
    }

    impl MyContract {
        #[allow(dead_code)]
        fn do_something(&mut self, foo: u64) -> impl ToCallData {
            DoSomethingArg { foo }
        }
    }

    #[test]
    fn test_call_builder() {
        // let contract = MyContract;
        // let do_something = CallBuilder::<MyContract>::new([0;
        // 32]).with_value(5).call(|my_contract| my_contract.do_something(43));
    }
}
