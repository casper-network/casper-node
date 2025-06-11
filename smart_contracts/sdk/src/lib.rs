#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
#[macro_use]
extern crate alloc;

pub mod abi;
pub mod prelude;
pub mod serializers;
#[cfg(not(target_arch = "wasm32"))]
pub use linkme;

#[cfg(not(target_arch = "wasm32"))]
pub mod abi_generator;
pub mod casper;
pub mod collections;
pub mod contrib;
#[cfg(feature = "std")]
pub mod schema;
pub mod types;

use crate::prelude::{marker::PhantomData, ptr::NonNull};

use crate::serializers::borsh::{BorshDeserialize, BorshSerialize};
use casper::{CallResult, Entity};
pub use casper_contract_macros as macros;
pub use casper_contract_sdk_sys as sys;
pub use casper_executor_wasm_common;
use types::{Address, CallError};

cfg_if::cfg_if! {
    if #[cfg(feature = "std")] {
        #[inline]
        pub fn set_panic_hook() {
            static SET_HOOK: std::sync::Once = std::sync::Once::new();
            SET_HOOK.call_once(|| {
                std::panic::set_hook(Box::new(|panic_info| {
                    let msg = panic_info.to_string();
                    casper::print(&msg);
                }));
            });
        }
    }
    else {
        pub fn set_panic_hook() {
            // TODO: What to do?
        }
    }
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
    type Return<'a>;

    fn entry_point(&self) -> &str;

    fn input_data(&self) -> Option<crate::prelude::Vec<u8>>;
}

/// To derive this contract you have to use `#[casper]` macro on top of impl block.
///
/// This proc macro handles generation of a manifest.
pub trait Contract {
    type Ref: ContractRef;

    fn name() -> &'static str;
    fn create<T: ToCallData>(
        value: u64,
        call_data: T,
    ) -> Result<ContractHandle<Self::Ref>, CallError>;
    fn default_create() -> Result<ContractHandle<Self::Ref>, CallError>;
    fn upgrade<T: ToCallData>(code: Option<&[u8]>, call_data: T) -> Result<(), CallError>;
}

#[derive(Debug)]
pub enum Access {
    Private,
    Public,
}

// A println! like macro that calls `host::print` function.
#[cfg(target_arch = "wasm32")]
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => ({
        $crate::prelude::casper::print(&$crate::prelude::format!($($arg)*));
    })
}

#[cfg(not(target_arch = "wasm32"))]
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => ({
        eprintln!("📝 {}", &$crate::prelude::format!($($arg)*));
    })
}

#[macro_export]
macro_rules! revert {
    () => {{
        $crate::casper::ret(
            $crate::casper_executor_wasm_common::flags::ReturnFlags::REVERT,
            None,
        );
        unreachable!()
    }};
    ($arg:expr) => {{
        let value = $arg;
        let data =
            $crate::serializers::borsh::to_vec(&value).expect("Revert value should serialize");
        $crate::casper::ret(
            $crate::casper_executor_wasm_common::flags::ReturnFlags::REVERT,
            Some(data.as_slice()),
        );
        #[allow(unreachable_code)]
        value
    }};
}

pub trait UnwrapOrRevert<T> {
    /// Unwraps the value into its inner type or calls [`crate::casper::ret`] with a
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
            casper::ret(
                casper_executor_wasm_common::flags::ReturnFlags::REVERT,
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
    #[must_use]
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
            transferred_value: None,
        }
    }

    /// A shorthand form to call contracts with default settings.
    #[inline]
    pub fn call<'a, CallData: ToCallData>(
        &self,
        func: impl FnOnce(T) -> CallData,
    ) -> Result<CallData::Return<'a>, CallError>
    where
        CallData::Return<'a>: BorshDeserialize,
    {
        self.build_call().call(func)
    }

    /// A shorthand form to call contracts with default settings.
    #[inline]
    pub fn try_call<CallData: ToCallData>(
        &self,
        func: impl FnOnce(T) -> CallData,
    ) -> Result<CallResult<CallData>, CallError> {
        self.build_call().try_call(func)
    }

    #[must_use]
    pub fn contract_address(&self) -> Address {
        self.contract_address
    }

    #[must_use]
    pub fn entity(&self) -> Entity {
        Entity::Contract(self.contract_address)
    }

    /// Returns the balance of the contract.
    #[must_use]
    pub fn balance(&self) -> u64 {
        casper::get_balance_of(&Entity::Contract(self.contract_address))
    }
}

