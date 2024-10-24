use num_rational::Ratio;
use num_traits::{CheckedMul, CheckedSub};
use once_cell::sync::Lazy;
use std::collections::BTreeMap;
use tempfile::TempDir;

use casper_engine_test_support::{
    ChainspecConfig, ExecuteRequestBuilder, LmdbWasmTestBuilder, StepRequestBuilder,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_PROTOCOL_VERSION, LOCAL_GENESIS_REQUEST,
    MINIMUM_ACCOUNT_CREATION_BALANCE, SYSTEM_ADDR,
};
use casper_execution_engine::{
    engine_state::{engine_config::DEFAULT_MINIMUM_DELEGATION_AMOUNT, Error},
    execution::ExecError,
};

use crate::test::system_contracts::auction::{
    get_delegator_staked_amount, get_era_info, get_validator_bid,
};
use casper_types::{
    self,
    account::AccountHash,
    api_error::ApiError,
    runtime_args,
    system::auction::{
        BidsExt, DelegationRate, Error as AuctionError, Reservation, SeigniorageAllocation,
        ARG_AMOUNT, ARG_DELEGATION_RATE, ARG_DELEGATOR, ARG_DELEGATORS, ARG_ENTRY_POINT,
        ARG_PUBLIC_KEY, ARG_RESERVATIONS, ARG_RESERVED_SLOTS, ARG_REWARDS_MAP, ARG_VALIDATOR,
        DELEGATION_RATE_DENOMINATOR, METHOD_DISTRIBUTE,
    },
    ProtocolVersion, PublicKey, SecretKey, U512,
};

const ARG_TARGET: &str = "target";

const CONTRACT_TRANSFER_TO_ACCOUNT: &str = "transfer_to_account_u512.wasm";
const CONTRACT_ADD_BID: &str = "add_bid.wasm";
const CONTRACT_DELEGATE: &str = "delegate.wasm";
const CONTRACT_UNDELEGATE: &str = "undelegate.wasm";
const CONTRACT_ADD_RESERVATIONS: &str = "add_reservations.wasm";
const CONTRACT_CANCEL_RESERVATIONS: &str = "cancel_reservations.wasm";

const ADD_BID_AMOUNT_1: u64 = 1_000_000_000_000;
const ADD_BID_RESERVED_SLOTS: u32 = 1;

static VALIDATOR_1: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([3; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static DELEGATOR_1: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([205; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static DELEGATOR_2: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([207; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static DELEGATOR_3: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([209; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static DELEGATOR_4: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([211; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});

static VALIDATOR_1_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*VALIDATOR_1));
static DELEGATOR_1_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*DELEGATOR_1));
static DELEGATOR_2_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*DELEGATOR_2));
static DELEGATOR_3_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*DELEGATOR_3));
static DELEGATOR_4_ADDR: Lazy<AccountHash> = Lazy::new(|| AccountHash::from(&*DELEGATOR_4));

const VALIDATOR_1_DELEGATION_RATE: DelegationRate = 10;
const VALIDATOR_1_RESERVATION_DELEGATION_RATE: DelegationRate = 20;

/// Fund validator and delegators accounts.
fn setup_accounts(max_delegators_per_validator: u32) -> LmdbWasmTestBuilder {
    let chainspec =
        ChainspecConfig::default().with_max_delegators_per_validator(max_delegators_per_validator);

    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = LmdbWasmTestBuilder::new_with_config(data_dir.path(), chainspec);

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let transfer_to_validator_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *VALIDATOR_1_ADDR,
            ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE)
        },
    )
    .build();

    let transfer_to_delegator_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_1_ADDR,
            ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE)
        },
    )
    .build();

    let transfer_to_delegator_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_2_ADDR,
            ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE)
        },
    )
    .build();

    let transfer_to_delegator_3 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_3_ADDR,
            ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE)
        },
    )
    .build();

    let transfer_to_delegator_4 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_TO_ACCOUNT,
        runtime_args! {
            ARG_TARGET => *DELEGATOR_4_ADDR,
            ARG_AMOUNT => U512::from(MINIMUM_ACCOUNT_CREATION_BALANCE)
        },
    )
    .build();

    let post_genesis_request = vec![
        transfer_to_validator_1,
        transfer_to_delegator_1,
        transfer_to_delegator_2,
        transfer_to_delegator_3,
        transfer_to_delegator_4,
    ];

    for request in post_genesis_request {
        builder.exec(request).expect_success().commit();
    }

    builder
}

