use std::collections::BTreeSet;

use num_rational::Ratio;

use casper_types::{
    ConsensusProtocolName, FeeHandling, HoldBalanceHandling, PricingHandling, PublicKey,
    RefundHandling, TimeDiff, TransactionV1Config,
};

use crate::types::SyncHandling;

/// Options to allow overriding default chainspec and config settings.
pub(crate) struct ConfigsOverride {
    pub era_duration: TimeDiff,
    pub minimum_block_time: TimeDiff,
    pub minimum_era_height: u64,
    pub unbonding_delay: u64,
    pub round_seigniorage_rate: Ratio<u64>,
    pub consensus_protocol: ConsensusProtocolName,
    pub finders_fee: Ratio<u64>,
    pub finality_signature_proportion: Ratio<u64>,
    pub signature_rewards_max_delay: u64,
    pub storage_multiplier: u8,
    pub max_gas_price: u8,
    pub min_gas_price: u8,
    pub upper_threshold: u64,
    pub lower_threshold: u64,
    pub max_block_size: u32,
    pub block_gas_limit: u64,
    pub refund_handling_override: Option<RefundHandling>,
    pub fee_handling_override: Option<FeeHandling>,
    pub pricing_handling_override: Option<PricingHandling>,
    pub allow_prepaid_override: Option<bool>,
    pub balance_hold_interval_override: Option<TimeDiff>,
    pub administrators: Option<BTreeSet<PublicKey>>,
    pub chain_name: Option<String>,
    pub gas_hold_balance_handling: Option<HoldBalanceHandling>,
    pub transaction_v1_override: Option<TransactionV1Config>,
    pub vm_casper_v2: bool,
    pub node_config_override: NodeConfigOverride,
}

impl ConfigsOverride {
    pub(crate) fn with_refund_handling(mut self, refund_handling: RefundHandling) -> Self {
        self.refund_handling_override = Some(refund_handling);
        self
    }

    pub(crate) fn with_fee_handling(mut self, fee_handling: FeeHandling) -> Self {
        self.fee_handling_override = Some(fee_handling);
        self
    }

    pub(crate) fn with_pricing_handling(mut self, pricing_handling: PricingHandling) -> Self {
        self.pricing_handling_override = Some(pricing_handling);
        self
    }

    #[allow(unused)]
    pub(crate) fn with_allow_prepaid(mut self, allow_prepaid: bool) -> Self {
        self.allow_prepaid_override = Some(allow_prepaid);
        self
    }

    pub(crate) fn with_balance_hold_interval(mut self, balance_hold_interval: TimeDiff) -> Self {
        self.balance_hold_interval_override = Some(balance_hold_interval);
        self
    }

    pub(crate) fn with_min_gas_price(mut self, min_gas_price: u8) -> Self {
        self.min_gas_price = min_gas_price;
        self
    }

    pub(crate) fn with_max_gas_price(mut self, max_gas_price: u8) -> Self {
        self.max_gas_price = max_gas_price;
        self
    }

    pub(crate) fn with_lower_threshold(mut self, lower_threshold: u64) -> Self {
        self.lower_threshold = lower_threshold;
        self
    }

    pub(crate) fn with_upper_threshold(mut self, upper_threshold: u64) -> Self {
        self.upper_threshold = upper_threshold;
        self
    }

    pub(crate) fn with_block_size(mut self, max_block_size: u32) -> Self {
        self.max_block_size = max_block_size;
        self
    }

    pub(crate) fn with_block_gas_limit(mut self, block_gas_limit: u64) -> Self {
        self.block_gas_limit = block_gas_limit;
        self
    }

    pub(crate) fn with_minimum_era_height(mut self, minimum_era_height: u64) -> Self {
        self.minimum_era_height = minimum_era_height;
        self
    }

    pub(crate) fn with_administrators(mut self, administrators: BTreeSet<PublicKey>) -> Self {
        self.administrators = Some(administrators);
        self
    }

    pub(crate) fn with_chain_name(mut self, chain_name: String) -> Self {
        self.chain_name = Some(chain_name);
        self
    }

    pub(crate) fn with_gas_hold_balance_handling(
        mut self,
        gas_hold_balance_handling: HoldBalanceHandling,
    ) -> Self {
        self.gas_hold_balance_handling = Some(gas_hold_balance_handling);
        self
    }

    pub(crate) fn with_transaction_v1_config(
        mut self,
        transaction_v1config: TransactionV1Config,
    ) -> Self {
        self.transaction_v1_override = Some(transaction_v1config);
        self
    }

    pub(crate) fn with_idle_tolerance(mut self, idle_tolernace: TimeDiff) -> Self {
        let config = NodeConfigOverride {
            idle_tolerance: Some(idle_tolernace),
            ..Default::default()
        };
        self.node_config_override = config;
        self
    }
}

impl Default for ConfigsOverride {
    fn default() -> Self {
        ConfigsOverride {
            era_duration: TimeDiff::from_millis(0), // zero means use the default value
            minimum_block_time: "1second".parse().unwrap(),
            minimum_era_height: 2,
            unbonding_delay: 3,
            round_seigniorage_rate: Ratio::new(1, 100),
            consensus_protocol: ConsensusProtocolName::Zug,
            finders_fee: Ratio::new(1, 4),
            finality_signature_proportion: Ratio::new(1, 3),
            signature_rewards_max_delay: 5,
            storage_multiplier: 1,
            max_gas_price: 3,
            min_gas_price: 1,
            upper_threshold: 90,
            lower_threshold: 50,
            max_block_size: 10_485_760u32,
            block_gas_limit: 10_000_000_000_000u64,
            refund_handling_override: None,
            fee_handling_override: None,
            pricing_handling_override: None,
            allow_prepaid_override: None,
            balance_hold_interval_override: None,
            administrators: None,
            chain_name: None,
            gas_hold_balance_handling: None,
            transaction_v1_override: None,
            vm_casper_v2: false,
            node_config_override: NodeConfigOverride::default(),
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct NodeConfigOverride {
    pub sync_handling_override: Option<SyncHandling>,
    pub idle_tolerance: Option<TimeDiff>,
}
