#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::{distributions::Standard, prelude::Distribution, Rng};
use serde::{Deserialize, Serialize};

use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U64_SERIALIZED_LENGTH},
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
pub struct HostFunctionV2<T> {
    /// How much the user is charged for calling the host function.
    cost: Cost,
    /// Weights of the function arguments.
    arguments: T,
}

impl<T> Default for HostFunctionV2<T>
where
    T: Default,
{
    fn default() -> Self {
        Self {
            cost: DEFAULT_FIXED_COST,
            arguments: T::default(),
        }
    }
}

impl<T> HostFunctionV2<T> {
    /// Creates a new instance of `HostFunction` with a fixed call cost and argument weights.
    pub const fn new(cost: Cost, arguments: T) -> Self {
        Self { cost, arguments }
    }

    pub fn with_new_static_cost(self, cost: Cost) -> Self {
        Self {
            cost,
            arguments: self.arguments,
        }
    }

    /// Returns the base gas fee for calling the host function.
    pub fn cost(&self) -> Cost {
        self.cost
    }
}

impl<T> HostFunctionV2<T>
where
    T: Default,
{
    /// Creates a new fixed host function cost with argument weights of zero.
    pub fn fixed(cost: Cost) -> Self {
        Self {
            cost,
            ..Default::default()
        }
    }

    pub fn zero() -> Self {
        Self {
            cost: Default::default(),
            arguments: Default::default(),
        }
    }
}

impl<T> HostFunctionV2<T>
where
    T: AsRef<[Cost]>,
{
    /// Returns a slice containing the argument weights.
    pub fn arguments(&self) -> &[Cost] {
        self.arguments.as_ref()
    }

    /// Calculate gas cost for a host function
    pub fn calculate_gas_cost(&self, weights: T) -> Option<Gas> {
        let mut gas = Gas::new(self.cost);
        for (argument, weight) in self.arguments.as_ref().iter().zip(weights.as_ref()) {
            let lhs = Gas::new(*argument);
            let rhs = Gas::new(*weight);
            let product = lhs.checked_mul(rhs)?;
            gas = gas.checked_add(product)?;
        }
        Some(gas)
    }
}

#[cfg(any(feature = "testing", test))]
impl<T> Distribution<HostFunctionV2<T>> for Standard
where
    Standard: Distribution<T>,
    T: AsMut<[Cost]> + Default,
{
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> HostFunctionV2<T> {
        let cost = rng.gen::<u32>() as u64;
        let mut arguments = T::default();
        for arg in arguments.as_mut() {
            *arg = rng.gen::<u32>() as u64;
        }

        HostFunctionV2::new(cost, arguments)
    }
}

impl<T> ToBytes for HostFunctionV2<T>
where
    T: AsRef<[Cost]>,
{
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);
        ret.append(&mut self.cost.to_bytes()?);
        for value in self.arguments.as_ref().iter() {
            ret.append(&mut value.to_bytes()?);
        }
        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        self.cost.serialized_length() + (U64_SERIALIZED_LENGTH * self.arguments.as_ref().len())
    }
}

impl<T> FromBytes for HostFunctionV2<T>
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
/// An identifier that represents an unused argument.
const NOT_USED: Cost = 0;

/// An arbitrary default fixed cost for host functions that were not researched yet.
const DEFAULT_FIXED_COST: Cost = 200;

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
const DEFAULT_CREATE_ENTRYPOINT_SIZE_WEIGHT: Cost = 0;
const DEFAULT_CREATE_INPUT_SIZE_WEIGHT: Cost = 0;
const DEFAULT_CREATE_SEED_SIZE_WEIGHT: Cost = 0;

const DEFAULT_EMIT_COST: Cost = 200;
const DEFAULT_EMIT_TOPIC_SIZE_WEIGHT: Cost = 100;
const DEFAULT_EMIT_PAYLOAD_SIZE_HEIGHT: Cost = 100;

const DEFAULT_ENV_INFO_COST: Cost = 10_000;

