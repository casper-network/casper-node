use datasize::DataSize;
use rand::{distributions::Standard, prelude::Distribution, Rng};
use serde::{Deserialize, Serialize};

use casper_types::bytesrepr::{self, FromBytes, ToBytes};

use super::gas::Gas;

/// Representation of argument's cost.
pub type Cost = u32;

/// Representation of a host function cost
///
/// Total gas cost is equal to `cost` + sum of each argument weight multiplied by the byte size of
/// the data.
#[derive(Copy, Clone, PartialEq, Eq, Deserialize, Serialize, Debug, Default)]
pub struct HostFunction<T> {
    /// How much user is charged for cost only
    cost: Cost,
    arguments: T,
}

impl<T> DataSize for HostFunction<T>
where
    T: Copy + AsRef<[Cost]>,
{
    const IS_DYNAMIC: bool = false;

    const STATIC_HEAP_SIZE: usize = 0;

    fn estimate_heap_size(&self) -> usize {
        0
    }
}

impl<T> HostFunction<T> {
    pub fn new(cost: Cost, arguments: T) -> Self {
        Self { cost, arguments }
    }

    pub fn cost(&self) -> Cost {
        self.cost
    }
}

impl<T> HostFunction<T>
where
    T: AsRef<[Cost]>,
{
    pub fn arguments(&self) -> &[Cost] {
        self.arguments.as_ref()
    }

    /// Calculate gas cost for a host function
    pub fn calculate_gas_cost(&self, weights: T) -> Gas {
        let mut gas = Gas::new(self.cost.into());
        for (argument, weight) in self.arguments.as_ref().iter().zip(weights.as_ref()) {
            let lhs = Gas::new((*argument).into());
            let rhs = Gas::new((*weight).into());
            gas += lhs * rhs;
        }
        gas
    }
}

impl<T> Distribution<HostFunction<T>> for Standard
where
    Standard: Distribution<T>,
    T: AsRef<[Cost]>,
{
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> HostFunction<T> {
        let cost = rng.gen::<Cost>();
        let arguments = rng.gen();
        HostFunction::<T> { cost, arguments }
    }
}

impl<T> ToBytes for HostFunction<T>
where
    T: ToBytes + AsRef<[Cost]>,
{
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);
        ret.append(&mut self.cost.to_bytes()?);
        ret.append(&mut self.arguments.to_bytes()?);
        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        self.cost.serialized_length() + self.arguments.serialized_length()
    }
}

impl<T> FromBytes for HostFunction<T>
where
    T: FromBytes + Default,
{
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (cost, bytes) = FromBytes::from_bytes(bytes)?;
        let (arguments, bytes) = FromBytes::from_bytes(bytes)?;
        Ok((Self { cost, arguments }, bytes))
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug, DataSize, Default)]
pub struct HostFunctionCosts {
    pub read_value: HostFunction<[Cost; 3]>,
    pub read_value_local: HostFunction<[Cost; 3]>,
    pub write: HostFunction<[Cost; 4]>,
    pub write_local: HostFunction<[Cost; 4]>,
    pub add: HostFunction<[Cost; 4]>,
    pub add_local: HostFunction<[Cost; 4]>,
    pub new_uref: HostFunction<[Cost; 3]>,
    pub load_named_keys: HostFunction<[Cost; 2]>,
    pub ret: HostFunction<[Cost; 2]>,
    pub get_key: HostFunction<[Cost; 5]>,
    pub has_key: HostFunction<[Cost; 2]>,
    pub put_key: HostFunction<[Cost; 4]>,
    pub remove_key: HostFunction<[Cost; 2]>,
    pub revert: HostFunction<[Cost; 1]>,
    pub is_valid_uref: HostFunction<[Cost; 2]>,
    pub add_associated_key: HostFunction<[Cost; 3]>,
    pub remove_associated_key: HostFunction<[Cost; 2]>,
    pub update_associated_key: HostFunction<[Cost; 3]>,
    pub set_action_threshold: HostFunction<[Cost; 2]>,
    pub get_caller: HostFunction<[Cost; 1]>,
    pub get_blocktime: HostFunction<[Cost; 1]>,
    pub create_purse: HostFunction<[Cost; 2]>,
    pub transfer_to_account: HostFunction<[Cost; 4]>,
    pub transfer_from_purse_to_account: HostFunction<[Cost; 6]>,
    pub transfer_from_purse_to_purse: HostFunction<[Cost; 6]>,
    pub get_balance: HostFunction<[Cost; 3]>,
    pub get_phase: HostFunction<[Cost; 1]>,
    pub get_system_contract: HostFunction<[Cost; 3]>,
    pub get_main_purse: HostFunction<[Cost; 1]>,
    pub read_host_buffer: HostFunction<[Cost; 3]>,
    pub create_contract_package_at_hash: HostFunction<[Cost; 2]>,
    pub create_contract_user_group: HostFunction<[Cost; 8]>,
    pub add_contract_version: HostFunction<[Cost; 10]>,
    pub disable_contract_version: HostFunction<[Cost; 4]>,
    pub call_contract: HostFunction<[Cost; 7]>,
    pub call_versioned_contract: HostFunction<[Cost; 9]>,
    pub get_named_arg_size: HostFunction<[Cost; 3]>,
    pub get_named_arg: HostFunction<[Cost; 4]>,
    pub remove_contract_user_group: HostFunction<[Cost; 4]>,
    pub provision_contract_user_group_uref: HostFunction<[Cost; 5]>,
    pub remove_contract_user_group_urefs: HostFunction<[Cost; 6]>,
    pub print: HostFunction<[Cost; 2]>,
    pub blake2b: HostFunction<[Cost; 4]>,
}

