#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::{distributions::Standard, prelude::Distribution, Rng};
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes},
    Gas,
};

/// Representation of argument's cost.
pub type Cost = u64;

/// Representation of a host function cost.
///
/// The total gas cost is equal to `cost` + sum of each argument weight multiplied by the byte size
/// of the data.
///
/// NOTE: This is duplicating the `HostFunction` struct from the `casper-types` crate
/// but to avoid changing the public API of that crate, we are creating a new struct
/// with the same name and fields.
///
/// There is some opportunity to unify the code to turn `HostFunction` into a generic struct
/// that generalizes over the cost type, but that would require a lot of work and
/// is not worth it at this time.
#[derive(Copy, Clone, PartialEq, Eq, Deserialize, Serialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[serde(deny_unknown_fields)]
pub struct HostFFIFunctionCost {
    /// How much the user is charged for calling the host function.
    base_cost: Cost,
    /// How much the user is charged for each byte of the input data.
    per_byte: Cost,
}

impl Default for HostFFIFunctionCost {
    fn default() -> Self {
        Self {
            base_cost: DEFAULT_FIXED_COST,
            per_byte: DEFAULT_PER_BYTES_COST,
        }
    }
}

impl HostFFIFunctionCost {
    /// Creates a new instance of `HostFFIFunctionCost`.
    pub const fn new(base_cost: Cost, per_byte: Cost) -> Self {
        Self {
            base_cost,
            per_byte,
        }
    }

    /// Creates a new fixed host function cost with argument weights of zero.
    pub fn fixed(base_cost: Cost) -> Self {
        Self {
            base_cost,
            ..Default::default()
        }
    }

    pub fn zero() -> Self {
        Self {
            base_cost: Default::default(),
            per_byte: Default::default(),
        }
    }

    pub fn with_new_base_cost(self, base_cost: Cost) -> Self {
        Self {
            base_cost,
            per_byte: self.per_byte,
        }
    }

    /// Returns the base gas fee for calling the host function.
    pub fn base_cost(&self) -> Cost {
        self.base_cost
    }

    /// Calculate gas cost for a host function
    pub fn calculate_gas_cost(&self, number_of_bytes: u64) -> Option<Gas> {
        let mut gas = Gas::new(self.base_cost);
        let lhs = Gas::new(self.per_byte);
        let rhs = Gas::new(number_of_bytes);
        let product = lhs.checked_mul(rhs)?;
        gas = gas.checked_add(product)?;
        Some(gas)
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<HostFFIFunctionCost> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> HostFFIFunctionCost {
        let cost = rng.gen::<u32>() as u64;
        let per_byte = rng.gen::<u32>() as u64;
        HostFFIFunctionCost::new(cost, per_byte)
    }
}

impl ToBytes for HostFFIFunctionCost {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);
        ret.append(&mut self.base_cost.to_bytes()?);
        ret.append(&mut self.per_byte.to_bytes()?);
        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        self.base_cost.serialized_length() + self.per_byte.serialized_length()
    }
}

impl FromBytes for HostFFIFunctionCost {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (base_cost, bytes) = FromBytes::from_bytes(bytes)?;
        let (per_byte, bytes) = FromBytes::from_bytes(bytes)?;

        Ok((
            Self {
                base_cost,
                per_byte,
            },
            bytes,
        ))
    }
}
/// An identifier that represents an unused argument.
const NOT_USED: Cost = 0;

/// An arbitrary default fixed cost for host functions that were not researched yet.
const DEFAULT_FIXED_COST: Cost = 200;
const DEFAULT_PER_BYTES_COST: Cost = 0;
const DEFAULT_CALL_COST: u64 = 10_000;
const DEFAULT_ENV_BALANCE_COST: u64 = 100;

const DEFAULT_PRINT_COST: Cost = 100;

const DEFAULT_READ_COST: Cost = 1_000;
const DEFAULT_READ_KEY_SIZE_WEIGHT: Cost = 100;

const DEFAULT_RET_COST: Cost = 300;
const DEFAULT_RET_VALUE_SIZE_WEIGHT: Cost = 100;

const DEFAULT_TRANSFER_COST: Cost = 2_500_000_000;

const DEFAULT_WRITE_COST: Cost = 25_000;
const DEFAULT_WRITE_SIZE_WEIGHT: Cost = 100_000;

const DEFAULT_REMOVE_COST: Cost = 15_000;

const DEFAULT_COPY_INPUT_COST: Cost = 300;
const DEFAULT_COPY_INPUT_VALUE_SIZE_WEIGHT: Cost = 0;

const DEFAULT_CREATE_COST: Cost = 0;
const DEFAULT_CREATE_CODE_SIZE_WEIGHT: Cost = 0;

const DEFAULT_EMIT_COST: Cost = 200;
const DEFAULT_EMIT_PAYLOAD_SIZE_HEIGHT: Cost = 100;