/// Definition of a host function cost table.
#[derive(Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[serde(deny_unknown_fields)]
pub struct HostFunctionCostsV2 {
    /// Cost of calling the `read` host function.
    pub read: HostFunctionV2<[Cost; 6]>,
    /// Cost of calling the `write` host function.
    pub write: HostFunctionV2<[Cost; 5]>,
    /// Cost of calling the `remove` host function.
    pub remove: HostFunctionV2<[Cost; 3]>,
    /// Cost of calling the `copy_input` host function.
    pub copy_input: HostFunctionV2<[Cost; 2]>,
    /// Cost of calling the `ret` host function.
    pub ret: HostFunctionV2<[Cost; 2]>,
    /// Cost of calling the `create` host function.
    pub create: HostFunctionV2<[Cost; 10]>,
    /// Cost of calling the `transfer` host function.
    pub transfer: HostFunctionV2<[Cost; 3]>,
    /// Cost of calling the `env_balance` host function.
    pub env_balance: HostFunctionV2<[Cost; 4]>,
    /// Cost of calling the `upgrade` host function.
    pub upgrade: HostFunctionV2<[Cost; 6]>,
    /// Cost of calling the `call` host function.
    pub call: HostFunctionV2<[Cost; 9]>,
    /// Cost of calling the `print` host function.
    pub print: HostFunctionV2<[Cost; 2]>,
    /// Cost of calling the `emit` host function.
    pub emit: HostFunctionV2<[Cost; 4]>,
    /// Cost of calling the `env_info` host function.
    pub env_info: HostFunctionV2<[Cost; 2]>,
}

impl HostFunctionCostsV2 {
    pub fn zero() -> Self {
        Self {
            read: HostFunctionV2::zero(),
            write: HostFunctionV2::zero(),
            remove: HostFunctionV2::zero(),
            copy_input: HostFunctionV2::zero(),
            ret: HostFunctionV2::zero(),
            create: HostFunctionV2::zero(),
            transfer: HostFunctionV2::zero(),
            env_balance: HostFunctionV2::zero(),
            upgrade: HostFunctionV2::zero(),
            call: HostFunctionV2::zero(),
            print: HostFunctionV2::zero(),
            emit: HostFunctionV2::zero(),
            env_info: HostFunctionV2::zero(),
        }
    }
}

impl Default for HostFunctionCostsV2 {
    fn default() -> Self {
        Self {
            read: HostFunctionV2::new(
                DEFAULT_READ_COST,
                [
                    NOT_USED,
                    NOT_USED,
                    DEFAULT_READ_KEY_SIZE_WEIGHT,
                    NOT_USED,
                    NOT_USED,
                    NOT_USED,
                ],
            ),
            write: HostFunctionV2::new(
                DEFAULT_WRITE_COST,
                [
                    NOT_USED,
                    NOT_USED,
                    NOT_USED,
                    NOT_USED,
                    DEFAULT_WRITE_SIZE_WEIGHT,
                ],
            ),
            remove: HostFunctionV2::new(DEFAULT_REMOVE_COST, [NOT_USED, NOT_USED, NOT_USED]),
            copy_input: HostFunctionV2::new(
                DEFAULT_COPY_INPUT_COST,
                [NOT_USED, DEFAULT_COPY_INPUT_VALUE_SIZE_WEIGHT],
            ),
            ret: HostFunctionV2::new(DEFAULT_RET_COST, [NOT_USED, DEFAULT_RET_VALUE_SIZE_WEIGHT]),
            create: HostFunctionV2::new(
                DEFAULT_CREATE_COST,
                [
                    NOT_USED,
                    DEFAULT_CREATE_CODE_SIZE_WEIGHT,
                    NOT_USED,
                    NOT_USED,
                    DEFAULT_CREATE_ENTRYPOINT_SIZE_WEIGHT,
                    NOT_USED,
                    DEFAULT_CREATE_INPUT_SIZE_WEIGHT,
                    NOT_USED,
                    DEFAULT_CREATE_SEED_SIZE_WEIGHT,
                    NOT_USED,
                ],
            ),
            env_balance: HostFunctionV2::fixed(DEFAULT_ENV_BALANCE_COST),
            transfer: HostFunctionV2::new(DEFAULT_TRANSFER_COST, [NOT_USED, NOT_USED, NOT_USED]),
            upgrade: HostFunctionV2::new(
                DEFAULT_FIXED_COST,
                [NOT_USED, NOT_USED, NOT_USED, NOT_USED, NOT_USED, NOT_USED],
            ),
            call: HostFunctionV2::new(
                DEFAULT_CALL_COST,
                [
                    NOT_USED, NOT_USED, NOT_USED, NOT_USED, NOT_USED, NOT_USED, NOT_USED, NOT_USED,
                    NOT_USED,
                ],
            ),
            print: HostFunctionV2::new(DEFAULT_PRINT_COST, [NOT_USED, NOT_USED]),
            emit: HostFunctionV2::new(
                DEFAULT_EMIT_COST,
                [
                    NOT_USED,
                    DEFAULT_EMIT_TOPIC_SIZE_WEIGHT,
                    NOT_USED,
                    DEFAULT_EMIT_PAYLOAD_SIZE_HEIGHT,
                ],
            ),
            env_info: HostFunctionV2::new(DEFAULT_ENV_INFO_COST, [NOT_USED, NOT_USED]),
        }
    }
}