impl ToBytes for HostFunctionCosts {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);
        ret.append(&mut self.read_value.to_bytes()?);
        ret.append(&mut self.read_value_local.to_bytes()?);
        ret.append(&mut self.write.to_bytes()?);
        ret.append(&mut self.write_local.to_bytes()?);
        ret.append(&mut self.add.to_bytes()?);
        ret.append(&mut self.add_local.to_bytes()?);
        ret.append(&mut self.new_uref.to_bytes()?);
        ret.append(&mut self.load_named_keys.to_bytes()?);
        ret.append(&mut self.ret.to_bytes()?);
        ret.append(&mut self.get_key.to_bytes()?);
        ret.append(&mut self.has_key.to_bytes()?);
        ret.append(&mut self.put_key.to_bytes()?);
        ret.append(&mut self.remove_key.to_bytes()?);
        ret.append(&mut self.revert.to_bytes()?);
        ret.append(&mut self.is_valid_uref.to_bytes()?);
        ret.append(&mut self.add_associated_key.to_bytes()?);
        ret.append(&mut self.remove_associated_key.to_bytes()?);
        ret.append(&mut self.update_associated_key.to_bytes()?);
        ret.append(&mut self.set_action_threshold.to_bytes()?);
        ret.append(&mut self.get_caller.to_bytes()?);
        ret.append(&mut self.get_blocktime.to_bytes()?);
        ret.append(&mut self.create_purse.to_bytes()?);
        ret.append(&mut self.transfer_to_account.to_bytes()?);
        ret.append(&mut self.transfer_from_purse_to_account.to_bytes()?);
        ret.append(&mut self.transfer_from_purse_to_purse.to_bytes()?);
        ret.append(&mut self.get_balance.to_bytes()?);
        ret.append(&mut self.get_phase.to_bytes()?);
        ret.append(&mut self.get_system_contract.to_bytes()?);
        ret.append(&mut self.get_main_purse.to_bytes()?);
        ret.append(&mut self.read_host_buffer.to_bytes()?);
        ret.append(&mut self.create_contract_package_at_hash.to_bytes()?);
        ret.append(&mut self.create_contract_user_group.to_bytes()?);
        ret.append(&mut self.add_contract_version.to_bytes()?);
        ret.append(&mut self.disable_contract_version.to_bytes()?);
        ret.append(&mut self.call_contract.to_bytes()?);
        ret.append(&mut self.call_versioned_contract.to_bytes()?);
        ret.append(&mut self.get_named_arg_size.to_bytes()?);
        ret.append(&mut self.get_named_arg.to_bytes()?);
        ret.append(&mut self.remove_contract_user_group.to_bytes()?);
        ret.append(&mut self.provision_contract_user_group_uref.to_bytes()?);
        ret.append(&mut self.remove_contract_user_group_urefs.to_bytes()?);
        ret.append(&mut self.print.to_bytes()?);
        ret.append(&mut self.blake2b.to_bytes()?);
        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        self.read_value.serialized_length()
            + self.read_value_local.serialized_length()
            + self.write.serialized_length()
            + self.write_local.serialized_length()
            + self.add.serialized_length()
            + self.add_local.serialized_length()
            + self.new_uref.serialized_length()
            + self.load_named_keys.serialized_length()
            + self.ret.serialized_length()
            + self.get_key.serialized_length()
            + self.has_key.serialized_length()
            + self.put_key.serialized_length()
            + self.remove_key.serialized_length()
            + self.revert.serialized_length()
            + self.is_valid_uref.serialized_length()
            + self.add_associated_key.serialized_length()
            + self.remove_associated_key.serialized_length()
            + self.update_associated_key.serialized_length()
            + self.set_action_threshold.serialized_length()
            + self.get_caller.serialized_length()
            + self.get_blocktime.serialized_length()
            + self.create_purse.serialized_length()
            + self.transfer_to_account.serialized_length()
            + self.transfer_from_purse_to_account.serialized_length()
            + self.transfer_from_purse_to_purse.serialized_length()
            + self.get_balance.serialized_length()
            + self.get_phase.serialized_length()
            + self.get_system_contract.serialized_length()
            + self.get_main_purse.serialized_length()
            + self.read_host_buffer.serialized_length()
            + self.create_contract_package_at_hash.serialized_length()
            + self.create_contract_user_group.serialized_length()
            + self.add_contract_version.serialized_length()
            + self.disable_contract_version.serialized_length()
            + self.call_contract.serialized_length()
            + self.call_versioned_contract.serialized_length()
            + self.get_named_arg_size.serialized_length()
            + self.get_named_arg.serialized_length()
            + self.remove_contract_user_group.serialized_length()
            + self.provision_contract_user_group_uref.serialized_length()
            + self.remove_contract_user_group_urefs.serialized_length()
            + self.print.serialized_length()
            + self.blake2b.serialized_length()
    }
}

