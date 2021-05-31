use casper_types::bytesrepr::{self, FromBytes};

use crate::shared::system_config::auction_costs::AuctionCosts;

pub struct LegacyAuctionCosts(AuctionCosts);

impl From<LegacyAuctionCosts> for AuctionCosts {
    fn from(legacy_auction_costs: LegacyAuctionCosts) -> Self {
        legacy_auction_costs.0
    }
}

impl FromBytes for LegacyAuctionCosts {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (get_era_validators, rem) = FromBytes::from_bytes(bytes)?;
        let (read_seigniorage_recipients, rem) = FromBytes::from_bytes(rem)?;
        let (add_bid, rem) = FromBytes::from_bytes(rem)?;
        let (withdraw_bid, rem) = FromBytes::from_bytes(rem)?;
        let (delegate, rem) = FromBytes::from_bytes(rem)?;
        let (undelegate, rem) = FromBytes::from_bytes(rem)?;
        let (run_auction, rem) = FromBytes::from_bytes(rem)?;
        let (slash, rem) = FromBytes::from_bytes(rem)?;
        let (distribute, rem) = FromBytes::from_bytes(rem)?;
        let (withdraw_delegator_reward, rem) = FromBytes::from_bytes(rem)?;
        let (withdraw_validator_reward, rem) = FromBytes::from_bytes(rem)?;
        let (read_era_id, rem) = FromBytes::from_bytes(rem)?;
        let (activate_bid, rem) = FromBytes::from_bytes(rem)?;

        let auction_costs = AuctionCosts {
            get_era_validators,
            read_seigniorage_recipients,
            add_bid,
            withdraw_bid,
            delegate,
            undelegate,
            run_auction,
            slash,
            distribute,
            withdraw_delegator_reward,
            withdraw_validator_reward,
            read_era_id,
            activate_bid,
        };

        Ok((LegacyAuctionCosts(auction_costs), rem))
    }
}
