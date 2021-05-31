use casper_types::bytesrepr::{self, FromBytes};

use crate::shared::host_function_costs::{Cost, HostFunction, HostFunctionCosts};

pub(crate) type LegacyCost = u32;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) struct LegacyHostFunction<T> {
    /// How much user is charged for cost only
    cost: LegacyCost,
    arguments: T,
}

impl<T> FromBytes for LegacyHostFunction<T>
where
    T: Default + AsMut<[Cost]>,
{
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (cost, mut bytes) = FromBytes::from_bytes(bytes)?;
        let mut arguments = T::default();
        let arguments_mut = arguments.as_mut();
        for ith_argument in arguments_mut {
            let (cost, rem) = FromBytes::from_bytes(bytes)?;
            *ith_argument = cost;
            bytes = rem;
        }
        Ok((Self { cost, arguments }, bytes))
    }
}

impl<T> From<LegacyHostFunction<T>> for HostFunction<T> {
    fn from(value: LegacyHostFunction<T>) -> Self {
        HostFunction::new(value.cost, value.arguments)
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) struct LegacyHostFunctionCosts(HostFunctionCosts);

impl From<LegacyHostFunctionCosts> for HostFunctionCosts {
    fn from(value: LegacyHostFunctionCosts) -> Self {
        value.0
    }
}

impl FromBytes for LegacyHostFunctionCosts {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (read_value, bytes): (LegacyHostFunction<[LegacyCost; 3]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (read_value_local, bytes): (LegacyHostFunction<[LegacyCost; 3]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (write, bytes): (LegacyHostFunction<[LegacyCost; 4]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (write_local, bytes): (LegacyHostFunction<[LegacyCost; 4]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (add, bytes): (LegacyHostFunction<[LegacyCost; 4]>, _) = FromBytes::from_bytes(bytes)?;
        let (new_uref, bytes): (LegacyHostFunction<[LegacyCost; 3]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (load_named_keys, bytes): (LegacyHostFunction<[LegacyCost; 2]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (ret, bytes): (LegacyHostFunction<[LegacyCost; 2]>, _) = FromBytes::from_bytes(bytes)?;
        let (get_key, bytes): (LegacyHostFunction<[LegacyCost; 5]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (has_key, bytes): (LegacyHostFunction<[LegacyCost; 2]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (put_key, bytes): (LegacyHostFunction<[LegacyCost; 4]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (remove_key, bytes): (LegacyHostFunction<[LegacyCost; 2]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (revert, bytes): (LegacyHostFunction<[LegacyCost; 1]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (is_valid_uref, bytes): (LegacyHostFunction<[LegacyCost; 2]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (add_associated_key, bytes): (LegacyHostFunction<[LegacyCost; 3]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (remove_associated_key, bytes): (LegacyHostFunction<[LegacyCost; 2]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (update_associated_key, bytes): (LegacyHostFunction<[LegacyCost; 3]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (set_action_threshold, bytes): (LegacyHostFunction<[LegacyCost; 2]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (get_caller, bytes): (LegacyHostFunction<[LegacyCost; 1]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (get_blocktime, bytes): (LegacyHostFunction<[LegacyCost; 1]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (create_purse, bytes): (LegacyHostFunction<[LegacyCost; 2]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (transfer_to_account, bytes): (LegacyHostFunction<[LegacyCost; 7]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (transfer_from_purse_to_account, bytes): (LegacyHostFunction<[LegacyCost; 9]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (transfer_from_purse_to_purse, bytes): (LegacyHostFunction<[LegacyCost; 8]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (get_balance, bytes): (LegacyHostFunction<[LegacyCost; 3]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (get_phase, bytes): (LegacyHostFunction<[LegacyCost; 1]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (get_system_contract, bytes): (LegacyHostFunction<[LegacyCost; 3]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (get_main_purse, bytes): (LegacyHostFunction<[LegacyCost; 1]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (read_host_buffer, bytes): (LegacyHostFunction<[LegacyCost; 3]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (create_contract_package_at_hash, bytes): (LegacyHostFunction<[LegacyCost; 2]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (create_contract_user_group, bytes): (LegacyHostFunction<[LegacyCost; 8]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (add_contract_version, bytes): (LegacyHostFunction<[LegacyCost; 10]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (disable_contract_version, bytes): (LegacyHostFunction<[LegacyCost; 4]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (call_contract, bytes): (LegacyHostFunction<[LegacyCost; 7]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (call_versioned_contract, bytes): (LegacyHostFunction<[LegacyCost; 9]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (get_named_arg_size, bytes): (LegacyHostFunction<[LegacyCost; 3]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (get_named_arg, bytes): (LegacyHostFunction<[LegacyCost; 4]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (remove_contract_user_group, bytes): (LegacyHostFunction<[LegacyCost; 4]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (provision_contract_user_group_uref, bytes): (LegacyHostFunction<[LegacyCost; 5]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (remove_contract_user_group_urefs, bytes): (LegacyHostFunction<[LegacyCost; 6]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (print, bytes): (LegacyHostFunction<[LegacyCost; 2]>, _) =
            FromBytes::from_bytes(bytes)?;
        let (blake2b, bytes): (LegacyHostFunction<[LegacyCost; 4]>, _) =
            FromBytes::from_bytes(bytes)?;

        let legacy_host_function_costs = HostFunctionCosts {
            read_value: read_value.into(),
            read_value_local: read_value_local.into(),
            write: write.into(),
            write_local: write_local.into(),
            add: add.into(),
            new_uref: new_uref.into(),
            load_named_keys: load_named_keys.into(),
            ret: ret.into(),
            get_key: get_key.into(),
            has_key: has_key.into(),
            put_key: put_key.into(),
            remove_key: remove_key.into(),
            revert: revert.into(),
            is_valid_uref: is_valid_uref.into(),
            add_associated_key: add_associated_key.into(),
            remove_associated_key: remove_associated_key.into(),
            update_associated_key: update_associated_key.into(),
            set_action_threshold: set_action_threshold.into(),
            get_caller: get_caller.into(),
            get_blocktime: get_blocktime.into(),
            create_purse: create_purse.into(),
            transfer_to_account: transfer_to_account.into(),
            transfer_from_purse_to_account: transfer_from_purse_to_account.into(),
            transfer_from_purse_to_purse: transfer_from_purse_to_purse.into(),
            get_balance: get_balance.into(),
            get_phase: get_phase.into(),
            get_system_contract: get_system_contract.into(),
            get_main_purse: get_main_purse.into(),
            read_host_buffer: read_host_buffer.into(),
            create_contract_package_at_hash: create_contract_package_at_hash.into(),
            create_contract_user_group: create_contract_user_group.into(),
            add_contract_version: add_contract_version.into(),
            disable_contract_version: disable_contract_version.into(),
            call_contract: call_contract.into(),
            call_versioned_contract: call_versioned_contract.into(),
            get_named_arg_size: get_named_arg_size.into(),
            get_named_arg: get_named_arg.into(),
            remove_contract_user_group: remove_contract_user_group.into(),
            provision_contract_user_group_uref: provision_contract_user_group_uref.into(),
            remove_contract_user_group_urefs: remove_contract_user_group_urefs.into(),
            print: print.into(),
            blake2b: blake2b.into(),
        };

        Ok((LegacyHostFunctionCosts(legacy_host_function_costs), bytes))
    }
}
