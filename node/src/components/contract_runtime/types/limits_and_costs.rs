use crate::{
    contract_runtime::types::transaction_process_context::TransactionProcessContextError,
    types::MetaTransaction,
};
use casper_types::{Chainspec, Gas, U512};
use num_rational::Ratio;

#[derive(Clone, Debug)]
pub(crate) struct LimitsAndCosts {
    gas_limit: Gas,
    cost_estimate: U512,
    initial_cost: U512,
    gas_price: u8,

    min_cost: U512,
    wei_per_mote: u128,
    base_fee_wei: u128,
    evm_block_gas_limit: u64,

    consumed: Option<Gas>,
    refund: Option<U512>,
    available: Option<U512>,
}

impl LimitsAndCosts {
    pub(crate) fn new(
        gas_limit: Gas,
        cost_estimate: U512,
        initial_cost: U512,
        gas_price: u8,
        min_cost: U512,
        wei_per_mote: u128,
        base_fee_wei: u128,
        evm_block_gas_limit: u64,
    ) -> Self {
        LimitsAndCosts {
            gas_limit,
            cost_estimate,
            initial_cost,
            min_cost,
            wei_per_mote,
            base_fee_wei,
            gas_price,
            consumed: None,
            refund: None,
            available: None,
            evm_block_gas_limit,
        }
    }

    pub(crate) fn evm_block_gas_limit(&self) -> u64 {
        self.evm_block_gas_limit
    }

    pub(crate) fn gas_limit(&self) -> Gas {
        self.gas_limit
    }

    pub(crate) fn cost_estimate(&self) -> U512 {
        self.cost_estimate
    }

    pub(crate) fn initial_cost(&self) -> U512 {
        self.initial_cost
    }

    pub(crate) fn gas_price(&self) -> u8 {
        self.gas_price
    }

    pub(crate) fn min_cost(&self) -> U512 {
        self.min_cost
    }

    pub(crate) fn consumed(&self) -> Option<Gas> {
        self.consumed
    }

    pub(crate) fn with_gas_limit_consumed(&mut self) -> &mut Self {
        match self.consumed {
            Some(consumed) => {
                self.consumed = Some(consumed.saturating_add(self.gas_limit));
            }
            None => {
                self.consumed = Some(self.gas_limit);
            }
        }
        self
    }

    pub(crate) fn with_added_consumed(&mut self, added_consumed: Gas) -> &mut Self {
        match self.consumed {
            Some(consumed) => {
                self.consumed = Some(consumed.saturating_add(added_consumed));
            }
            None => {
                self.consumed = Some(added_consumed);
            }
        }
        self
    }

    pub(crate) fn refund(&self) -> U512 {
        self.refund.unwrap_or_default()
    }

    pub(crate) fn with_refund(&mut self, refund: U512) -> &mut Self {
        self.refund = Some(refund);
        self
    }

    pub(crate) fn available(&self) -> Option<U512> {
        self.available
    }

    pub(crate) fn with_available(&mut self, available: Option<U512>) -> &mut Self {
        self.available = available;
        self
    }

    pub(crate) fn with_zero_cost(&mut self) -> &mut Self {
        self.initial_cost = U512::zero();
        self.min_cost = U512::zero();
        self
    }

    pub(crate) fn wei_per_mote(&self) -> u128 {
        self.wei_per_mote
    }

    pub(crate) fn base_fee_wei(&self) -> u128 {
        self.base_fee_wei
    }

    /// Converts an EVM gas cost, denominated in wei, to motes by rounding up.
    ///
    /// Rounding is applied after multiplying gas by price, so sub-mote totals
    /// are charged as one mote without overcharging each gas unit separately.
    pub(crate) fn evm_gas_fee_motes(&self, gas: u64, gas_price_wei: u128) -> Option<U512> {
        let wei_per_mote = self.wei_per_mote();
        if wei_per_mote == 0 {
            return None;
        }
        let fee_wei = U512::from(gas).checked_mul(U512::from(gas_price_wei))?;
        Some(
            Ratio::new(fee_wei, U512::from(wei_per_mote))
                .ceil()
                .to_integer(),
        )
    }

    pub(crate) fn try_from_meta_txn(
        mtxn: &MetaTransaction,
        chainspec: &Chainspec,
    ) -> Result<LimitsAndCosts, TransactionProcessContextError> {
        /*
        we solve for halting state using a `gas limit` which is the maximum amount of
        computation we will allow a given transaction to consume. the transaction itself
        provides a function to determine this if provided with the current cost tables
        gas_limit is ALWAYS calculated with price == 1.

        next there is the actual cost, i.e. how much we charge for that computation
        this is calculated by multiplying the gas limit by the current `gas_price`
        gas price has a floor of 1, and the ceiling is configured in the chainspec
        NOTE: when the gas price is 1, the gas limit and the cost are coincidentally
        equal because x == x * 1; thus it is recommended to run tests with
        price >1 to avoid being confused by this.

        the third important value is the amount of computation consumed by executing a transaction.
        consumed is determined after execution and is used for refund & fee post-processing.

        for native transactions, there is no wasm and the consumed always equals the limit.
        for bytecode / wasm based transactions, the consumed is based on what opcodes were executed
            and can range from >=0 to <=gas_limit.
        */

        let cost_estimate = match mtxn.cost_estimate() {
            Some(cost_estimate) => cost_estimate,
            None => return Err(TransactionProcessContextError::MissingCostEstimate),
        };

        let evm_block_gas_limit = chainspec.evm_config.block_gas_limit;

        let initial_cost = mtxn.initial_cost().value();

        let gas_price = mtxn.gas_price();

        let gas_limit = match &mtxn.gas_limit(chainspec) {
            Ok(gas_limit) => *gas_limit,
            Err(ite) => {
                return Err(TransactionProcessContextError::InvalidTransaction(
                    ite.clone(),
                ))
            }
        };
        let gas_limit_value = gas_limit.value();

        let baseline_motes_amount = chainspec.core_config.baseline_motes_amount_u512();
        let min_cost = match mtxn.min_cost(gas_limit_value, baseline_motes_amount) {
            Ok(motes) => motes.value(),
            Err(err) => return Err(TransactionProcessContextError::InvalidTransaction(err)),
        };

        let wei_per_mote = u128::from(chainspec.evm_config.wei_per_mote);
        let base_fee_wei = chainspec.evm_config.base_fee_wei();

        Ok(LimitsAndCosts::new(
            gas_limit,
            cost_estimate,
            initial_cost,
            gas_price,
            min_cost,
            wei_per_mote,
            base_fee_wei,
            evm_block_gas_limit,
        ))
    }
}

impl Default for LimitsAndCosts {
    fn default() -> Self {
        LimitsAndCosts {
            gas_limit: Gas::default(),
            cost_estimate: U512::zero(),
            initial_cost: U512::zero(),
            min_cost: U512::zero(),
            wei_per_mote: 0u128,
            base_fee_wei: 0u128,
            gas_price: 0,
            consumed: None,
            refund: None,
            available: None,
            evm_block_gas_limit: 0u64,
        }
    }
}