/// Submit validator bid for `VALIDATOR_1_ADDR` and advance eras
/// until they are elected as active validator.
fn setup_validator_bid(builder: &mut LmdbWasmTestBuilder, reserved_slots: u32) {
    let add_validator_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => VALIDATOR_1_DELEGATION_RATE,
            ARG_RESERVED_SLOTS => reserved_slots,
        },
    )
    .build();

    builder
        .exec(add_validator_request)
        .expect_success()
        .commit();

    for _ in 0..=builder.get_auction_delay() {
        let step_request = StepRequestBuilder::new()
            .with_parent_state_hash(builder.get_post_state_hash())
            .with_protocol_version(ProtocolVersion::V1_0_0)
            .with_next_era_id(builder.get_era().successor())
            .with_run_auction(true)
            .build();

        assert!(
            builder.step(step_request).is_success(),
            "must execute step request"
        );
    }
}

#[ignore]
#[test]
fn should_enforce_max_delegators_per_validator_with_reserved_slots() {
    let mut builder = setup_accounts(3);

    setup_validator_bid(&mut builder, ADD_BID_RESERVED_SLOTS);

    let delegation_request_1 = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    let delegation_request_2 = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_2.clone(),
        },
    )
    .build();

    let delegation_requests = [delegation_request_1, delegation_request_2];

    for request in delegation_requests {
        builder.exec(request).expect_success().commit();
    }

    // Delegator 3 is not on reservation list and validator is at delegator limit
    // therefore delegation request should fail
    let delegation_request_3 = ExecuteRequestBuilder::standard(
        *DELEGATOR_3_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_3.clone(),
        },
    )
    .build();

    builder.exec(delegation_request_3).expect_failure();
    let error = builder.get_error().expect("should get error");

    assert!(matches!(
        error,
        Error::Exec(ExecError::Revert(ApiError::AuctionError(auction_error)))
        if auction_error == AuctionError::ExceededDelegatorSizeLimit as u8));

    // Once we put Delegator 3 on reserved list the delegation request should succeed
    let reservation = Reservation::new(VALIDATOR_1.clone(), DELEGATOR_3.clone(), 0);
    let reservation_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_RESERVATIONS,
        runtime_args! {
            ARG_RESERVATIONS => vec![reservation],
        },
    )
    .build();
    builder.exec(reservation_request).expect_success().commit();

    let delegation_request_4 = ExecuteRequestBuilder::standard(
        *DELEGATOR_3_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_3.clone(),
        },
    )
    .build();
    builder.exec(delegation_request_4).expect_success().commit();

    // Delegator 4 not on reserved list and validator at capacity
    // therefore delegation request should fail
    let delegation_request_5 = ExecuteRequestBuilder::standard(
        *DELEGATOR_4_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_4.clone(),
        },
    )
    .build();
    builder.exec(delegation_request_5).expect_failure();

    // Now we undelegate Delegator 3 and cancel his reservation,
    // then add reservation for Delegator 4. Then delegation request for
    // Delegator 4 should succeed
    let undelegation_request = ExecuteRequestBuilder::standard(
        *DELEGATOR_3_ADDR,
        CONTRACT_UNDELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::MAX,
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_3.clone(),
        },
    )
    .build();
    builder.exec(undelegation_request).expect_success().commit();

    let cancellation_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_CANCEL_RESERVATIONS,
        runtime_args! {
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATORS => vec![DELEGATOR_3.clone()],
        },
    )
    .build();
    builder.exec(cancellation_request).expect_success().commit();

    let reservation = Reservation::new(VALIDATOR_1.clone(), DELEGATOR_4.clone(), 0);
    let reservation_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_RESERVATIONS,
        runtime_args! {
            ARG_RESERVATIONS => vec![reservation],
        },
    )
    .build();
    builder.exec(reservation_request).expect_success().commit();

    let delegation_request_6 = ExecuteRequestBuilder::standard(
        *DELEGATOR_4_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_4.clone(),
        },
    )
    .build();
    builder.exec(delegation_request_6).expect_success().commit();
}

