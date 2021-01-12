use std::convert::TryFrom;

use casper_execution_engine::shared::system_config::{
    auction_costs::AuctionCosts, mint_costs::MintCosts, proof_of_stake_costs::ProofOfStakeCosts,
    standard_payment_costs::StandardPaymentCosts, SystemConfig,
};

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

impl From<MintCosts> for ipc::ChainSpec_SystemConfig_MintCosts {
    fn from(mint_costs: MintCosts) -> Self {
        let mut pb_mint_costs = ipc::ChainSpec_SystemConfig_MintCosts::new();

        pb_mint_costs.set_mint(mint_costs.mint);
        pb_mint_costs.set_reduce_total_supply(mint_costs.reduce_total_supply);
        pb_mint_costs.set_create(mint_costs.create);
        pb_mint_costs.set_balance(mint_costs.balance);
        pb_mint_costs.set_transfer(mint_costs.transfer);
        pb_mint_costs.set_read_base_round_reward(mint_costs.read_base_round_reward);

        pb_mint_costs
    }
}

impl From<ipc::ChainSpec_SystemConfig_MintCosts> for MintCosts {
    fn from(pb_mint_costs: ipc::ChainSpec_SystemConfig_MintCosts) -> Self {
        Self {
            mint: pb_mint_costs.mint,
            reduce_total_supply: pb_mint_costs.reduce_total_supply,
            create: pb_mint_costs.create,
            balance: pb_mint_costs.balance,
            transfer: pb_mint_costs.transfer,
            read_base_round_reward: pb_mint_costs.read_base_round_reward,
        }
    }
}

impl From<StandardPaymentCosts> for ipc::ChainSpec_SystemConfig_StandardPaymentCosts {
    fn from(standard_payment_costs: StandardPaymentCosts) -> Self {
        let mut pb_standard_payment_costs = ipc::ChainSpec_SystemConfig_StandardPaymentCosts::new();

        pb_standard_payment_costs.set_pay(standard_payment_costs.pay);

        pb_standard_payment_costs
    }
}

impl From<ipc::ChainSpec_SystemConfig_StandardPaymentCosts> for StandardPaymentCosts {
    fn from(pb_standard_payment_costs: ipc::ChainSpec_SystemConfig_StandardPaymentCosts) -> Self {
        Self {
            pay: pb_standard_payment_costs.pay,
        }
    }
}

impl From<ProofOfStakeCosts> for ipc::ChainSpec_SystemConfig_ProofOfStakeCosts {
    fn from(proof_of_stake_costs: ProofOfStakeCosts) -> Self {
        let mut pb_standard_payment_costs = ipc::ChainSpec_SystemConfig_ProofOfStakeCosts::new();

        pb_standard_payment_costs.set_get_payment_purse(proof_of_stake_costs.get_payment_purse);
        pb_standard_payment_costs.set_set_refund_purse(proof_of_stake_costs.set_refund_purse);
        pb_standard_payment_costs.set_get_refund_purse(proof_of_stake_costs.get_refund_purse);
        pb_standard_payment_costs.set_finalize_payment(proof_of_stake_costs.finalize_payment);

        pb_standard_payment_costs
    }
}

impl From<ipc::ChainSpec_SystemConfig_ProofOfStakeCosts> for ProofOfStakeCosts {
    fn from(pb_proof_of_stake_costs: ipc::ChainSpec_SystemConfig_ProofOfStakeCosts) -> Self {
        Self {
            get_payment_purse: pb_proof_of_stake_costs.get_payment_purse,
            set_refund_purse: pb_proof_of_stake_costs.set_refund_purse,
            get_refund_purse: pb_proof_of_stake_costs.get_refund_purse,
            finalize_payment: pb_proof_of_stake_costs.finalize_payment,
        }
    }
}

impl From<SystemConfig> for ipc::ChainSpec_SystemConfig {
    fn from(system_config: SystemConfig) -> Self {
        let mut pb_system_config = ipc::ChainSpec_SystemConfig::new();

        pb_system_config.set_wasmless_transfer_cost(system_config.wasmless_transfer_cost());
        pb_system_config.set_auction_costs(system_config.auction_costs().clone().into());
        pb_system_config.set_mint_costs(system_config.mint_costs().clone().into());
        pb_system_config
            .set_proof_of_stake_costs(system_config.proof_of_stake_costs().clone().into());
        pb_system_config
            .set_standard_payment_costs(system_config.standard_payment_costs().clone().into());

        pb_system_config
    }
}

impl TryFrom<ipc::ChainSpec_SystemConfig> for SystemConfig {
    type Error = MappingError;

    fn try_from(mut pb_system_config: ipc::ChainSpec_SystemConfig) -> Result<Self, Self::Error> {
        Ok(SystemConfig::new(
            pb_system_config.get_wasmless_transfer_cost(),
            pb_system_config.take_auction_costs().into(),
            pb_system_config.take_mint_costs().into(),
            pb_system_config.take_proof_of_stake_costs().into(),
            pb_system_config.take_standard_payment_costs().into(),
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
