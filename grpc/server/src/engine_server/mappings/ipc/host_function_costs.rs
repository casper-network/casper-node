use datasize::DataSize;
use std::convert::{TryFrom, TryInto};

use crate::engine_server::{
    ipc,
    mappings::{MappingError, ParsingError},
};
use casper_execution_engine::shared::host_function_costs::{HostFunction, HostFunctionCosts};

impl From<u32> for ipc::ChainSpec_WasmConfig_HostFunctionCosts_HostFunction_Argument {
    fn from(weight: u32) -> Self {
        let mut arg = ipc::ChainSpec_WasmConfig_HostFunctionCosts_HostFunction_Argument::new();
        arg.set_weight(weight);
        arg
    }
}

impl From<Option<u32>> for ipc::ChainSpec_WasmConfig_HostFunctionCosts_HostFunction_Argument {
    fn from(weight: Option<u32>) -> Self {
        let mut arg = ipc::ChainSpec_WasmConfig_HostFunctionCosts_HostFunction_Argument::new();
        arg.set_weight(weight.unwrap_or(0));
        arg
    }
}

impl From<ipc::ChainSpec_WasmConfig_HostFunctionCosts_HostFunction_Argument> for u32 {
    fn from(arg: ipc::ChainSpec_WasmConfig_HostFunctionCosts_HostFunction_Argument) -> Self {
        arg.weight
    }
}

impl From<ipc::ChainSpec_WasmConfig_HostFunctionCosts_HostFunction_Argument> for Option<u32> {
    fn from(arg: ipc::ChainSpec_WasmConfig_HostFunctionCosts_HostFunction_Argument) -> Self {
        if arg.weight > 0 {
            Some(arg.weight)
        } else {
            // Zero is considered a None case logically
            None
        }
    }
}

impl From<HostFunction<()>> for ipc::ChainSpec_WasmConfig_HostFunctionCosts_HostFunction {
    fn from(host_function_cost: HostFunction<()>) -> Self {
        let mut pb_host_function_costs = Self::new();
        pb_host_function_costs.set_cost(host_function_cost.cost);

        pb_host_function_costs
    }
}

impl TryFrom<ipc::ChainSpec_WasmConfig_HostFunctionCosts_HostFunction> for HostFunction<()> {
    type Error = MappingError;
    fn try_from(
        pb_host_function: ipc::ChainSpec_WasmConfig_HostFunctionCosts_HostFunction,
    ) -> Result<Self, Self::Error> {
        let host_function = HostFunction::fixed(pb_host_function.cost);
        Ok(host_function)
    }
}

impl From<HostFunction<(u32,)>> for ipc::ChainSpec_WasmConfig_HostFunctionCosts_HostFunction {
    fn from(host_function_cost: HostFunction<(u32,)>) -> Self {
        let mut pb_host_function_costs = Self::new();
        pb_host_function_costs.set_cost(host_function_cost.cost);
        pb_host_function_costs
            .mut_arguments()
            .push(host_function_cost.arguments.0.into());

        pb_host_function_costs
    }
}

impl TryFrom<ipc::ChainSpec_WasmConfig_HostFunctionCosts_HostFunction> for HostFunction<(u32,)> {
    type Error = ParsingError;
    fn try_from(
        pb_host_function: ipc::ChainSpec_WasmConfig_HostFunctionCosts_HostFunction,
    ) -> Result<Self, Self::Error> {
        if pb_host_function.get_arguments().len() != 1 {
            return Err("Invalid length".to_string().into());
        }

        let host_function = HostFunction::new(
            pb_host_function.cost,
            (pb_host_function.arguments[0].weight,),
        );
        Ok(host_function)
    }
}

macro_rules! host_function_impl {
    ($($n:tt $name:ident);+) => {
        impl<$($name),*> TryFrom<ipc::ChainSpec_WasmConfig_HostFunctionCosts_HostFunction> for HostFunction<($($name),*)>
        where $($name: Default + DataSize + From<ipc::ChainSpec_WasmConfig_HostFunctionCosts_HostFunction_Argument>),*
        {
            type Error = ParsingError;
            fn try_from(
                mut pb_host_function: ipc::ChainSpec_WasmConfig_HostFunctionCosts_HostFunction,
            ) -> Result<Self, Self::Error> {
                let values = [$($n,)+];

                let args: Vec<_> = pb_host_function.take_arguments().into_iter().collect();
                if args.len() != values.last().copied().unwrap() + 1{
                    return Err(format!("Expected {} arguments but received {}", values.last().unwrap(), args.len()).into());
                }

                let t = ($(args.get($n).cloned().unwrap().into(),)+);
                Ok(Self::new(pb_host_function.cost, t))
            }
        }

        impl<$($name),*> From<HostFunction<($($name),*)>> for ipc::ChainSpec_WasmConfig_HostFunctionCosts_HostFunction
        where $($name: Default + DataSize + Into<ipc::ChainSpec_WasmConfig_HostFunctionCosts_HostFunction_Argument>),*
        {
            fn from(host_function_cost: HostFunction<($($name),*)>) -> Self {
                let mut pb_host_function_costs = Self::new();
                pb_host_function_costs.set_cost(host_function_cost.cost);
                $(pb_host_function_costs.mut_arguments().push(host_function_cost.arguments.$n.into());)+
                pb_host_function_costs
            }
        }
    };
}