#[ignore]
#[test]
fn should_allow_validator_to_reserve_all_delegator_slots() {
    let max_delegators_per_validator = 2;

    let mut builder = setup_accounts(max_delegators_per_validator);

    setup_validator_bid(&mut builder, 0);

    // cannot reserve more slots than maximum delegator number
    let add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => VALIDATOR_1_DELEGATION_RATE,
            ARG_RESERVED_SLOTS => max_delegators_per_validator + 1,
        },
    )
    .build();

    builder.exec(add_bid_request).expect_failure();
    let error = builder.get_error().expect("should get error");

    assert!(matches!(
        error,
        Error::Exec(ExecError::Revert(ApiError::AuctionError(auction_error)))
        if auction_error == AuctionError::ExceededReservationSlotsLimit as u8));

    // can reserve all slots
    let add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => VALIDATOR_1_DELEGATION_RATE,
            ARG_RESERVED_SLOTS => max_delegators_per_validator,
        },
    )
    .build();

    builder.exec(add_bid_request).expect_success().commit();
}

#[ignore]
#[test]
fn should_not_allow_validator_to_reserve_more_slots_than_free_delegator_slots() {
    let max_delegators_per_validator = 2;

    let mut builder = setup_accounts(max_delegators_per_validator);

    setup_validator_bid(&mut builder, 0);

    let delegation_request_1 = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    builder.exec(delegation_request_1).expect_success().commit();

    // cannot reserve more slots than number of free delegator slots
    let add_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => VALIDATOR_1_DELEGATION_RATE,
            ARG_RESERVED_SLOTS =>  max_delegators_per_validator,
        },
    )
    .build();

    builder.exec(add_bid_request).expect_failure();
    let error = builder.get_error().expect("should get error");

    assert!(matches!(
        error,
        Error::Exec(ExecError::Revert(ApiError::AuctionError(auction_error)))
        if auction_error == AuctionError::ExceededReservationSlotsLimit as u8));
}

#[ignore]
#[test]
fn should_not_allow_validator_to_reduce_number_of_reserved_spots_if_they_are_occupied() {
    let mut builder = setup_accounts(3);

    let reserved_slots = 2;
    setup_validator_bid(&mut builder, reserved_slots);

    // add reservations for Delegators 1 and 2
    let reservation_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_RESERVATIONS,
        runtime_args! {
            ARG_RESERVATIONS => vec![
                Reservation::new(VALIDATOR_1.clone(), DELEGATOR_1.clone(), 0),
                Reservation::new(VALIDATOR_1.clone(), DELEGATOR_2.clone(), 0),
            ],
        },
    )
    .build();
    builder.exec(reservation_request).expect_success().commit();
    let reservations = builder
        .get_bids()
        .reservations_by_validator_public_key(&VALIDATOR_1)
        .expect("should have reservations");
    assert_eq!(reservations.len(), 2);

    // cannot reduce number of reserved slots because
    // there are reservations for all of them
    let add_validator_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => VALIDATOR_1_DELEGATION_RATE,
            ARG_RESERVED_SLOTS => reserved_slots - 1,
        },
    )
    .build();

    builder.exec(add_validator_bid_request).expect_failure();
    let error = builder.get_error().expect("should get error");
    assert!(matches!(
        error,
        Error::Exec(ExecError::Revert(ApiError::AuctionError(auction_error)))
        if auction_error == AuctionError::ReservationSlotsCountTooSmall as u8));

    // remove a reservation for Delegator 2 and
    // reduce number of reserved spots
    let cancellation_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_CANCEL_RESERVATIONS,
        runtime_args! {
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATORS => vec![DELEGATOR_2.clone()],
        },
    )
    .build();
    builder.exec(cancellation_request).expect_success().commit();

    let reservations = builder
        .get_bids()
        .reservations_by_validator_public_key(&VALIDATOR_1)
        .expect("should have reservations");
    assert_eq!(reservations.len(), 1);

    let add_validator_bid_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_BID,
        runtime_args! {
            ARG_PUBLIC_KEY => VALIDATOR_1.clone(),
            ARG_AMOUNT => U512::from(ADD_BID_AMOUNT_1),
            ARG_DELEGATION_RATE => VALIDATOR_1_DELEGATION_RATE,
            ARG_RESERVED_SLOTS => reserved_slots - 1,
        },
    )
    .build();

    builder
        .exec(add_validator_bid_request)
        .expect_success()
        .commit();

    // cannot add a reservation for Delegator 2 back
    // because number of slots is reduced
    let reservation_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_RESERVATIONS,
        runtime_args! {
            ARG_RESERVATIONS => vec![
                Reservation::new(VALIDATOR_1.clone(), DELEGATOR_2.clone(), 0),
            ],
        },
    )
    .build();
    builder.exec(reservation_request).expect_failure();
    let error = builder.get_error().expect("should get error");

    assert!(matches!(
        error,
        Error::Exec(ExecError::Revert(ApiError::AuctionError(auction_error)))
        if auction_error == AuctionError::ExceededReservationsLimit as u8));
}

