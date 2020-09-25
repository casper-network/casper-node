use bytesrepr::{FromBytes, ToBytes};
use casper_types::bytesrepr;
use rand::{distributions::Standard, prelude::Distribution, Rng};
use serde::{Deserialize, Serialize};

mod polynomial;

pub use polynomial::{Polynomial, PolynomialExpr};

/// Representation of a host function cost as ingredients of polynomials.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct HostFunctionCost(Polynomial);

impl HostFunctionCost {
    pub fn fixed(cost: u32) -> HostFunctionCost {
        HostFunctionCost(vec![PolynomialExpr::Coefficient(cost)])
    }

    pub fn into_inner(self) -> Polynomial {
        self.0
    }

    pub fn add_polynomial(&mut self, polynomial: PolynomialExpr) {
        self.0.push(polynomial)
    }
}

impl From<Polynomial> for HostFunctionCost {
    fn from(polynomial: Polynomial) -> Self {
        HostFunctionCost(polynomial)
    }
}

impl Distribution<HostFunctionCost> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> HostFunctionCost {
        HostFunctionCost::fixed(rng.gen())
    }
}

impl ToBytes for HostFunctionCost {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.0.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.0.serialized_length()
    }
}

impl FromBytes for HostFunctionCost {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (polynomials, bytes) = FromBytes::from_bytes(bytes)?;
        Ok((HostFunctionCost(polynomials), bytes))
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct HostFunctionCosts {
    pub read_value: HostFunctionCost,
    pub read_value_local: HostFunctionCost,
    pub write: HostFunctionCost,
    pub write_local: HostFunctionCost,
    pub add: HostFunctionCost,
    pub add_local: HostFunctionCost,
    pub new_uref: HostFunctionCost,
    pub load_named_keys: HostFunctionCost,
    pub ret: HostFunctionCost,
    pub get_key: HostFunctionCost,
    pub has_key: HostFunctionCost,
    pub put_key: HostFunctionCost,
    pub remove_key: HostFunctionCost,
    pub revert: HostFunctionCost,
    pub is_valid_uref: HostFunctionCost,
    pub add_associated_key: HostFunctionCost,
    pub remove_associated_key: HostFunctionCost,
    pub update_associated_key: HostFunctionCost,
    pub set_action_threshold: HostFunctionCost,
    pub get_caller: HostFunctionCost,
    pub get_blocktime: HostFunctionCost,
    pub create_purse: HostFunctionCost,
    pub transfer_to_account: HostFunctionCost,
    pub transfer_from_purse_to_account: HostFunctionCost,
    pub transfer_from_purse_to_purse: HostFunctionCost,
    pub get_balance: HostFunctionCost,
    pub get_phase: HostFunctionCost,
    pub get_system_contract: HostFunctionCost,
    pub get_main_purse: HostFunctionCost,
    pub read_host_buffer: HostFunctionCost,
    pub create_contract_package_at_hash: HostFunctionCost,
    pub create_contract_user_group: HostFunctionCost,
    pub add_contract_version: HostFunctionCost,
    pub disable_contract_version: HostFunctionCost,
    pub call_contract: HostFunctionCost,
    pub call_versioned_contract: HostFunctionCost,
    pub get_named_arg_size: HostFunctionCost,
    pub get_named_arg: HostFunctionCost,
    pub remove_contract_user_group: HostFunctionCost,
    pub provision_contract_user_group_uref: HostFunctionCost,
    pub remove_contract_user_group_urefs: HostFunctionCost,
    pub print: HostFunctionCost,
}

impl Default for HostFunctionCosts {
    fn default() -> Self {
        Self {
            read_value: HostFunctionCost::fixed(0),
            read_value_local: HostFunctionCost::fixed(0),
            write: HostFunctionCost::fixed(0),
            write_local: HostFunctionCost::fixed(0),
            add: HostFunctionCost::fixed(0),
            add_local: HostFunctionCost::fixed(0),
            new_uref: HostFunctionCost::fixed(0),
            load_named_keys: HostFunctionCost::fixed(0),
            ret: HostFunctionCost::fixed(0),
            get_key: HostFunctionCost::fixed(0),
            has_key: HostFunctionCost::fixed(0),
            put_key: HostFunctionCost::fixed(0),
            remove_key: HostFunctionCost::fixed(0),
            revert: HostFunctionCost::fixed(0),
            is_valid_uref: HostFunctionCost::fixed(0),
            add_associated_key: HostFunctionCost::fixed(0),
            remove_associated_key: HostFunctionCost::fixed(0),
            update_associated_key: HostFunctionCost::fixed(0),
            set_action_threshold: HostFunctionCost::fixed(0),
            get_caller: HostFunctionCost::fixed(0),
            get_blocktime: HostFunctionCost::fixed(0),
            create_purse: HostFunctionCost::fixed(0),
            transfer_to_account: HostFunctionCost::fixed(0),
            transfer_from_purse_to_account: HostFunctionCost::fixed(0),
            transfer_from_purse_to_purse: HostFunctionCost::fixed(0),
            get_balance: HostFunctionCost::fixed(0),
            get_phase: HostFunctionCost::fixed(0),
            get_system_contract: HostFunctionCost::fixed(0),
            get_main_purse: HostFunctionCost::fixed(0),
            read_host_buffer: HostFunctionCost::fixed(0),
            create_contract_package_at_hash: HostFunctionCost::fixed(0),
            create_contract_user_group: HostFunctionCost::fixed(0),
            add_contract_version: HostFunctionCost::fixed(0),
            disable_contract_version: HostFunctionCost::fixed(0),
            call_contract: HostFunctionCost::fixed(0),
            call_versioned_contract: HostFunctionCost::fixed(0),
            get_named_arg_size: HostFunctionCost::fixed(0),
            get_named_arg: HostFunctionCost::fixed(0),
            remove_contract_user_group: HostFunctionCost::fixed(0),
            provision_contract_user_group_uref: HostFunctionCost::fixed(0),
            remove_contract_user_group_urefs: HostFunctionCost::fixed(0),
            print: HostFunctionCost::fixed(0),
        }
    }
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
        }
    }
}

#[cfg(any(feature = "gens", test))]
pub mod gens {
    use proptest::prelude::*;

    use super::{HostFunctionCost, HostFunctionCosts};
    use crate::shared::host_function_costs::polynomial::gens::polynomial_arb;

    fn host_function_cost_arb() -> impl Strategy<Value = HostFunctionCost> {
        polynomial_arb().prop_map(HostFunctionCost::from)
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
            }
        }
    }
}