host_function_impl!(0 T0; 1 T1);
host_function_impl!(0 T0; 1 T1; 2 T2);
host_function_impl!(0 T0; 1 T1; 2 T2; 3 T3);
host_function_impl!(0 T0; 1 T1; 2 T2; 3 T3; 4 T4);
host_function_impl!(0 T0; 1 T1; 2 T2; 3 T3; 4 T4; 5 T5);
host_function_impl!(0 T0; 1 T1; 2 T2; 3 T3; 4 T4; 5 T5; 6 T6);
host_function_impl!(0 T0; 1 T1; 2 T2; 3 T3; 4 T4; 5 T5; 6 T6; 7 T7);
host_function_impl!(0 T0; 1 T1; 2 T2; 3 T3; 4 T4; 5 T5; 6 T6; 7 T7; 8 T8);
host_function_impl!(0 T0; 1 T1; 2 T2; 3 T3; 4 T4; 5 T5; 6 T6; 7 T7; 8 T8; 9 T9);
host_function_impl!(0 T0; 1 T1; 2 T2; 3 T3; 4 T4; 5 T5; 6 T6; 7 T7; 8 T8; 9 T9; 10 T10);

impl From<HostFunctionCosts> for ipc::ChainSpec_WasmConfig_HostFunctionCosts {
    fn from(host_function_costs: HostFunctionCosts) -> Self {
        let mut pb_host_function_costs = ipc::ChainSpec_WasmConfig_HostFunctionCosts::new();

        pb_host_function_costs.set_read_value(host_function_costs.read_value.into());
        pb_host_function_costs.set_read_value_local(host_function_costs.read_value_local.into());
        pb_host_function_costs.set_write(host_function_costs.write.into());
        pb_host_function_costs.set_write_local(host_function_costs.write_local.into());
        pb_host_function_costs.set_add(host_function_costs.add.into());
        pb_host_function_costs.set_add_local(host_function_costs.add_local.into());
        pb_host_function_costs.set_new_uref(host_function_costs.new_uref.into());
        pb_host_function_costs.set_load_named_keys(host_function_costs.load_named_keys.into());
        pb_host_function_costs.set_ret(host_function_costs.ret.into());
        pb_host_function_costs.set_get_key(host_function_costs.get_key.into());
        pb_host_function_costs.set_has_key(host_function_costs.has_key.into());
        pb_host_function_costs.set_put_key(host_function_costs.put_key.into());
        pb_host_function_costs.set_remove_key(host_function_costs.remove_key.into());
        pb_host_function_costs.set_revert(host_function_costs.revert.into());
        pb_host_function_costs.set_is_valid_uref(host_function_costs.is_valid_uref.into());
        pb_host_function_costs
            .set_add_associated_key(host_function_costs.add_associated_key.into());
        pb_host_function_costs
            .set_remove_associated_key(host_function_costs.remove_associated_key.into());
        pb_host_function_costs
            .set_update_associated_key(host_function_costs.update_associated_key.into());
        pb_host_function_costs
            .set_set_action_threshold(host_function_costs.set_action_threshold.into());
        pb_host_function_costs.set_get_caller(host_function_costs.get_caller.into());
        pb_host_function_costs.set_get_blocktime(host_function_costs.get_blocktime.into());
        pb_host_function_costs.set_create_purse(host_function_costs.create_purse.into());
        pb_host_function_costs
            .set_transfer_to_account(host_function_costs.transfer_to_account.into());
        pb_host_function_costs.set_transfer_from_purse_to_account(
            host_function_costs.transfer_from_purse_to_account.into(),
        );
        pb_host_function_costs.set_transfer_from_purse_to_purse(
            host_function_costs.transfer_from_purse_to_purse.into(),
        );
        pb_host_function_costs.set_get_balance(host_function_costs.get_balance.into());
        pb_host_function_costs.set_get_phase(host_function_costs.get_phase.into());
        pb_host_function_costs
            .set_get_system_contract(host_function_costs.get_system_contract.into());
        pb_host_function_costs.set_get_main_purse(host_function_costs.get_main_purse.into());
        pb_host_function_costs.set_read_host_buffer(host_function_costs.read_host_buffer.into());
        pb_host_function_costs.set_create_contract_package_at_hash(
            host_function_costs.create_contract_package_at_hash.into(),
        );
        pb_host_function_costs
            .set_create_contract_user_group(host_function_costs.create_contract_user_group.into());
        pb_host_function_costs
            .set_add_contract_version(host_function_costs.add_contract_version.into());
        pb_host_function_costs
            .set_disable_contract_version(host_function_costs.disable_contract_version.into());
        pb_host_function_costs.set_call_contract(host_function_costs.call_contract.into());
        pb_host_function_costs
            .set_call_versioned_contract(host_function_costs.call_versioned_contract.into());
        pb_host_function_costs
            .set_get_named_arg_size(host_function_costs.get_named_arg_size.into());
        pb_host_function_costs.set_get_named_arg(host_function_costs.get_named_arg.into());
        pb_host_function_costs
            .set_remove_contract_user_group(host_function_costs.remove_contract_user_group.into());
        pb_host_function_costs.set_provision_contract_user_group_uref(
            host_function_costs
                .provision_contract_user_group_uref
                .into(),
        );
        pb_host_function_costs.set_remove_contract_user_group_urefs(
            host_function_costs.remove_contract_user_group_urefs.into(),
        );
        pb_host_function_costs.set_print(host_function_costs.print.into());
        pb_host_function_costs
    }
}