#[ignore]
#[test]
fn should_not_allow_validator_to_remove_active_reservation_if_there_are_no_free_delegator_slots() {
    let mut builder = setup_accounts(2);

    let reserved_slots = 1;
    setup_validator_bid(&mut builder, reserved_slots);

    // add delegation for Delegator 1
    let delegation_request_1 = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();

    builder.exec(delegation_request_1).expect_success().commit();

    // cannot add delegation for Delegator 2
    let delegation_request_2 = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_2.clone(),
        },
    )
    .build();

    builder.exec(delegation_request_2).expect_failure();
    let error = builder.get_error().expect("should get error");
    assert!(matches!(
        error,
        Error::Exec(ExecError::Revert(ApiError::AuctionError(auction_error)))
        if auction_error == AuctionError::ExceededDelegatorSizeLimit as u8));

    // add reservation for Delegator 2
    let reservation_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_RESERVATIONS,
        runtime_args! {
            ARG_RESERVATIONS => vec![
                Reservation::new(VALIDATOR_1.clone(), DELEGATOR_2.clone(), 0),
            ],
        },
    )
    .build();
    builder.exec(reservation_request).expect_success().commit();

    // add delegation for Delegator 2
    let delegation_request_2 = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => U512::from(DEFAULT_MINIMUM_DELEGATION_AMOUNT),
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_2.clone(),
        },
    )
    .build();

    builder.exec(delegation_request_2).expect_success().commit();

    // cannot cancel reservation for Delegator 2
    // because there are no free public slots for delegators
    let cancellation_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_CANCEL_RESERVATIONS,
        runtime_args! {
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATORS => vec![DELEGATOR_2.clone()],
        },
    )
    .build();
    builder.exec(cancellation_request).expect_failure();
    let error = builder.get_error().expect("should get error");
    assert!(matches!(
        error,
        Error::Exec(ExecError::Revert(ApiError::AuctionError(auction_error)))
        if auction_error == AuctionError::ExceededDelegatorSizeLimit as u8));
}

