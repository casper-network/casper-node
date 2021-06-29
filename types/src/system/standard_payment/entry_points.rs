use alloc::{boxed::Box, string::ToString};

use crate::{
    system::standard_payment::{ARG_AMOUNT, METHOD_PAY},
    CLType, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Parameter,
};

/// Creates standard payment contract entry points.
pub fn standard_payment_entry_points() -> EntryPoints {
    let mut entry_points = EntryPoints::new();

    let entry_point = EntryPoint::new(
        METHOD_PAY.to_string(),
        vec![Parameter::new(ARG_AMOUNT, CLType::U512)],
        CLType::Result {
            ok: Box::new(CLType::Unit),
            err: Box::new(CLType::U32),
        },
        EntryPointAccess::Public,
        EntryPointType::Session,
    );
    entry_points.add_entry_point(entry_point);

    entry_points
}