impl FromBytes for HostFunctionCosts {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (read_value, rem) = FromBytes::from_bytes(bytes)?;
        let (read_value_local, rem) = FromBytes::from_bytes(rem)?;
        let (write, rem) = FromBytes::from_bytes(rem)?;
        let (write_local, rem) = FromBytes::from_bytes(rem)?;
        let (add, rem) = FromBytes::from_bytes(rem)?;
        let (add_local, rem) = FromBytes::from_bytes(rem)?;
        let (new_uref, rem) = FromBytes::from_bytes(rem)?;
        let (load_named_keys, rem) = FromBytes::from_bytes(rem)?;
        let (ret, rem) = FromBytes::from_bytes(rem)?;
        let (get_key, rem) = FromBytes::from_bytes(rem)?;
        let (has_key, rem) = FromBytes::from_bytes(rem)?;
        let (put_key, rem) = FromBytes::from_bytes(rem)?;
        let (remove_key, rem) = FromBytes::from_bytes(rem)?;
        let (revert, rem) = FromBytes::from_bytes(rem)?;
        let (is_valid_uref, rem) = FromBytes::from_bytes(rem)?;
        let (add_associated_key, rem) = FromBytes::from_bytes(rem)?;
        let (remove_associated_key, rem) = FromBytes::from_bytes(rem)?;
        let (update_associated_key, rem) = FromBytes::from_bytes(rem)?;
        let (set_action_threshold, rem) = FromBytes::from_bytes(rem)?;
        let (get_caller, rem) = FromBytes::from_bytes(rem)?;
        let (get_blocktime, rem) = FromBytes::from_bytes(rem)?;
        let (create_purse, rem) = FromBytes::from_bytes(rem)?;
        let (transfer_to_account, rem) = FromBytes::from_bytes(rem)?;
        let (transfer_from_purse_to_account, rem) = FromBytes::from_bytes(rem)?;
        let (transfer_from_purse_to_purse, rem) = FromBytes::from_bytes(rem)?;
        let (get_balance, rem) = FromBytes::from_bytes(rem)?;
        let (get_phase, rem) = FromBytes::from_bytes(rem)?;
        let (get_system_contract, rem) = FromBytes::from_bytes(rem)?;
        let (get_main_purse, rem) = FromBytes::from_bytes(rem)?;
        let (read_host_buffer, rem) = FromBytes::from_bytes(rem)?;
        let (create_contract_package_at_hash, rem) = FromBytes::from_bytes(rem)?;
        let (create_contract_user_group, rem) = FromBytes::from_bytes(rem)?;
        let (add_contract_version, rem) = FromBytes::from_bytes(rem)?;
        let (disable_contract_version, rem) = FromBytes::from_bytes(rem)?;
        let (call_contract, rem) = FromBytes::from_bytes(rem)?;
        let (call_versioned_contract, rem) = FromBytes::from_bytes(rem)?;
        let (get_named_arg_size, rem) = FromBytes::from_bytes(rem)?;
        let (get_named_arg, rem) = FromBytes::from_bytes(rem)?;
        let (remove_contract_user_group, rem) = FromBytes::from_bytes(rem)?;
        let (provision_contract_user_group_uref, rem) = FromBytes::from_bytes(rem)?;
        let (remove_contract_user_group_urefs, rem) = FromBytes::from_bytes(rem)?;
        let (print, rem) = FromBytes::from_bytes(rem)?;
        let (blake2b, rem) = FromBytes::from_bytes(rem)?;
        Ok((
            HostFunctionCosts {
                read_value,
                read_value_local,
                write,
                write_local,
                add,
                add_local,
                new_uref,
                load_named_keys,
                ret,
                get_key,
                has_key,
                put_key,
                remove_key,
                revert,
                is_valid_uref,
                add_associated_key,
                remove_associated_key,
                update_associated_key,
                set_action_threshold,
                get_caller,
                get_blocktime,
                create_purse,
                transfer_to_account,
                transfer_from_purse_to_account,
                transfer_from_purse_to_purse,
                get_balance,
                get_phase,
                get_system_contract,
                get_main_purse,
                read_host_buffer,
                create_contract_package_at_hash,
                create_contract_user_group,
                add_contract_version,
                disable_contract_version,
                call_contract,
                call_versioned_contract,
                get_named_arg_size,
                get_named_arg,
                remove_contract_user_group,
                provision_contract_user_group_uref,
                remove_contract_user_group_urefs,
                print,
                blake2b,
            },
            rem,
        ))
    }
}

