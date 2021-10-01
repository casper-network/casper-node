use tracing::warn;

use casper_types::{Gas, Key, U512};

use super::Error;
use crate::{
    core::engine_state::execution_result::ExecutionResult,
    shared::{execution_journal::ExecutionJournal, transform::Transform},
};

fn on_fail_charge_test_helper<T>(
    f: impl Fn() -> Result<T, Error>,
    success_cost: Gas,
    error_cost: Gas,
) -> ExecutionResult {
    let transfers = Vec::default();
    let _result = on_fail_charge!(f(), error_cost, transfers);
    ExecutionResult::Success {
        execution_journal: Default::default(),
        transfers,
        cost: success_cost,
    }
}

#[test]
fn on_fail_charge_ok_test() {
    let val = Gas::new(U512::from(123));
    match on_fail_charge_test_helper(|| Ok(()), val, Gas::new(U512::from(456))) {
        ExecutionResult::Success { cost, .. } => assert_eq!(cost, val),
        ExecutionResult::Failure { .. } => panic!("Should be success"),
    }
}

#[test]
fn on_fail_charge_err_laziness_test() {
    let input: Result<(), Error> = Err(Error::GasLimit);
    let error_cost = Gas::new(U512::from(456));
    match on_fail_charge_test_helper(|| input.clone(), Gas::new(U512::from(123)), error_cost) {
        ExecutionResult::Success { .. } => panic!("Should fail"),
        ExecutionResult::Failure { cost, .. } => assert_eq!(cost, error_cost),
    }
}

#[test]
fn on_fail_charge_with_action() {
    let f = || {
        let input: Result<(), Error> = Err(Error::GasLimit);
        let transfers = Vec::default();
        let journal: ExecutionJournal = vec![(Key::Hash([42u8; 32]), Transform::Identity)].into();

        on_fail_charge!(input, Gas::new(U512::from(456)), journal, transfers);
        ExecutionResult::Success {
            execution_journal: Default::default(),
            transfers: Vec::default(),
            cost: Gas::default(),
        }
    };
    match f() {
        ExecutionResult::Success { .. } => panic!("Should fail"),
        ExecutionResult::Failure {
            cost,
            execution_journal,
            ..
        } => {
            assert_eq!(cost, Gas::new(U512::from(456)));
            // Check if the containers are non-empty
            assert_eq!(execution_journal.len(), 1);
        }
    }
}