impl ToBytes for HostFunctionCostsV2 {
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
    }
}

impl FromBytes for HostFunctionCostsV2 {
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
        Ok((
            HostFunctionCostsV2 {
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
            },
            rem,
        ))
    }
}

#[cfg(any(feature = "testing", test))]
impl Distribution<HostFunctionCostsV2> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> HostFunctionCostsV2 {
        HostFunctionCostsV2 {
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
        }
    }
}

#[doc(hidden)]
#[cfg(any(feature = "gens", test))]
pub mod gens {
    use proptest::prelude::*;

    use super::*;

    #[allow(unused)]
    pub fn host_function_cost_v2_arb<const N: usize>(
    ) -> impl Strategy<Value = HostFunctionV2<[Cost; N]>> {
        (any::<u64>(), any::<[u64; N]>())
            .prop_map(|(cost, arguments)| HostFunctionV2::new(cost, arguments))
    }

    prop_compose! {
        pub fn host_function_costs_v2_arb() (
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
        ) -> HostFunctionCostsV2 {
            HostFunctionCostsV2 {
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
                env_info
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Gas, U512};

    use super::*;

    const COST: Cost = 42;
    const ARGUMENT_COSTS: [Cost; 3] = [123, 456, 789];
    const WEIGHTS: [u64; 3] = [1000, 1000, 1000];

    #[test]
    fn calculate_gas_cost_for_host_function() {
        let host_function = HostFunctionV2::new(COST, ARGUMENT_COSTS);
        let expected_cost = COST
            + (ARGUMENT_COSTS[0] * Cost::from(WEIGHTS[0]))
            + (ARGUMENT_COSTS[1] * Cost::from(WEIGHTS[1]))
            + (ARGUMENT_COSTS[2] * Cost::from(WEIGHTS[2]));
        assert_eq!(
            host_function.calculate_gas_cost(WEIGHTS),
            Some(Gas::new(expected_cost))
        );
    }

    #[test]
    fn calculate_gas_cost_would_overflow() {
        let large_value = Cost::MAX;

        let host_function = HostFunctionV2::new(
            large_value,
            [large_value, large_value, large_value, large_value],
        );

        let lhs =
            host_function.calculate_gas_cost([large_value, large_value, large_value, large_value]);

        let large_value = U512::from(large_value);
        let rhs = large_value + (U512::from(4) * large_value * large_value);

        assert_eq!(lhs, Some(Gas::new(rhs)));
    }
    #[test]
    fn calculate_large_gas_cost() {
        let hf = HostFunctionV2::new(1, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        assert_eq!(
            hf.calculate_gas_cost([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
            Some(Gas::new(
                1 + (1 + 2 * 2 + 3 * 3 + 4 * 4 + 5 * 5 + 6 * 6 + 7 * 7 + 8 * 8 + 9 * 9 + 10 * 10)
            ))
        );
    }
}

#[cfg(test)]
mod proptests {
    use proptest::prelude::*;

    use crate::bytesrepr;

    use super::*;

    proptest! {
        #[test]
        fn test_host_function(host_function in gens::host_function_cost_v2_arb::<10>()) {
            bytesrepr::test_serialization_roundtrip(&host_function);
        }

        #[test]
        fn test_host_function_costs(host_function_costs in gens::host_function_costs_v2_arb()) {
            bytesrepr::test_serialization_roundtrip(&host_function_costs);
        }
    }
}
