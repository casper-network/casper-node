use alloc::boxed::Box;

use crate::{
    system::handle_payment::{
        ARG_PURSE, METHOD_GET_PAYMENT_PURSE, METHOD_GET_REFUND_PURSE, METHOD_SET_REFUND_PURSE,
    },
    CLType, EntryPoint, EntryPointAccess, EntryPointPayment, EntryPointType, EntryPoints,
    Parameter,
};

/// Creates handle payment contract entry points.
pub fn handle_payment_entry_points() -> EntryPoints {
    let mut entry_points = EntryPoints::new();

    let get_payment_purse = EntryPoint::new(
        METHOD_GET_PAYMENT_PURSE,
        vec![],
        CLType::URef,
        EntryPointAccess::Public,
        EntryPointType::Called,
        EntryPointPayment::Caller,
    );
    entry_points.add_entry_point(get_payment_purse);

    let set_refund_purse = EntryPoint::new(
        METHOD_SET_REFUND_PURSE,
        vec![Parameter::new(ARG_PURSE, CLType::URef)],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::Called,
        EntryPointPayment::Caller,
    );
    entry_points.add_entry_point(set_refund_purse);

    let get_refund_purse = EntryPoint::new(
        METHOD_GET_REFUND_PURSE,
        vec![],
        CLType::Option(Box::new(CLType::URef)),
        EntryPointAccess::Public,
        EntryPointType::Called,
        EntryPointPayment::Caller,
    );
    entry_points.add_entry_point(get_refund_purse);

    entry_points
}
