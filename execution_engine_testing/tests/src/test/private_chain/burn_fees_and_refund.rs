use casper_engine_test_support::{
    TransferRequestBuilder, DEFAULT_PAYMENT, DEFAULT_PROTOCOL_VERSION,
    MINIMUM_ACCOUNT_CREATION_BALANCE,
};
use casper_types::{
    system::handle_payment::ACCUMULATION_PURSE_KEY, EntityAddr, FeeHandling, MintCosts,
    RefundHandling, DEFAULT_NOP_COST, U512,
};
use num_rational::Ratio;
use num_traits::{One, Zero};

use crate::test::private_chain::{
    self, ACCOUNT_1_ADDR, DEFAULT_ADMIN_ACCOUNT_ADDR, PRIVATE_CHAIN_ALLOW_AUCTION_BIDS,
    PRIVATE_CHAIN_ALLOW_UNRESTRICTED_TRANSFERS, PRIVATE_CHAIN_COMPUTE_REWARDS,
};

#[ignore]
#[allow(unused)]
// #[test]
fn should_burn_the_fees_without_refund() {
    let zero_refund_handling = RefundHandling::Refund {
        refund_ratio: Ratio::zero(),
    };
    let fee_handling = FeeHandling::Burn;
    let expected_fee_amount = *DEFAULT_PAYMENT;
    test_burning_fees(zero_refund_handling, fee_handling, expected_fee_amount);
}

#[ignore]
#[allow(unused)]
// #[test]
fn should_burn_the_fees_with_half_of_refund() {
    let half_refund_handling = RefundHandling::Refund {
        refund_ratio: Ratio::new(1, 2),
    };
    let fee_handling = FeeHandling::Burn;
    let expected_fee_amount =
        (U512::from(DEFAULT_NOP_COST) / U512::from(2u64)) + (*DEFAULT_PAYMENT / U512::from(2u64));
    test_burning_fees(half_refund_handling, fee_handling, expected_fee_amount);
}

#[ignore]
#[allow(unused)]
// #[test]
fn should_burn_the_fees_with_refund() {
    let full_refund_handling = RefundHandling::Refund {
        refund_ratio: Ratio::one(),
    };
    let fee_handling = FeeHandling::Burn;
    let expected_fee_amount = U512::from(DEFAULT_NOP_COST);
    test_burning_fees(full_refund_handling, fee_handling, expected_fee_amount);
}

#[ignore]
#[allow(unused)]
// #[test]
fn should_burn_full_refund_with_accumulating_fee() {
    let full_refund_handling = RefundHandling::Burn {
        refund_ratio: Ratio::one(),
    };
    let fee_handling = FeeHandling::Accumulate;
    let expected_fee_amount = *DEFAULT_PAYMENT - U512::from(DEFAULT_NOP_COST);
    test_burning_fees(full_refund_handling, fee_handling, expected_fee_amount);
}

#[ignore]
#[allow(unused)]
// #[test]
fn should_burn_zero_refund_with_accumulating_fee() {
    let full_refund_handling = RefundHandling::Burn {
        refund_ratio: Ratio::zero(),
    };
    let fee_handling = FeeHandling::Accumulate;
    let expected_fee_amount = U512::zero();
    test_burning_fees(full_refund_handling, fee_handling, expected_fee_amount);
}

#[ignore]
#[allow(unused)]
// #[test]
fn should_burn_zero_refund_and_burn_fees() {
    let full_refund_handling = RefundHandling::Burn {
        refund_ratio: Ratio::zero(),
    };
    let fee_handling = FeeHandling::Burn;
    let expected_fee_amount = *DEFAULT_PAYMENT; // 0% refund + fee
    test_burning_fees(full_refund_handling, fee_handling, expected_fee_amount);
}

#[ignore]
#[allow(unused)]
// #[test]
fn should_burn_full_refund_and_burn_fees() {
    let full_refund_handling = RefundHandling::Burn {
        refund_ratio: Ratio::one(),
    };
    let fee_handling = FeeHandling::Burn;
    let expected_fee_amount = *DEFAULT_PAYMENT; // 100% refund + fee
    test_burning_fees(full_refund_handling, fee_handling, expected_fee_amount);
}

fn test_burning_fees(
    refund_handling: RefundHandling,
    fee_handling: FeeHandling,
    expected_burn_amount: U512,
) {
    let mut builder = private_chain::custom_setup_genesis_only(
        PRIVATE_CHAIN_ALLOW_AUCTION_BIDS,
        PRIVATE_CHAIN_ALLOW_UNRESTRICTED_TRANSFERS,
        refund_handling,
        fee_handling,
        PRIVATE_CHAIN_COMPUTE_REWARDS,
    );

    let protocol_version = DEFAULT_PROTOCOL_VERSION;

    let handle_payment = builder.get_handle_payment_contract_hash();
    let handle_payment_1 = builder.get_named_keys(EntityAddr::System(handle_payment.value()));
    let rewards_purse_key = handle_payment_1
        .get(ACCUMULATION_PURSE_KEY)
        .expect("should have rewards purse");
    let rewards_purse_uref = rewards_purse_key.into_uref().expect("should be uref");
    assert_eq!(builder.get_purse_balance(rewards_purse_uref), U512::zero());
    let total_supply_before = builder.total_supply(None, protocol_version);
    // TODO: reevaluate this test, considering fee / refund / pricing modes
    // let exec_request_1 = ExecuteRequestBuilder::module_bytes(
    //     *DEFAULT_ADMIN_ACCOUNT_ADDR,
    //     wasm_utils::do_minimum_bytes(),
    //     RuntimeArgs::default(),
    // )
    //     .build();
    // let exec_request_1_proposer = exec_request_1.proposer.clone();
    // let proposer_account_1 = builder
    //     .get_entity_by_account_hash(exec_request_1_proposer.to_account_hash())
    //     .expect("should have proposer account");
    // builder.exec(exec_request_1).expect_success().commit();
    // assert_eq!(
    //     builder.get_purse_balance(proposer_account_1.main_purse()),
    //     U512::zero(),
    //     "proposer should not receive anything",
    // );
    let total_supply_after = builder.total_supply(None, protocol_version);
    assert_eq!(
        total_supply_before - total_supply_after,
        expected_burn_amount,
        "total supply should be burned exactly by the amount of calculated fee after refund"
    );
    let transfer_request =
        TransferRequestBuilder::new(MINIMUM_ACCOUNT_CREATION_BALANCE, *ACCOUNT_1_ADDR)
            .with_initiator(*DEFAULT_ADMIN_ACCOUNT_ADDR)
            .build();
    let total_supply_before = builder.total_supply(None, protocol_version);
    builder
        .transfer_and_commit(transfer_request)
        .expect_success();
    let total_supply_after = builder.total_supply(None, protocol_version);

    match fee_handling {
        FeeHandling::PayToProposer | FeeHandling::Accumulate | FeeHandling::NoFee => {
            assert_eq!(total_supply_before, total_supply_after);
        }
        FeeHandling::Burn => {
            assert_eq!(
                total_supply_before - total_supply_after,
                U512::from(MintCosts::default().transfer), // This includes fees
                "total supply should be burned exactly by the amount of calculated fees"
            );
        }
    }
}
