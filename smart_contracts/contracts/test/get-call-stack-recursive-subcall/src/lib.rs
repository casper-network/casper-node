#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};

use casper_contract::{
    contract_api::{account, runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    bytesrepr,
    bytesrepr::{Error, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    runtime_args, ApiError, CLType, CLTyped, ContractHash, ContractPackageHash, EntryPointType,
    Key, Phase, RuntimeArgs, Tagged, URef, U512,
};

pub const CONTRACT_PACKAGE_NAME: &str = "forwarder";
pub const PACKAGE_ACCESS_KEY_NAME: &str = "forwarder_access";
pub const CONTRACT_NAME: &str = "our_contract_name";

pub const METHOD_FORWARDER_CONTRACT_NAME: &str = "forwarder_contract";
pub const METHOD_FORWARDER_SESSION_NAME: &str = "forwarder_session";

pub const ARG_CALLS: &str = "calls";
pub const ARG_CURRENT_DEPTH: &str = "current_depth";

const DEFAULT_PAYMENT: u64 = 1_500_000_000_000;

#[repr(u8)]
enum ContractAddressTag {
    ContractHash = 0,
    ContractPackageHash,
}

#[derive(Debug, Copy, Clone)]
pub enum ContractAddress {
    ContractHash(ContractHash),
    ContractPackageHash(ContractPackageHash),
}

impl Tagged<u8> for ContractAddress {
    fn tag(&self) -> u8 {
        match self {
            ContractAddress::ContractHash(_) => ContractAddressTag::ContractHash as u8,
            ContractAddress::ContractPackageHash(_) => {
                ContractAddressTag::ContractPackageHash as u8
            }
        }
    }
}

impl ToBytes for ContractAddress {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.push(self.tag());
        match self {
            ContractAddress::ContractHash(contract_hash) => {
                result.append(&mut contract_hash.to_bytes()?)
            }
            ContractAddress::ContractPackageHash(contract_package_hash) => {
                result.append(&mut contract_package_hash.to_bytes()?)
            }
        }
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                ContractAddress::ContractHash(contract_hash) => contract_hash.serialized_length(),
                ContractAddress::ContractPackageHash(contract_package_hash) => {
                    contract_package_hash.serialized_length()
                }
            }
    }
}

impl FromBytes for ContractAddress {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (tag, remainder): (u8, &[u8]) = FromBytes::from_bytes(bytes)?;
        match tag {
            tag if tag == ContractAddressTag::ContractHash as u8 => {
                let (contract_hash, remainder) = ContractHash::from_bytes(remainder)?;
                Ok((ContractAddress::ContractHash(contract_hash), remainder))
            }
            tag if tag == ContractAddressTag::ContractPackageHash as u8 => {
                let (contract_package_hash, remainder) =
                    ContractPackageHash::from_bytes(remainder)?;
                Ok((
                    ContractAddress::ContractPackageHash(contract_package_hash),
                    remainder,
                ))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Call {
    pub contract_address: ContractAddress,
    pub target_method: String,
    pub entry_point_type: EntryPointType,
}

impl ToBytes for Call {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut result = bytesrepr::allocate_buffer(self)?;
        result.append(&mut self.contract_address.to_bytes()?);
        result.append(&mut self.target_method.to_bytes()?);
        result.append(&mut self.entry_point_type.to_bytes()?);
        Ok(result)
    }

    fn serialized_length(&self) -> usize {
        self.contract_address.serialized_length()
            + self.target_method.serialized_length()
            + self.entry_point_type.serialized_length()
    }
}

impl FromBytes for Call {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (contract_address, remainder) = ContractAddress::from_bytes(bytes)?;
        let (target_method, remainder) = String::from_bytes(remainder)?;
        let (entry_point_type, remainder) = EntryPointType::from_bytes(remainder)?;
        Ok((
            Call {
                contract_address,
                target_method,
                entry_point_type,
            },
            remainder,
        ))
    }
}

impl CLTyped for Call {
    fn cl_type() -> CLType {
        CLType::Any
    }
}

pub fn standard_payment(amount: U512) {
    const METHOD_GET_PAYMENT_PURSE: &str = "get_payment_purse";

    let main_purse = account::get_main_purse();

    let handle_payment_pointer = system::get_handle_payment();

    let payment_purse: URef = runtime::call_contract(
        handle_payment_pointer,
        METHOD_GET_PAYMENT_PURSE,
        RuntimeArgs::default(),
    );

    system::transfer_from_purse_to_purse(main_purse, payment_purse, amount, None).unwrap_or_revert()
}

pub fn recurse() {
    let calls: Vec<Call> = runtime::get_named_arg(ARG_CALLS);
    let current_depth: u8 = runtime::get_named_arg(ARG_CURRENT_DEPTH);

    // The important bit
    {
        let call_stack = runtime::get_call_stack();
        let name = alloc::format!("call_stack-{}", current_depth);
        let call_stack_at = storage::new_uref(call_stack);
        runtime::put_key(&name, Key::URef(call_stack_at));
    }

    if current_depth == 0 && runtime::get_phase() == Phase::Payment {
        standard_payment(U512::from(DEFAULT_PAYMENT))
    }

    if current_depth == calls.len() as u8 {
        return;
    }

    let args = runtime_args! {
        ARG_CALLS => calls.clone(),
        ARG_CURRENT_DEPTH => current_depth + 1u8,
    };

    match calls.get(current_depth as usize) {
        Some(Call {
            contract_address: ContractAddress::ContractPackageHash(contract_package_hash),
            target_method,
            ..
        }) => {
            runtime::call_versioned_contract::<()>(
                *contract_package_hash,
                None,
                target_method,
                args,
            );
        }
        Some(Call {
            contract_address: ContractAddress::ContractHash(contract_hash),
            target_method,
            ..
        }) => {
            runtime::call_contract::<()>(*contract_hash, target_method, args);
        }
        _ => runtime::revert(ApiError::User(0)),
    }
}
