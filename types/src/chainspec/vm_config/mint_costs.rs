//! Costs of the mint system contract.
#[cfg(feature = "datasize")]
use datasize::DataSize;
use rand::{distributions::Standard, prelude::*, Rng};
use serde::{Deserialize, Serialize};

use crate::bytesrepr::{self, FromBytes, ToBytes};

/// Default cost of the `mint` mint entry point.
pub const DEFAULT_MINT_COST: u32 = 2_500_000_000;
/// Default cost of the `reduce_total_supply` mint entry point.
pub const DEFAULT_REDUCE_TOTAL_SUPPLY_COST: u32 = 10_000;
/// Default cost of the `create` mint entry point.
pub const DEFAULT_CREATE_COST: u32 = 2_500_000_000;
/// Default cost of the `balance` mint entry point.
pub const DEFAULT_BALANCE_COST: u32 = 10_000;
/// Default cost of the `transfer` mint entry point.
pub const DEFAULT_TRANSFER_COST: u32 = 10_000;
/// Default cost of the `read_base_round_reward` mint entry point.
pub const DEFAULT_READ_BASE_ROUND_REWARD_COST: u32 = 10_000;
/// Default cost of the `mint_into_existing_purse` mint entry point.
pub const DEFAULT_MINT_INTO_EXISTING_PURSE_COST: u32 = 2_500_000_000;

/// Description of the costs of calling mint entry points.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[serde(deny_unknown_fields)]
pub struct MintCosts {
    /// Cost of calling the `mint` entry point.
    pub mint: u32,
    /// Cost of calling the `reduce_total_supply` entry point.
    pub reduce_total_supply: u32,
    /// Cost of calling the `create` entry point.
    pub create: u32,
    /// Cost of calling the `balance` entry point.
    pub balance: u32,
    /// Cost of calling the `transfer` entry point.
    pub transfer: u32,
    /// Cost of calling the `read_base_round_reward` entry point.
    pub read_base_round_reward: u32,
    /// Cost of calling the `mint_into_existing_purse` entry point.
    pub mint_into_existing_purse: u32,
}

impl Default for MintCosts {
    fn default() -> Self {
        Self {
            mint: DEFAULT_MINT_COST,
            reduce_total_supply: DEFAULT_REDUCE_TOTAL_SUPPLY_COST,
            create: DEFAULT_CREATE_COST,
            balance: DEFAULT_BALANCE_COST,
            transfer: DEFAULT_TRANSFER_COST,
            read_base_round_reward: DEFAULT_READ_BASE_ROUND_REWARD_COST,
            mint_into_existing_purse: DEFAULT_MINT_INTO_EXISTING_PURSE_COST,
        }
    }
}

impl ToBytes for MintCosts {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut ret = bytesrepr::unchecked_allocate_buffer(self);

        let Self {
            mint,
            reduce_total_supply,
            create,
            balance,
            transfer,
            read_base_round_reward,
            mint_into_existing_purse,
        } = self;

        ret.append(&mut mint.to_bytes()?);
        ret.append(&mut reduce_total_supply.to_bytes()?);
        ret.append(&mut create.to_bytes()?);
        ret.append(&mut balance.to_bytes()?);
        ret.append(&mut transfer.to_bytes()?);
        ret.append(&mut read_base_round_reward.to_bytes()?);
        ret.append(&mut mint_into_existing_purse.to_bytes()?);

        Ok(ret)
    }

    fn serialized_length(&self) -> usize {
        let Self {
            mint,
            reduce_total_supply,
            create,
            balance,
            transfer,
            read_base_round_reward,
            mint_into_existing_purse,
        } = self;

        mint.serialized_length()
            + reduce_total_supply.serialized_length()
            + create.serialized_length()
            + balance.serialized_length()
            + transfer.serialized_length()
            + read_base_round_reward.serialized_length()
            + mint_into_existing_purse.serialized_length()
    }
}

impl FromBytes for MintCosts {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (mint, rem) = FromBytes::from_bytes(bytes)?;
        let (reduce_total_supply, rem) = FromBytes::from_bytes(rem)?;
        let (create, rem) = FromBytes::from_bytes(rem)?;
        let (balance, rem) = FromBytes::from_bytes(rem)?;
        let (transfer, rem) = FromBytes::from_bytes(rem)?;
        let (read_base_round_reward, rem) = FromBytes::from_bytes(rem)?;
        let (mint_into_existing_purse, rem) = FromBytes::from_bytes(rem)?;

        Ok((
            Self {
                mint,
                reduce_total_supply,
                create,
                balance,
                transfer,
                read_base_round_reward,
                mint_into_existing_purse,
            },
            rem,
        ))
    }
}

impl Distribution<MintCosts> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> MintCosts {
        MintCosts {
            mint: rng.gen(),
            reduce_total_supply: rng.gen(),
            create: rng.gen(),
            balance: rng.gen(),
            transfer: rng.gen(),
            read_base_round_reward: rng.gen(),
            mint_into_existing_purse: rng.gen(),
        }
    }
}

#[doc(hidden)]
#[cfg(any(feature = "gens", test))]
pub mod gens {
    use proptest::{num, prop_compose};

    use super::MintCosts;

    prop_compose! {
        pub fn mint_costs_arb()(
            mint in num::u32::ANY,
            reduce_total_supply in num::u32::ANY,
            create in num::u32::ANY,
            balance in num::u32::ANY,
            transfer in num::u32::ANY,
            read_base_round_reward in num::u32::ANY,
            mint_into_existing_purse in num::u32::ANY,
        ) -> MintCosts {
            MintCosts {
                mint,
                reduce_total_supply,
                create,
                balance,
                transfer,
                read_base_round_reward,
                mint_into_existing_purse,
            }
        }
    }
}