#[ignore]
#[test]
fn should_handle_reserved_slots() {
    let mut builder = setup_accounts(4);

    let reserved_slots = 3;
    setup_validator_bid(&mut builder, reserved_slots);

    let reservations = builder
        .get_bids()
        .reservations_by_validator_public_key(&VALIDATOR_1);
    assert!(reservations.is_none());

    // add reservations for Delegators 1 and 2
    let reservation_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_RESERVATIONS,
        runtime_args! {
            ARG_RESERVATIONS => vec![
                Reservation::new(VALIDATOR_1.clone(), DELEGATOR_1.clone(), 0),
                Reservation::new(VALIDATOR_1.clone(), DELEGATOR_2.clone(), 0),
            ],
        },
    )
    .build();
    builder.exec(reservation_request).expect_success().commit();
    let reservations = builder
        .get_bids()
        .reservations_by_validator_public_key(&VALIDATOR_1)
        .expect("should have reservations");
    assert_eq!(reservations.len(), 2);

    // try to cancel reservation for Delegators 1,2 and 3
    // this fails because reservation for Delegator 3 doesn't exist yet
    let cancellation_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_CANCEL_RESERVATIONS,
        runtime_args! {
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATORS => vec![DELEGATOR_1.clone(), DELEGATOR_2.clone(), DELEGATOR_3.clone()],
        },
    )
    .build();
    builder.exec(cancellation_request).expect_failure();
    let error = builder.get_error().expect("should get error");
    assert!(matches!(
        error,
        Error::Exec(ExecError::Revert(ApiError::AuctionError(auction_error)))
        if auction_error == AuctionError::ReservationNotFound as u8));

    // add reservation for Delegator 2 and 3
    // reservation for Delegator 2 already exists, but it shouldn't cause an error
    let reservation_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_RESERVATIONS,
        runtime_args! {
            ARG_RESERVATIONS => vec![
                Reservation::new(VALIDATOR_1.clone(), DELEGATOR_2.clone(), 0),
                Reservation::new(VALIDATOR_1.clone(), DELEGATOR_3.clone(), 0),
            ],
        },
    )
    .build();
    builder.exec(reservation_request).expect_success().commit();
    let reservations = builder
        .get_bids()
        .reservations_by_validator_public_key(&VALIDATOR_1)
        .expect("should have reservations");
    assert_eq!(reservations.len(), 3);

    // try to add reservation for Delegator 4
    // this fails because the reservation list is already full
    let reservation_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_RESERVATIONS,
        runtime_args! {
            ARG_RESERVATIONS => vec![
                Reservation::new(VALIDATOR_1.clone(), DELEGATOR_4.clone(), 0),
            ],
        },
    )
    .build();
    builder.exec(reservation_request).expect_failure();
    let error = builder.get_error().expect("should get error");
    assert!(matches!(
        error,
        Error::Exec(ExecError::Revert(ApiError::AuctionError(auction_error)))
        if auction_error == AuctionError::ExceededReservationsLimit as u8));

    // cancel all reservations
    let cancellation_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_CANCEL_RESERVATIONS,
        runtime_args! {
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATORS => vec![DELEGATOR_1.clone(), DELEGATOR_2.clone(), DELEGATOR_3.clone()],
        },
    )
    .build();
    builder.exec(cancellation_request).expect_success().commit();
    let reservations = builder
        .get_bids()
        .reservations_by_validator_public_key(&VALIDATOR_1);
    assert!(reservations.is_none());
}

