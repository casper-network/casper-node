use std::convert::TryFrom;

use casper_execution_engine::shared::system_config::{auction_costs::AuctionCosts, SystemConfig};

use crate::engine_server::{ipc, mappings::MappingError};

impl From<AuctionCosts> for ipc::ChainSpec_SystemConfig_AuctionCosts {
    fn from(auction_costs: AuctionCosts) -> Self {
        let mut pb_auction_costs = ipc::ChainSpec_SystemConfig_AuctionCosts::new();

        pb_auction_costs.set_get_era_validators(auction_costs.get_era_validators);
        pb_auction_costs.set_read_seigniorage_recipients(auction_costs.read_seigniorage_recipients);
        pb_auction_costs.set_add_bid(auction_costs.add_bid);
        pb_auction_costs.set_withdraw_bid(auction_costs.withdraw_bid);
        pb_auction_costs.set_delegate(auction_costs.delegate);
        pb_auction_costs.set_undelegate(auction_costs.undelegate);
        pb_auction_costs.set_run_auction(auction_costs.run_auction);
        pb_auction_costs.set_slash(auction_costs.slash);
        pb_auction_costs.set_distribute(auction_costs.distribute);
        pb_auction_costs.set_withdraw_delegator_reward(auction_costs.withdraw_delegator_reward);
        pb_auction_costs.set_withdraw_validator_reward(auction_costs.withdraw_validator_reward);
        pb_auction_costs.set_read_era_id(auction_costs.read_era_id);

        pb_auction_costs
    }
}

impl From<ipc::ChainSpec_SystemConfig_AuctionCosts> for AuctionCosts {
    fn from(pb_system_config: ipc::ChainSpec_SystemConfig_AuctionCosts) -> Self {
        Self {
            get_era_validators: pb_system_config.get_era_validators,
            read_seigniorage_recipients: pb_system_config.read_seigniorage_recipients,
            add_bid: pb_system_config.add_bid,
            withdraw_bid: pb_system_config.withdraw_bid,
            delegate: pb_system_config.delegate,
            undelegate: pb_system_config.undelegate,
            run_auction: pb_system_config.run_auction,
            slash: pb_system_config.slash,
            distribute: pb_system_config.distribute,
            withdraw_delegator_reward: pb_system_config.withdraw_delegator_reward,
            withdraw_validator_reward: pb_system_config.withdraw_validator_reward,
            read_era_id: pb_system_config.read_era_id,
        }
    }
}

impl From<SystemConfig> for ipc::ChainSpec_SystemConfig {
    fn from(system_config: SystemConfig) -> Self {
        let mut pb_system_config = ipc::ChainSpec_SystemConfig::new();

        pb_system_config.set_wasmless_transfer_cost(system_config.wasmless_transfer_cost());
        pb_system_config.set_auction_costs(system_config.auction_costs().clone().into());

        pb_system_config
    }
}

impl TryFrom<ipc::ChainSpec_SystemConfig> for SystemConfig {
    type Error = MappingError;

    fn try_from(mut pb_system_config: ipc::ChainSpec_SystemConfig) -> Result<Self, Self::Error> {
        Ok(SystemConfig::new(
            pb_system_config.get_wasmless_transfer_cost(),
            pb_system_config.take_auction_costs().into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use proptest::proptest;

    use casper_execution_engine::shared::system_config::{gens, SystemConfig};

    use super::*;
    use crate::engine_server::mappings::test_utils;

    proptest! {
        #[test]
        fn round_trip(system_config in gens::system_config_arb()) {
            test_utils::protobuf_round_trip::<SystemConfig, ipc::ChainSpec_SystemConfig>(system_config);
        }
    }
}