pub struct CallBuilder<T: ContractRef> {
    address: Address,
    transferred_value: Option<u64>,
    marker: PhantomData<T>,
}

impl<T: ContractRef> CallBuilder<T> {
    #[must_use]
    pub fn new(address: Address) -> Self {
        CallBuilder {
            address,
            transferred_value: None,
            marker: PhantomData,
        }
    }

    #[must_use]
    pub fn with_transferred_value(mut self, transferred_value: u64) -> Self {
        self.transferred_value = Some(transferred_value);
        self
    }

    /// Casts the call builder to a different contract reference.
    #[must_use]
    pub fn cast<U: ContractRef>(self) -> CallBuilder<U> {
        CallBuilder {
            address: self.address,
            transferred_value: self.transferred_value,
            marker: PhantomData,
        }
    }

    pub fn try_call<CallData: ToCallData>(
        &self,
        func: impl FnOnce(T) -> CallData,
    ) -> Result<CallResult<CallData>, CallError> {
        let inst = T::new();
        let call_data = func(inst);
        casper::call(
            &self.address,
            self.transferred_value.unwrap_or(0),
            call_data,
        )
    }

    pub fn call<'a, CallData: ToCallData>(
        &self,
        func: impl FnOnce(T) -> CallData,
    ) -> Result<CallData::Return<'a>, CallError>
    where
        CallData::Return<'a>: BorshDeserialize,
    {
        let inst = T::new();
        let call_data = func(inst);
        let call_result = casper::call(
            &self.address,
            self.transferred_value.unwrap_or(0),
            call_data,
        )?;
        call_result.into_result()
    }
}

pub struct ContractBuilder<'a, T: ContractRef> {
    transferred_value: Option<u64>,
    code: Option<&'a [u8]>,
    seed: Option<&'a [u8; 32]>,
    marker: PhantomData<T>,
}

impl<T: ContractRef> Default for ContractBuilder<'_, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, T: ContractRef> ContractBuilder<'a, T> {
    #[must_use]
    pub fn new() -> Self {
        ContractBuilder {
            transferred_value: None,
            code: None,
            seed: None,
            marker: PhantomData,
        }
    }

    #[must_use]
    pub fn with_transferred_value(mut self, transferred_value: u64) -> Self {
        self.transferred_value = Some(transferred_value);
        self
    }

    #[must_use]
    pub fn with_code(mut self, code: &'a [u8]) -> Self {
        self.code = Some(code);
        self
    }

    #[must_use]
    pub fn with_seed(mut self, seed: &'a [u8; 32]) -> Self {
        self.seed = Some(seed);
        self
    }

    pub fn create<CallData: ToCallData>(
        &self,
        func: impl FnOnce() -> CallData,
    ) -> Result<ContractHandle<T>, CallError>
    where
        CallData::Return<'a>: BorshDeserialize,
    {
        let value = self.transferred_value.unwrap_or(0);
        let call_data = func();
        let input_data = call_data.input_data();
        let seed = self.seed;
        let create_result = casper::create(
            self.code,
            value,
            Some(call_data.entry_point()),
            input_data.as_deref(),
            seed,
        )?;
        Ok(ContractHandle::from_address(create_result.contract_address))
    }

    pub fn default_create(&self) -> Result<ContractHandle<T>, CallError> {
        if self.transferred_value.is_some() {
            panic!("Value should not be set for default create");
        }

        let value = self.transferred_value.unwrap_or(0);
        let seed = self.seed;
        let create_result = casper::create(self.code, value, None, None, seed)?;
        Ok(ContractHandle::from_address(create_result.contract_address))
    }
}

/// Trait for converting a message data to a string.
pub trait Message: BorshSerialize {
    const TOPIC: &'static str;
    /// Converts the message data to a string.
    fn payload(&self) -> Vec<u8>;
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_call_builder() {}
}
