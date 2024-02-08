use alloc::boxed::Box;

use crate::{
    addressable_entity::Parameters,
    system::mint::{
        ARG_AMOUNT, ARG_ID, ARG_PURSE, ARG_SOURCE, ARG_TARGET, ARG_TO, METHOD_BALANCE,
        METHOD_CREATE, METHOD_MINT, METHOD_MINT_INTO_EXISTING_PURSE, METHOD_READ_BASE_ROUND_REWARD,
        METHOD_REDUCE_TOTAL_SUPPLY, METHOD_TRANSFER,
    },
    CLType, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Parameter,
};

/// Returns entry points for a mint system contract.
pub fn mint_entry_points() -> EntryPoints {
    let mut entry_points = EntryPoints::new();

    let entry_point = EntryPoint::new(
        METHOD_MINT,
        vec![Parameter::new(ARG_AMOUNT, CLType::U512)],
        CLType::Result {
            ok: Box::new(CLType::URef),
            err: Box::new(CLType::U8),
        },
        EntryPointAccess::Public,
        EntryPointType::AddressableEntity,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_REDUCE_TOTAL_SUPPLY,
        vec![Parameter::new(ARG_AMOUNT, CLType::U512)],
        CLType::Result {
            ok: Box::new(CLType::Unit),
            err: Box::new(CLType::U8),
        },
        EntryPointAccess::Public,
        EntryPointType::AddressableEntity,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_CREATE,
        Parameters::new(),
        CLType::URef,
        EntryPointAccess::Public,
        EntryPointType::AddressableEntity,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_BALANCE,
        vec![Parameter::new(ARG_PURSE, CLType::URef)],
        CLType::Option(Box::new(CLType::U512)),
        EntryPointAccess::Public,
        EntryPointType::AddressableEntity,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_TRANSFER,
        vec![
            Parameter::new(ARG_TO, CLType::Option(Box::new(CLType::ByteArray(32)))),
            Parameter::new(ARG_SOURCE, CLType::URef),
            Parameter::new(ARG_TARGET, CLType::URef),
            Parameter::new(ARG_AMOUNT, CLType::U512),
            Parameter::new(ARG_ID, CLType::Option(Box::new(CLType::U64))),
        ],
        CLType::Result {
            ok: Box::new(CLType::Unit),
            err: Box::new(CLType::U8),
        },
        EntryPointAccess::Public,
        EntryPointType::AddressableEntity,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_READ_BASE_ROUND_REWARD,
        Parameters::new(),
        CLType::U512,
        EntryPointAccess::Public,
        EntryPointType::AddressableEntity,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_MINT_INTO_EXISTING_PURSE,
        vec![
            Parameter::new(ARG_AMOUNT, CLType::U512),
            Parameter::new(ARG_PURSE, CLType::URef),
        ],
        CLType::Result {
            ok: Box::new(CLType::Unit),
            err: Box::new(CLType::U8),
        },
        EntryPointAccess::Public,
        EntryPointType::AddressableEntity,
    );
    entry_points.add_entry_point(entry_point);

    entry_points
}
