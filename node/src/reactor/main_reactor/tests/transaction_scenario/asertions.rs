use super::utils::{Assertion, TestStateSnapshot};
use crate::reactor::main_reactor::tests::transactions::{
    assert_exec_result_cost, exec_result_is_success, BalanceAmount,
};
use async_trait::async_trait;
use casper_types::{
    execution::{RetValue, TransformKindV2},
    Gas, PublicKey, TransactionHash, U512,
};
use once_cell::sync::Lazy;
use std::collections::BTreeMap;

pub(crate) struct TransactionSuccessful {
    hash: TransactionHash,
}

impl TransactionSuccessful {
    pub(crate) fn new(hash: TransactionHash) -> Self {
        Self { hash }
    }
}

pub static ZERO_BALANCE_AMOUNT: Lazy<BalanceAmount> = Lazy::new(BalanceAmount::zero);

#[async_trait]
impl Assertion for TransactionSuccessful {
    async fn assert(&self, snapshots_at_heights: BTreeMap<u64, TestStateSnapshot>) {
        let current_state = snapshots_at_heights.last_key_value().unwrap().1;
        assert!(current_state.exec_infos.contains_key(&self.hash));
        let exec_info = current_state.exec_infos.get(&self.hash).unwrap();
        assert!(exec_info.execution_result.is_some());
        let result = exec_info.execution_result.as_ref().unwrap();
        assert!(exec_result_is_success(result));
    }
}

pub(crate) struct TransactionFailure {
    hash: TransactionHash,
    expected_error_message: Option<String>,
}

impl TransactionFailure {
    pub(crate) fn new(hash: TransactionHash) -> Self {
        Self {
            hash,
            expected_error_message: None,
        }
    }

    pub(crate) fn expected_error_message(hash: TransactionHash, error_message: &str) -> Self {
        Self {
            hash,
            expected_error_message: Some(error_message.to_string()),
        }
    }
}

#[async_trait]
impl Assertion for TransactionFailure {
    async fn assert(&self, snapshots_at_heights: BTreeMap<u64, TestStateSnapshot>) {
        let current_state = snapshots_at_heights.last_key_value().unwrap().1;
        assert!(current_state.exec_infos.contains_key(&self.hash));
        let exec_info = current_state.exec_infos.get(&self.hash).unwrap();
        assert!(exec_info.execution_result.is_some());
        let result = exec_info.execution_result.as_ref().unwrap();
        let error_msg = match result {
            casper_types::execution::ExecutionResult::V1(_) => todo!(),
            casper_types::execution::ExecutionResult::V2(execution_result_v2) => {
                execution_result_v2.error_message.clone()
            }
        };
        assert!(error_msg.is_some());
        if let Some(msg) = &self.expected_error_message {
            assert_eq!(error_msg.unwrap(), msg.to_string());
        }
    }
}

pub(crate) struct ExecResultCost {
    hash: TransactionHash,
    expected_cost: U512,
    expected_consumed_gas: Gas,
}

impl ExecResultCost {
    pub(crate) fn new(
        hash: TransactionHash,
        expected_cost: U512,
        expected_consumed_gas: Gas,
    ) -> Self {
        Self {
            hash,
            expected_cost,
            expected_consumed_gas,
        }
    }
}

#[async_trait]
impl Assertion for ExecResultCost {
    async fn assert(&self, snapshots_at_heights: BTreeMap<u64, TestStateSnapshot>) {
        let current_state = snapshots_at_heights.last_key_value().unwrap().1;
        assert!(current_state.exec_infos.contains_key(&self.hash));
        let exec_info = current_state.exec_infos.get(&self.hash).unwrap();
        assert!(exec_info.execution_result.is_some());
        let result = exec_info.execution_result.as_ref().unwrap();
        assert_exec_result_cost(
            result.clone(),
            self.expected_cost,
            self.expected_consumed_gas,
            "transfer_cost_fixed_price_no_fee_no_refund",
        );
    }
}

pub(crate) struct TotalSupplyChange {
    //It's a signed integer since we can expect either an increase or decrease.
    total_supply_change: i64,
    at_block_height: u64,
}

impl TotalSupplyChange {
    pub(crate) fn new(total_supply_change: i64, at_block_height: u64) -> Self {
        Self {
            total_supply_change,
            at_block_height,
        }
    }
}

#[async_trait]
impl Assertion for TotalSupplyChange {
    async fn assert(&self, snapshots_at_heights: BTreeMap<u64, TestStateSnapshot>) {
        let before = snapshots_at_heights.get(&1).unwrap();
        let after = snapshots_at_heights.get(&self.at_block_height).unwrap();
        let before_total_supply = before.total_supply;
        let got = after.total_supply;
        let total_supply = self.total_supply_change;
        let expected = if total_supply > 0 {
            before_total_supply
                .checked_add(total_supply.unsigned_abs().into())
                .unwrap()
        } else {
            before_total_supply
                .checked_sub(total_supply.unsigned_abs().into())
                .unwrap()
        };
        assert_eq!(expected, got);
    }
}

