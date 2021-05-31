use casper_types::bytesrepr::{self, FromBytes};

use crate::shared::system_config::mint_costs::MintCosts;

pub struct LegacyMintCosts(MintCosts);

impl From<LegacyMintCosts> for MintCosts {
    fn from(legacy_mint_costs: LegacyMintCosts) -> Self {
        legacy_mint_costs.0
    }
}

impl FromBytes for LegacyMintCosts {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (mint, rem) = FromBytes::from_bytes(bytes)?;
        let (reduce_total_supply, rem) = FromBytes::from_bytes(rem)?;
        let (create, rem) = FromBytes::from_bytes(rem)?;
        let (balance, rem) = FromBytes::from_bytes(rem)?;
        let (transfer, rem) = FromBytes::from_bytes(rem)?;
        let (read_base_round_reward, rem) = FromBytes::from_bytes(rem)?;
        let legacy_mint_costs = MintCosts {
            mint,
            reduce_total_supply,
            create,
            balance,
            transfer,
            read_base_round_reward,
        };
        Ok((LegacyMintCosts(legacy_mint_costs), rem))
    }
}