#[ignore]
#[test]
fn should_update_reservation_delegation_rate() {
    let mut builder = setup_accounts(4);

    let reserved_slots = 3;
    setup_validator_bid(&mut builder, reserved_slots);

    let reservations = builder
        .get_bids()
        .reservations_by_validator_public_key(&VALIDATOR_1);
    assert!(reservations.is_none());

    // add reservations for Delegators 1 and 2
    let reservation_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_RESERVATIONS,
        runtime_args! {
            ARG_RESERVATIONS => vec![
                Reservation::new(VALIDATOR_1.clone(), DELEGATOR_1.clone(), 0),
                Reservation::new(VALIDATOR_1.clone(), DELEGATOR_2.clone(), 0),
            ],
        },
    )
    .build();
    builder.exec(reservation_request).expect_success().commit();
    let reservations = builder
        .get_bids()
        .reservations_by_validator_public_key(&VALIDATOR_1)
        .expect("should have reservations");
    assert_eq!(reservations.len(), 2);

    // try to change delegation rate for Delegator 1
    // this fails because delegation rate value is invalid
    let reservation_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_RESERVATIONS,
        runtime_args! {
            ARG_RESERVATIONS => vec![
                Reservation::new(VALIDATOR_1.clone(), DELEGATOR_1.clone(), DELEGATION_RATE_DENOMINATOR + 1),
            ],
        },
    )
    .build();
    builder.exec(reservation_request).expect_failure();
    let error = builder.get_error().expect("should get error");
    assert!(matches!(
        error,
        Error::Exec(ExecError::Revert(ApiError::AuctionError(auction_error)))
        if auction_error == AuctionError::DelegationRateTooLarge as u8));

    // change delegation rate for Delegator 1
    let reservation_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_RESERVATIONS,
        runtime_args! {
            ARG_RESERVATIONS => vec![
                Reservation::new(VALIDATOR_1.clone(), DELEGATOR_1.clone(), 10),
            ],
        },
    )
    .build();
    builder.exec(reservation_request).expect_success().commit();
    let reservations = builder
        .get_bids()
        .reservations_by_validator_public_key(&VALIDATOR_1)
        .expect("should have reservations");
    assert_eq!(reservations.len(), 2);

    let delegator_1_reservation = reservations
        .iter()
        .find(|r| *r.delegator_public_key() == *DELEGATOR_1)
        .unwrap();
    assert_eq!(*delegator_1_reservation.delegation_rate(), 10);
}