const DEFAULT_ENV_INFO_COST: Cost = 10_000;

const DEFAULT_GENERIC_HASH_COST: Cost = 0;
const DEFAULT_GENERIC_HASH_SIZE_WEIGHT: Cost = 0;

const DEFAULT_RECOVER_SECP256K1_COST: Cost = 0;
const DEFAULT_RECOVER_SECP256K1_SIZE_WEIGHT: Cost = 0;
const DEFAULT_ALT_BN128_ADD_COST: Cost = 1_000_000;
const DEFAULT_ALT_BN128_MUL_COST: Cost = 1_000_000;
const DEFAULT_ALT_BN128_PAIRING_COST: Cost = 1_000_000;

/// Definition of a host function cost table.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[serde(deny_unknown_fields)]
pub struct HostFFIFunctionCosts {
    /// Cost of calling the `read` host function.
    pub read: HostFFIFunctionCost,
    /// Cost of calling the `write` host function.
    pub write: HostFFIFunctionCost,
    /// Cost of calling the `remove` host function.
    pub remove: HostFFIFunctionCost,
    /// Cost of calling the `copy_input` host function.
    pub copy_input: HostFFIFunctionCost,
    /// Cost of calling the `ret` host function.
    pub ret: HostFFIFunctionCost,
    /// Cost of calling the `create` host function.
    pub create: HostFFIFunctionCost,
    /// Cost of calling the `transfer` host function.
    pub transfer: HostFFIFunctionCost,
    /// Cost of calling the `env_balance` host function.
    pub env_balance: HostFFIFunctionCost,
    /// Cost of calling the `upgrade` host function.
    pub upgrade: HostFFIFunctionCost,
    /// Cost of calling the `call` host function.
    pub call: HostFFIFunctionCost,
    /// Cost of calling the `print` host function.
    pub print: HostFFIFunctionCost,
    /// Cost of calling the `emit` host function.
    pub emit: HostFFIFunctionCost,
    /// Cost of calling the `env_info` host function.
    pub env_info: HostFFIFunctionCost,
    /// Cost of calling the `generic_hash` host function.
    pub generic_hash: HostFFIFunctionCost,
    /// Cost of calling the `` host function.
    pub recover_secp256k1: HostFFIFunctionCost,
    /// Cost of calling the `alt_bn128_add` host function.
    pub alt_bn128_add: HostFFIFunctionCost,
    /// Cost of calling the `alt_bn128_mul` host function.
    pub alt_bn128_mul: HostFFIFunctionCost,
    /// Cost of calling the `alt_bn128_pairing` host function.
    pub alt_bn128_pairing: HostFFIFunctionCost,
}

impl HostFFIFunctionCosts {
    pub fn zero() -> Self {
        Self {
            read: HostFFIFunctionCost::zero(),
            write: HostFFIFunctionCost::zero(),
            remove: HostFFIFunctionCost::zero(),
            copy_input: HostFFIFunctionCost::zero(),
            ret: HostFFIFunctionCost::zero(),
            create: HostFFIFunctionCost::zero(),
            transfer: HostFFIFunctionCost::zero(),
            env_balance: HostFFIFunctionCost::zero(),
            upgrade: HostFFIFunctionCost::zero(),
            call: HostFFIFunctionCost::zero(),
            print: HostFFIFunctionCost::zero(),
            emit: HostFFIFunctionCost::zero(),
            env_info: HostFFIFunctionCost::zero(),
            generic_hash: HostFFIFunctionCost::zero(),
            recover_secp256k1: HostFFIFunctionCost::zero(),
            alt_bn128_add: HostFFIFunctionCost::zero(),
            alt_bn128_mul: HostFFIFunctionCost::zero(),
            alt_bn128_pairing: HostFFIFunctionCost::zero(),
        }
    }
}

