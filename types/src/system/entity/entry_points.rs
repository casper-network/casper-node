use crate::{
    account::{AccountHash, Weight},
    CLType, CLTyped, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Parameter,
};

use super::{
    ARG_ACCOUNT, ARG_WEIGHT, METHOD_ADD_ASSOCIATED_KEY, METHOD_REMOVE_ASSOCIATED_KEY,
    METHOD_UPDATE_ASSOCIATED_KEY,
};

/// Creates entity contract entry points.
pub fn entity_entry_points() -> EntryPoints {
    let mut entry_points = EntryPoints::new();

    let entry_point = EntryPoint::new(
        METHOD_ADD_ASSOCIATED_KEY,
        vec![
            Parameter::new(ARG_ACCOUNT, AccountHash::cl_type()),
            Parameter::new(ARG_WEIGHT, Weight::cl_type()),
        ],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Called,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_REMOVE_ASSOCIATED_KEY,
        vec![Parameter::new(ARG_ACCOUNT, AccountHash::cl_type())],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Called,
    );
    entry_points.add_entry_point(entry_point);

    let entry_point = EntryPoint::new(
        METHOD_UPDATE_ASSOCIATED_KEY,
        vec![
            Parameter::new(ARG_ACCOUNT, AccountHash::cl_type()),
            Parameter::new(ARG_WEIGHT, Weight::cl_type()),
        ],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Called,
    );
    entry_points.add_entry_point(entry_point);

    entry_points
}
