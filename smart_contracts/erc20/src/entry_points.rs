//! Contains definition of the entry points.
use alloc::{string::String, vec, vec::Vec};

use casper_types::{
    CLType, CLTyped, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Parameter, U256,
};

use crate::{
    address::Address,
    constants::{
        ARG_ADDRESS, ARG_AMOUNT, ARG_OWNER, ARG_RECIPIENT, ARG_SPENDER, METHOD_ALLOWANCE,
        METHOD_APPROVE, METHOD_BALANCE_OF, METHOD_DECIMALS, METHOD_NAME, METHOD_SYMBOL,
        METHOD_TOTAL_SUPPLY, METHOD_TRANSFER, METHOD_TRANSFER_FROM,
    },
};

/// Creates `name` entry point definition.
pub fn get_name_entry_point() -> EntryPoint {
    EntryPoint::new(
        String::from(METHOD_NAME),
        Vec::new(),
        String::cl_type(),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    )
}

/// Creates `symbol` entry point definition.
pub fn get_symbol_entry_point() -> EntryPoint {
    EntryPoint::new(
        String::from(METHOD_SYMBOL),
        Vec::new(),
        String::cl_type(),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    )
}
/// Creates `transfer_from` entry point definition.
pub fn get_transfer_from_entry_point() -> EntryPoint {
    EntryPoint::new(
        String::from(METHOD_TRANSFER_FROM),
        vec![
            Parameter::new(ARG_OWNER, Address::cl_type()),
            Parameter::new(ARG_RECIPIENT, Address::cl_type()),
            Parameter::new(ARG_AMOUNT, U256::cl_type()),
        ],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    )
}

/// Creates `allowance` entry point definition.
pub fn get_allowance_entry_point() -> EntryPoint {
    EntryPoint::new(
        String::from(METHOD_ALLOWANCE),
        vec![
            Parameter::new(ARG_OWNER, Address::cl_type()),
            Parameter::new(ARG_SPENDER, Address::cl_type()),
        ],
        U256::cl_type(),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    )
}

/// Creates `approve` entry point definition.
pub fn get_approve_entry_point() -> EntryPoint {
    EntryPoint::new(
        String::from(METHOD_APPROVE),
        vec![
            Parameter::new(ARG_SPENDER, Address::cl_type()),
            Parameter::new(ARG_AMOUNT, U256::cl_type()),
        ],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    )
}

/// Creates `transfer` entry point definition.
pub fn get_transfer_entry_point() -> EntryPoint {
    EntryPoint::new(
        String::from(METHOD_TRANSFER),
        vec![
            Parameter::new(ARG_RECIPIENT, Address::cl_type()),
            Parameter::new(ARG_AMOUNT, U256::cl_type()),
        ],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Contract,
    )
}

/// Creates `balance_of` entry point definition.
pub fn get_balance_of_entry_point() -> EntryPoint {
    EntryPoint::new(
        String::from(METHOD_BALANCE_OF),
        vec![Parameter::new(ARG_ADDRESS, Address::cl_type())],
        U256::cl_type(),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    )
}

/// Creates `total_supply` entry point definition.
pub fn get_total_supply_entry_point() -> EntryPoint {
    EntryPoint::new(
        String::from(METHOD_TOTAL_SUPPLY),
        Vec::new(),
        U256::cl_type(),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    )
}

/// Creates `decimals` entry point definition.
pub fn get_decimals_entry_point() -> EntryPoint {
    EntryPoint::new(
        String::from(METHOD_DECIMALS),
        Vec::new(),
        u8::cl_type(),
        EntryPointAccess::Public,
        EntryPointType::Contract,
    )
}

/// Creates default set of ERC20 token entry points.
pub fn get_default_entry_points() -> EntryPoints {
    let mut entry_points = EntryPoints::new();
    entry_points.add_entry_point(get_name_entry_point());
    entry_points.add_entry_point(get_symbol_entry_point());
    entry_points.add_entry_point(get_decimals_entry_point());
    entry_points.add_entry_point(get_total_supply_entry_point());
    entry_points.add_entry_point(get_balance_of_entry_point());
    entry_points.add_entry_point(get_transfer_entry_point());
    entry_points.add_entry_point(get_approve_entry_point());
    entry_points.add_entry_point(get_allowance_entry_point());
    entry_points.add_entry_point(get_transfer_from_entry_point());
    entry_points
}