impl Default for HostFFIFunctionCosts {
    fn default() -> Self {
        Self {
            read: HostFFIFunctionCost::new(DEFAULT_READ_COST, DEFAULT_READ_KEY_SIZE_WEIGHT),
            write: HostFFIFunctionCost::new(DEFAULT_WRITE_COST, DEFAULT_WRITE_SIZE_WEIGHT),
            remove: HostFFIFunctionCost::new(DEFAULT_REMOVE_COST, NOT_USED),
            copy_input: HostFFIFunctionCost::new(
                DEFAULT_COPY_INPUT_COST,
                DEFAULT_COPY_INPUT_VALUE_SIZE_WEIGHT,
            ),
            ret: HostFFIFunctionCost::new(DEFAULT_RET_COST, DEFAULT_RET_VALUE_SIZE_WEIGHT),
            create: HostFFIFunctionCost::new(DEFAULT_CREATE_COST, DEFAULT_CREATE_CODE_SIZE_WEIGHT),
            env_balance: HostFFIFunctionCost::fixed(DEFAULT_ENV_BALANCE_COST),
            transfer: HostFFIFunctionCost::new(DEFAULT_TRANSFER_COST, NOT_USED),
            upgrade: HostFFIFunctionCost::new(DEFAULT_FIXED_COST, NOT_USED),
            call: HostFFIFunctionCost::new(DEFAULT_CALL_COST, NOT_USED),
            print: HostFFIFunctionCost::new(DEFAULT_PRINT_COST, NOT_USED),
            emit: HostFFIFunctionCost::new(DEFAULT_EMIT_COST, DEFAULT_EMIT_PAYLOAD_SIZE_HEIGHT),
            env_info: HostFFIFunctionCost::new(DEFAULT_ENV_INFO_COST, NOT_USED),
            generic_hash: HostFFIFunctionCost::new(
                DEFAULT_GENERIC_HASH_COST,
                DEFAULT_GENERIC_HASH_SIZE_WEIGHT,
            ),
            recover_secp256k1: HostFFIFunctionCost::new(
                DEFAULT_RECOVER_SECP256K1_COST,
                DEFAULT_RECOVER_SECP256K1_SIZE_WEIGHT,
            ),
            alt_bn128_add: HostFFIFunctionCost::new(DEFAULT_ALT_BN128_ADD_COST, NOT_USED),
            alt_bn128_mul: HostFFIFunctionCost::new(DEFAULT_ALT_BN128_MUL_COST, NOT_USED),
            alt_bn128_pairing: HostFFIFunctionCost::new(DEFAULT_ALT_BN128_PAIRING_COST, NOT_USED),
        }
    }
}

impl ToBytes for HostFFIFunctionCosts {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);
        ret.append(&mut self.read.to_bytes()?);
        ret.append(&mut self.write.to_bytes()?);
        ret.append(&mut self.remove.to_bytes()?);
        ret.append(&mut self.copy_input.to_bytes()?);
        ret.append(&mut self.ret.to_bytes()?);
        ret.append(&mut self.create.to_bytes()?);
        ret.append(&mut self.transfer.to_bytes()?);
        ret.append(&mut self.env_balance.to_bytes()?);
        ret.append(&mut self.upgrade.to_bytes()?);
        ret.append(&mut self.call.to_bytes()?);
        ret.append(&mut self.print.to_bytes()?);
        ret.append(&mut self.emit.to_bytes()?);
        ret.append(&mut self.env_info.to_bytes()?);
        ret.append(&mut self.generic_hash.to_bytes()?);
        ret.append(&mut self.recover_secp256k1.to_bytes()?);
        ret.append(&mut self.alt_bn128_add.to_bytes()?);
        ret.append(&mut self.alt_bn128_mul.to_bytes()?);
        ret.append(&mut self.alt_bn128_pairing.to_bytes()?);
        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        self.read.serialized_length()
            + self.write.serialized_length()
            + self.remove.serialized_length()
            + self.copy_input.serialized_length()
            + self.ret.serialized_length()
            + self.create.serialized_length()
            + self.transfer.serialized_length()
            + self.env_balance.serialized_length()
            + self.upgrade.serialized_length()
            + self.call.serialized_length()
            + self.print.serialized_length()
            + self.emit.serialized_length()
            + self.env_info.serialized_length()
            + self.generic_hash.serialized_length()
            + self.recover_secp256k1.serialized_length()
            + self.alt_bn128_add.serialized_length()
            + self.alt_bn128_mul.serialized_length()
            + self.alt_bn128_pairing.serialized_length()
    }
}

