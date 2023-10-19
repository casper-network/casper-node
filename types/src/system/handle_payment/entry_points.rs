use alloc::boxed::Box;

use crate::{
    system::handle_payment::{
        ARG_ACCOUNT, ARG_AMOUNT, ARG_PURSE, METHOD_FINALIZE_PAYMENT, METHOD_GET_PAYMENT_PURSE,
        METHOD_GET_REFUND_PURSE, METHOD_SET_REFUND_PURSE,
    },
    CLType, EntryPoint, EntryPointAccess, EntryPointType, EntryPoints, Parameter,
};

use super::METHOD_DISTRIBUTE_ACCUMULATED_FEES;

/// Creates handle payment contract entry points.
pub fn handle_payment_entry_points() -> EntryPoints {
    let mut entry_points = EntryPoints::new();

    let get_payment_purse = EntryPoint::new(
        METHOD_GET_PAYMENT_PURSE,
        vec![],
        CLType::URef,
        EntryPointAccess::Public,
        EntryPointType::AddressableEntity,
    );
    entry_points.add_entry_point(get_payment_purse);

    let set_refund_purse = EntryPoint::new(
        METHOD_SET_REFUND_PURSE,
        vec![Parameter::new(ARG_PURSE, CLType::URef)],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::AddressableEntity,
    );
    entry_points.add_entry_point(set_refund_purse);

    let get_refund_purse = EntryPoint::new(
        METHOD_GET_REFUND_PURSE,
        vec![],
        CLType::Option(Box::new(CLType::URef)),
        EntryPointAccess::Public,
        EntryPointType::AddressableEntity,
    );
    entry_points.add_entry_point(get_refund_purse);

    let finalize_payment = EntryPoint::new(
        METHOD_FINALIZE_PAYMENT,
        vec![
            Parameter::new(ARG_AMOUNT, CLType::U512),
            Parameter::new(ARG_ACCOUNT, CLType::ByteArray(32)),
        ],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::AddressableEntity,
    );
    entry_points.add_entry_point(finalize_payment);

    let distribute_accumulated_fees = EntryPoint::new(
        METHOD_DISTRIBUTE_ACCUMULATED_FEES,
        vec![],
        CLType::Unit,
        EntryPointAccess::Public,
        EntryPointType::AddressableEntity,
    );
    entry_points.add_entry_point(distribute_accumulated_fees);

    entry_points
}