#[ignore]
#[test]
fn should_distribute_rewards_with_reserved_slots() {
    let validator_stake = U512::from(ADD_BID_AMOUNT_1);
    let delegator_1_stake = U512::from(1_000_000_000_000u64);
    let delegator_2_stake = U512::from(1_000_000_000_000u64);
    let total_delegator_stake = delegator_1_stake + delegator_2_stake;
    let total_stake = validator_stake + total_delegator_stake;

    let mut builder = setup_accounts(3);

    setup_validator_bid(&mut builder, ADD_BID_RESERVED_SLOTS);

    // add reservation for Delegator 1
    let reservation_request = ExecuteRequestBuilder::standard(
        *VALIDATOR_1_ADDR,
        CONTRACT_ADD_RESERVATIONS,
        runtime_args! {
            ARG_RESERVATIONS => vec![
                Reservation::new(VALIDATOR_1.clone(), DELEGATOR_1.clone(), VALIDATOR_1_RESERVATION_DELEGATION_RATE),
            ],
        },
    )
    .build();
    builder.exec(reservation_request).expect_success().commit();

    // add delegator bids for Delegator 1 and 2
    let delegation_request_1 = ExecuteRequestBuilder::standard(
        *DELEGATOR_1_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => delegator_1_stake,
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_1.clone(),
        },
    )
    .build();
    let delegation_request_2 = ExecuteRequestBuilder::standard(
        *DELEGATOR_2_ADDR,
        CONTRACT_DELEGATE,
        runtime_args! {
            ARG_AMOUNT => delegator_2_stake,
            ARG_VALIDATOR => VALIDATOR_1.clone(),
            ARG_DELEGATOR => DELEGATOR_2.clone(),
        },
    )
    .build();

    let delegation_requests = [delegation_request_1, delegation_request_2];

    for request in delegation_requests {
        builder.exec(request).expect_success().commit();
    }

    // calculate expected rewards
    let protocol_version = DEFAULT_PROTOCOL_VERSION;
    let initial_supply = builder.total_supply(protocol_version, None);
    let total_payout = builder.base_round_reward(None, protocol_version);
    let rate = builder.round_seigniorage_rate(None, protocol_version);
    let expected_total_reward = rate * initial_supply;
    let expected_total_reward_integer = expected_total_reward.to_integer();
    assert_eq!(total_payout, expected_total_reward_integer);

    // advance eras
    for _ in 0..=builder.get_auction_delay() {
        let step_request = StepRequestBuilder::new()
            .with_parent_state_hash(builder.get_post_state_hash())
            .with_protocol_version(ProtocolVersion::V1_0_0)
            .with_next_era_id(builder.get_era().successor())
            .with_run_auction(true)
            .build();

        assert!(
            builder.step(step_request).is_success(),
            "must execute step successfully"
        );
    }

    let mut rewards = BTreeMap::new();
    rewards.insert(VALIDATOR_1.clone(), vec![total_payout]);

    let distribute_request = ExecuteRequestBuilder::contract_call_by_hash(
        *SYSTEM_ADDR,
        builder.get_auction_contract_hash(),
        METHOD_DISTRIBUTE,
        runtime_args! {
            ARG_ENTRY_POINT => METHOD_DISTRIBUTE,
            ARG_REWARDS_MAP => rewards
        },
    )
    .build();

    builder.exec(distribute_request).commit().expect_success();

    let default_commission_rate = Ratio::new(
        U512::from(VALIDATOR_1_DELEGATION_RATE),
        U512::from(DELEGATION_RATE_DENOMINATOR),
    );
    let reservation_commission_rate = Ratio::new(
        U512::from(VALIDATOR_1_RESERVATION_DELEGATION_RATE),
        U512::from(DELEGATION_RATE_DENOMINATOR),
    );
    let reward_multiplier = Ratio::new(total_delegator_stake, total_stake);
    let base_delegator_reward = expected_total_reward
        .checked_mul(&reward_multiplier)
        .expect("must get delegator reward");

    let delegator_1_expected_payout = {
        let reward_multiplier = Ratio::new(delegator_1_stake, total_delegator_stake);
        let delegator_1_reward = base_delegator_reward
            .checked_mul(&reward_multiplier)
            .unwrap();
        let commission = delegator_1_reward
            .checked_mul(&reservation_commission_rate)
            .unwrap();
        delegator_1_reward
            .checked_sub(&commission)
            .unwrap()
            .to_integer()
    };
    let delegator_2_expected_payout = {
        let reward_multiplier = Ratio::new(delegator_2_stake, total_delegator_stake);
        let delegator_2_reward = base_delegator_reward
            .checked_mul(&reward_multiplier)
            .unwrap();
        let commission = delegator_2_reward
            .checked_mul(&default_commission_rate)
            .unwrap();
        delegator_2_reward
            .checked_sub(&commission)
            .unwrap()
            .to_integer()
    };

    let delegator_1_actual_payout = {
        let delegator_stake_before = delegator_1_stake;
        let delegator_stake_after =
            get_delegator_staked_amount(&mut builder, VALIDATOR_1.clone(), DELEGATOR_1.clone());
        delegator_stake_after - delegator_stake_before
    };
    assert_eq!(delegator_1_actual_payout, delegator_1_expected_payout);

    let delegator_2_actual_payout = {
        let delegator_stake_before = delegator_2_stake;
        let delegator_stake_after =
            get_delegator_staked_amount(&mut builder, VALIDATOR_1.clone(), DELEGATOR_2.clone());
        delegator_stake_after - delegator_stake_before
    };
    assert_eq!(delegator_2_actual_payout, delegator_2_expected_payout);

    let validator_1_expected_payout = {
        let total_delegator_payout = delegator_1_expected_payout + delegator_2_expected_payout;
        let validators_part = expected_total_reward - Ratio::from(total_delegator_payout);
        validators_part.to_integer()
    };

    let validator_1_actual_payout = {
        let validator_stake_before = validator_stake;
        let validator_stake_after = get_validator_bid(&mut builder, VALIDATOR_1.clone())
            .expect("should have validator bid")
            .staked_amount();
        validator_stake_after - validator_stake_before
    };

    assert_eq!(validator_1_actual_payout, validator_1_expected_payout);

    let era_info = get_era_info(&mut builder);

    assert!(matches!(
        era_info.select(DELEGATOR_1.clone()).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_1 && *amount == delegator_1_expected_payout
    ));

    assert!(matches!(
        era_info.select(DELEGATOR_2.clone()).next(),
        Some(SeigniorageAllocation::Delegator { delegator_public_key, amount, .. })
        if *delegator_public_key == *DELEGATOR_2 && *amount == delegator_2_expected_payout
    ));
}