impl FromBytes for HostFFIFunctionCosts {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (read, rem) = FromBytes::from_bytes(bytes)?;
        let (write, rem) = FromBytes::from_bytes(rem)?;
        let (remove, rem) = FromBytes::from_bytes(rem)?;
        let (copy_input, rem) = FromBytes::from_bytes(rem)?;
        let (ret, rem) = FromBytes::from_bytes(rem)?;
        let (create, rem) = FromBytes::from_bytes(rem)?;
        let (transfer, rem) = FromBytes::from_bytes(rem)?;
        let (env_balance, rem) = FromBytes::from_bytes(rem)?;
        let (upgrade, rem) = FromBytes::from_bytes(rem)?;
        let (call, rem) = FromBytes::from_bytes(rem)?;
        let (print, rem) = FromBytes::from_bytes(rem)?;
        let (emit, rem) = FromBytes::from_bytes(rem)?;
        let (env_info, rem) = FromBytes::from_bytes(rem)?;
        let (generic_hash, rem) = FromBytes::from_bytes(rem)?;
        let (recover_secp256k1, rem) = FromBytes::from_bytes(rem)?;
        let (alt_bn128_add, rem) = FromBytes::from_bytes(rem)?;
        let (alt_bn128_mul, rem) = FromBytes::from_bytes(rem)?;
        let (alt_bn128_pairing, rem) = FromBytes::from_bytes(rem)?;
        Ok((
            HostFFIFunctionCosts {
                read,
                write,
                remove,
                copy_input,
                ret,
                create,
                transfer,
                env_balance,
                upgrade,
                call,
                print,
                emit,
                env_info,
                generic_hash,
                recover_secp256k1,
                alt_bn128_add,
                alt_bn128_mul,
                alt_bn128_pairing,
            },
            rem,
        ))
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<HostFFIFunctionCosts> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> HostFFIFunctionCosts {
        HostFFIFunctionCosts {
            read: rng.gen(),
            write: rng.gen(),
            remove: rng.gen(),
            copy_input: rng.gen(),
            ret: rng.gen(),
            create: rng.gen(),
            transfer: rng.gen(),
            env_balance: rng.gen(),
            upgrade: rng.gen(),
            call: rng.gen(),
            print: rng.gen(),
            emit: rng.gen(),
            env_info: rng.gen(),
            generic_hash: rng.gen(),
            recover_secp256k1: rng.gen(),
            alt_bn128_add: rng.gen(),
            alt_bn128_mul: rng.gen(),
            alt_bn128_pairing: rng.gen(),
        }
    }
}

#[doc(hidden)]
#[cfg(any(feature = "gens", test))]
pub mod gens {
    use proptest::prelude::*;

    use super::*;

    #[allow(unused)]
    pub fn host_function_cost_v2_arb() -> impl Strategy<Value = HostFFIFunctionCost> {
        (any::<u64>(), any::<u64>())
            .prop_map(|(cost, arguments)| HostFFIFunctionCost::new(cost, arguments))
    }

    prop_compose! {
        pub fn host_ffi_opt_costs_arb() (
            read in host_function_cost_v2_arb(),
            write in host_function_cost_v2_arb(),
            remove in host_function_cost_v2_arb(),
            copy_input in host_function_cost_v2_arb(),
            ret in host_function_cost_v2_arb(),
            create in host_function_cost_v2_arb(),
            transfer in host_function_cost_v2_arb(),
            env_balance in host_function_cost_v2_arb(),
            upgrade in host_function_cost_v2_arb(),
            call in host_function_cost_v2_arb(),
            print in host_function_cost_v2_arb(),
            emit in host_function_cost_v2_arb(),
            env_info in host_function_cost_v2_arb(),
            generic_hash in host_function_cost_v2_arb(),
            recover_secp256k1 in host_function_cost_v2_arb(),
                        alt_bn128_add in host_function_cost_v2_arb(),
            alt_bn128_mul in host_function_cost_v2_arb(),
            alt_bn128_pairing in host_function_cost_v2_arb(),
        ) -> HostFFIFunctionCosts {
            HostFFIFunctionCosts {
                read,
                write,
                remove,
                copy_input,
                ret,
                create,
                transfer,
                env_balance,
                upgrade,
                call,
                print,
                emit,
                env_info,
                generic_hash,
                recover_secp256k1,
                                alt_bn128_add,
                alt_bn128_mul,
                alt_bn128_pairing
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Gas, U512};

    use super::*;

    const COST: Cost = 42;

    #[test]
    fn calculate_gas_cost_for_host_function() {
        let host_function = HostFFIFunctionCost::new(COST, 789);
        let expected_cost = COST + 789 * 155;
        assert_eq!(
            host_function.calculate_gas_cost(155),
            Some(Gas::new(expected_cost))
        );
    }

    #[test]
    fn calculate_gas_cost_would_overflow() {
        let large_value = Cost::MAX;

        let host_function = HostFFIFunctionCost::new(large_value, large_value);

        let lhs = host_function.calculate_gas_cost(large_value);

        let large_value = U512::from(large_value);
        let rhs = large_value + (large_value * large_value);

        assert_eq!(lhs, Some(Gas::new(rhs)));
    }
    #[test]
    fn calculate_large_gas_cost() {
        let hf = HostFFIFunctionCost::new(1, 55);
        assert_eq!(hf.calculate_gas_cost(17), Some(Gas::new(1 + (17 * 55))));
    }
}

#[cfg(test)]
mod proptests {
    use proptest::prelude::*;

    use crate::bytesrepr;

    use super::*;

    proptest! {
        #[test]
        fn test_host_function(host_function in gens::host_function_cost_v2_arb()) {
            bytesrepr::test_serialization_roundtrip(&host_function);
        }

        #[test]
        fn test_host_function_costs(host_function_costs in gens::host_ffi_opt_costs_arb()) {
            bytesrepr::test_serialization_roundtrip(&host_function_costs);
        }
    }
}