impl Distribution<HostFunctionCosts> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> HostFunctionCosts {
        HostFunctionCosts {
            read_value: rng.gen(),
            read_value_local: rng.gen(),
            write: rng.gen(),
            write_local: rng.gen(),
            add: rng.gen(),
            add_local: rng.gen(),
            new_uref: rng.gen(),
            load_named_keys: rng.gen(),
            ret: rng.gen(),
            get_key: rng.gen(),
            has_key: rng.gen(),
            put_key: rng.gen(),
            remove_key: rng.gen(),
            revert: rng.gen(),
            is_valid_uref: rng.gen(),
            add_associated_key: rng.gen(),
            remove_associated_key: rng.gen(),
            update_associated_key: rng.gen(),
            set_action_threshold: rng.gen(),
            get_caller: rng.gen(),
            get_blocktime: rng.gen(),
            create_purse: rng.gen(),
            transfer_to_account: rng.gen(),
            transfer_from_purse_to_account: rng.gen(),
            transfer_from_purse_to_purse: rng.gen(),
            get_balance: rng.gen(),
            get_phase: rng.gen(),
            get_system_contract: rng.gen(),
            get_main_purse: rng.gen(),
            read_host_buffer: rng.gen(),
            create_contract_package_at_hash: rng.gen(),
            create_contract_user_group: rng.gen(),
            add_contract_version: rng.gen(),
            disable_contract_version: rng.gen(),
            call_contract: rng.gen(),
            call_versioned_contract: rng.gen(),
            get_named_arg_size: rng.gen(),
            get_named_arg: rng.gen(),
            remove_contract_user_group: rng.gen(),
            provision_contract_user_group_uref: rng.gen(),
            remove_contract_user_group_urefs: rng.gen(),
            print: rng.gen(),
            blake2b: rng.gen(),
        }
    }
}

#[cfg(any(feature = "gens", test))]
pub mod gens {
    use proptest::prelude::*;

    use super::{Cost, HostFunction, HostFunctionCosts};

    pub fn host_function_cost_arb<T: Copy + Arbitrary>() -> impl Strategy<Value = HostFunction<T>> {
        (any::<Cost>(), any::<T>()).prop_map(|(cost, arguments)| HostFunction::new(cost, arguments))
    }