#[derive(Copy, Clone, Debug)]
/// An amount up or down.
pub enum BalanceChange {
    Up(U512),
    Down(U512),
}

/// Assert that the account associated with the given public key has observed a change in balance.
/// Can assert on total and available balance.
pub(crate) struct PublicKeyBalanceChange {
    /// public key of the account which needs to be queried
    public_key: PublicKey,
    //It's a signed integer since we can expect either an increase or decrease.
    total_balance_change: BalanceChange,
    //It's a signed integer since we can expect either an increase or decrease.
    available_balance_change: BalanceChange,
}

impl PublicKeyBalanceChange {
    pub(crate) fn new(
        public_key: PublicKey,
        total_balance_change: BalanceChange,
        available_balance_change: BalanceChange,
    ) -> Self {
        Self {
            public_key,
            total_balance_change,
            available_balance_change,
        }
    }
}

#[async_trait]
impl Assertion for PublicKeyBalanceChange {
    async fn assert(&self, snapshots_at_heights: BTreeMap<u64, TestStateSnapshot>) {
        let account_hash = self.public_key.to_account_hash();
        let before = snapshots_at_heights.get(&0).unwrap();
        let after = snapshots_at_heights.last_key_value().unwrap().1;
        let before_balance = before
            .balances
            .get(&account_hash)
            //There is a chance that the key we're asking for was not an account in
            // genesis, if that's true we don't expect it to be at height 0.
            .unwrap_or(&ZERO_BALANCE_AMOUNT);

        let before_total = before_balance.total;
        let before_available = before_balance.available;
        let after_total = after.balances.get(&account_hash).unwrap().total;
        let after_available = after.balances.get(&account_hash).unwrap().available;

        let expected = {
            match self.total_balance_change {
                BalanceChange::Up(val) => {
                    before_total.checked_add(val).expect("should mod total up")
                }
                BalanceChange::Down(val) => before_total
                    .checked_sub(val)
                    .expect("should mod total down"),
            }
        };

        assert_eq!(after_total, expected, "after_total should match expected");
        let expected = {
            match self.available_balance_change {
                BalanceChange::Up(val) => before_available
                    .checked_add(val)
                    .expect("should mod available up"),
                BalanceChange::Down(val) => before_available
                    .checked_sub(val)
                    .expect("should mod available down"),
            }
        };
        assert_eq!(
            after_available, expected,
            "after_available should match expected"
        );
    }
}

pub(crate) struct PublicKeyTotalMeetsAvailable {
    public_key: PublicKey,
}

impl PublicKeyTotalMeetsAvailable {
    pub(crate) fn new(public_key: PublicKey) -> Self {
        Self { public_key }
    }
}

#[async_trait]
impl Assertion for PublicKeyTotalMeetsAvailable {
    async fn assert(&self, snapshots_at_heights: BTreeMap<u64, TestStateSnapshot>) {
        let account_hash = self.public_key.to_account_hash();
        let after = snapshots_at_heights.last_key_value().unwrap().1;
        let balance = after.balances.get(&account_hash).unwrap();
        let after_total = balance.total;
        let after_available = balance.available;
        assert_eq!(after_total, after_available);
    }
}

pub(crate) struct ExecutionResultHasRet {
    hash: TransactionHash,
    expected_ret_val: RetValue,
}

impl ExecutionResultHasRet {
    pub(crate) fn new(hash: TransactionHash, expected_ret_val: RetValue) -> Self {
        Self {
            hash,
            expected_ret_val,
        }
    }
}

#[async_trait]
impl Assertion for ExecutionResultHasRet {
    async fn assert(&self, snapshots_at_heights: BTreeMap<u64, TestStateSnapshot>) {
        let current_state = snapshots_at_heights.last_key_value().unwrap().1;
        assert!(current_state.exec_infos.contains_key(&self.hash));
        let exec_info = current_state.exec_infos.get(&self.hash).unwrap();
        assert!(exec_info.execution_result.is_some());
        let result = exec_info.execution_result.as_ref().unwrap();
        assert!(exec_result_is_success(result));
        match result {
            casper_types::execution::ExecutionResult::V1(_) => todo!(),
            casper_types::execution::ExecutionResult::V2(execution_result_v2) => {
                let maybe_last_ret = execution_result_v2.effects.transforms().iter().filter(|el|{
                    matches!(el.kind(), TransformKindV2::Ret(v) if *v == self.expected_ret_val)
                }).last();
                assert!(maybe_last_ret.is_some());
                let last = maybe_last_ret.unwrap();
                assert!(
                    matches!(last.kind(), TransformKindV2::Ret(v) if *v == self.expected_ret_val)
                );
            }
        }
    }
}