impl TryFrom<ipc::ChainSpec_WasmConfig_HostFunctionCosts> for HostFunctionCosts {
    type Error = MappingError;

    fn try_from(
        mut pb_host_function_costs: ipc::ChainSpec_WasmConfig_HostFunctionCosts,
    ) -> Result<Self, Self::Error> {
        Ok(HostFunctionCosts {
            read_value: pb_host_function_costs.take_read_value().try_into()?,
            read_value_local: pb_host_function_costs.take_read_value_local().try_into()?,
            write: pb_host_function_costs.take_write().try_into()?,
            write_local: pb_host_function_costs.take_write_local().try_into()?,
            add: pb_host_function_costs.take_add().try_into()?,
            add_local: pb_host_function_costs.take_add_local().try_into()?,
            new_uref: pb_host_function_costs.take_new_uref().try_into()?,
            load_named_keys: pb_host_function_costs.take_load_named_keys().try_into()?,
            ret: pb_host_function_costs.take_ret().try_into()?,
            get_key: pb_host_function_costs.take_get_key().try_into()?,
            has_key: pb_host_function_costs.take_has_key().try_into()?,
            put_key: pb_host_function_costs.take_put_key().try_into()?,
            remove_key: pb_host_function_costs.take_remove_key().try_into()?,
            revert: pb_host_function_costs.take_revert().try_into()?,
            is_valid_uref: pb_host_function_costs.take_is_valid_uref().try_into()?,
            add_associated_key: pb_host_function_costs
                .take_add_associated_key()
                .try_into()?,
            remove_associated_key: pb_host_function_costs
                .take_remove_associated_key()
                .try_into()?,
            update_associated_key: pb_host_function_costs
                .take_update_associated_key()
                .try_into()?,
            set_action_threshold: pb_host_function_costs
                .take_set_action_threshold()
                .try_into()?,
            get_caller: pb_host_function_costs.take_get_caller().try_into()?,
            get_blocktime: pb_host_function_costs.take_get_blocktime().try_into()?,
            create_purse: pb_host_function_costs.take_create_purse().try_into()?,
            transfer_to_account: pb_host_function_costs
                .take_transfer_to_account()
                .try_into()?,
            transfer_from_purse_to_account: pb_host_function_costs
                .take_transfer_from_purse_to_account()
                .try_into()?,
            transfer_from_purse_to_purse: pb_host_function_costs
                .take_transfer_from_purse_to_purse()
                .try_into()?,
            get_balance: pb_host_function_costs.take_get_balance().try_into()?,
            get_phase: pb_host_function_costs.take_get_phase().try_into()?,
            get_system_contract: pb_host_function_costs
                .take_get_system_contract()
                .try_into()?,
            get_main_purse: pb_host_function_costs.take_get_main_purse().try_into()?,
            read_host_buffer: pb_host_function_costs.take_read_host_buffer().try_into()?,
            create_contract_package_at_hash: pb_host_function_costs
                .take_create_contract_package_at_hash()
                .try_into()?,
            create_contract_user_group: pb_host_function_costs
                .take_create_contract_user_group()
                .try_into()?,
            add_contract_version: pb_host_function_costs
                .take_add_contract_version()
                .try_into()?,
            disable_contract_version: pb_host_function_costs
                .take_disable_contract_version()
                .try_into()?,
            call_contract: pb_host_function_costs.take_call_contract().try_into()?,
            call_versioned_contract: pb_host_function_costs
                .take_call_versioned_contract()
                .try_into()?,
            get_named_arg_size: pb_host_function_costs
                .take_get_named_arg_size()
                .try_into()?,
            get_named_arg: pb_host_function_costs.take_get_named_arg().try_into()?,
            remove_contract_user_group: pb_host_function_costs
                .take_remove_contract_user_group()
                .try_into()?,
            provision_contract_user_group_uref: pb_host_function_costs
                .take_provision_contract_user_group_uref()
                .try_into()?,
            remove_contract_user_group_urefs: pb_host_function_costs
                .take_remove_contract_user_group_urefs()
                .try_into()?,
            print: pb_host_function_costs.take_print().try_into()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use proptest::proptest;

    use casper_execution_engine::shared::host_function_costs::gens;

    use super::*;
    use crate::engine_server::mappings::test_utils;

    proptest! {
        #[test]
        fn round_trip(host_function_costs in gens::host_function_costs_arb()) {
            test_utils::protobuf_round_trip::<HostFunctionCosts, ipc::ChainSpec_WasmConfig_HostFunctionCosts>(host_function_costs);
        }
    }
}