    prop_compose! {
        pub fn host_function_costs_arb() (
            read_value in host_function_cost_arb(),
            read_value_local in host_function_cost_arb(),
            write in host_function_cost_arb(),
            write_local in host_function_cost_arb(),
            add in host_function_cost_arb(),
            add_local in host_function_cost_arb(),
            new_uref in host_function_cost_arb(),
            load_named_keys in host_function_cost_arb(),
            ret in host_function_cost_arb(),
            get_key in host_function_cost_arb(),
            has_key in host_function_cost_arb(),
            put_key in host_function_cost_arb(),
            remove_key in host_function_cost_arb(),
            revert in host_function_cost_arb(),
            is_valid_uref in host_function_cost_arb(),
            add_associated_key in host_function_cost_arb(),
            remove_associated_key in host_function_cost_arb(),
            update_associated_key in host_function_cost_arb(),
            set_action_threshold in host_function_cost_arb(),
            get_caller in host_function_cost_arb(),
            get_blocktime in host_function_cost_arb(),
            create_purse in host_function_cost_arb(),
            transfer_to_account in host_function_cost_arb(),
            transfer_from_purse_to_account in host_function_cost_arb(),
            transfer_from_purse_to_purse in host_function_cost_arb(),
            get_balance in host_function_cost_arb(),
            get_phase in host_function_cost_arb(),
            get_system_contract in host_function_cost_arb(),
            get_main_purse in host_function_cost_arb(),
            read_host_buffer in host_function_cost_arb(),
            create_contract_package_at_hash in host_function_cost_arb(),
            create_contract_user_group in host_function_cost_arb(),
            add_contract_version in host_function_cost_arb(),
            disable_contract_version in host_function_cost_arb(),
            call_contract in host_function_cost_arb(),
            call_versioned_contract in host_function_cost_arb(),
            get_named_arg_size in host_function_cost_arb(),
            get_named_arg in host_function_cost_arb(),
            remove_contract_user_group in host_function_cost_arb(),
            provision_contract_user_group_uref in host_function_cost_arb(),
            remove_contract_user_group_urefs in host_function_cost_arb(),
            print in host_function_cost_arb(),
            blake2b in host_function_cost_arb(),
        ) -> HostFunctionCosts {
            HostFunctionCosts {
                read_value,
                read_value_local,
                write,
                write_local,
                add,
                add_local,
                new_uref,
                load_named_keys,
                ret,
                get_key,
                has_key,
                put_key,
                remove_key,
                revert,
                is_valid_uref,
                add_associated_key,
                remove_associated_key,
                update_associated_key,
                set_action_threshold,
                get_caller,
                get_blocktime,
                create_purse,
                transfer_to_account,
                transfer_from_purse_to_account,
                transfer_from_purse_to_purse,
                get_balance,
                get_phase,
                get_system_contract,
                get_main_purse,
                read_host_buffer,
                create_contract_package_at_hash,
                create_contract_user_group,
                add_contract_version,
                disable_contract_version,
                call_contract,
                call_versioned_contract,
                get_named_arg_size,
                get_named_arg,
                remove_contract_user_group,
                provision_contract_user_group_uref,
                remove_contract_user_group_urefs,
                print,
                blake2b,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use casper_types::U512;

    use super::*;

    const COST: Cost = 42;
    const ARGUMENT_COSTS: [Cost; 3] = [123, 456, 789];
    const WEIGHTS: [Cost; 3] = [1000, 1100, 1200];

    #[test]
    fn calculate_gas_cost_for_host_function() {
        let host_function = HostFunction::new(COST, ARGUMENT_COSTS);
        let expected_cost = COST
            + (ARGUMENT_COSTS[0] * WEIGHTS[0])
            + (ARGUMENT_COSTS[1] * WEIGHTS[1])
            + (ARGUMENT_COSTS[2] * WEIGHTS[2]);
        assert_eq!(
            host_function.calculate_gas_cost(WEIGHTS),
            Gas::new(expected_cost.into())
        );
    }

    #[test]
    fn calculate_gas_cost_would_overflow() {
        let large_value = Cost::max_value();

        let host_function = HostFunction::new(
            large_value,
            [large_value, large_value, large_value, large_value],
        );

        let lhs =
            host_function.calculate_gas_cost([large_value, large_value, large_value, large_value]);

        let large_value = U512::from(large_value);
        let rhs = large_value + (U512::from(4) * large_value * large_value);

        assert_eq!(lhs, Gas::new(rhs));
    }
}

#[cfg(test)]
mod proptests {
    use proptest::prelude::*;

    use casper_types::bytesrepr;

    use super::*;

    type Signature = [Cost; 10];

    proptest! {
        #[test]
        fn test_host_function(host_function in gens::host_function_cost_arb::<Signature>()) {
            bytesrepr::test_serialization_roundtrip(&host_function);
        }

        #[test]
        fn test_host_function_costs(host_function_costs in gens::host_function_costs_arb()) {
            bytesrepr::test_serialization_roundtrip(&host_function_costs);
        }
    }
}
