use config::{Config, ACCELERATION_PARAMETER, MAX_FAILED_ROUNDS, NUM_ROUNDS_TO_CONSIDER};

use casper_types::{TimeDiff, Timestamp};

use crate::components::consensus::{
    cl_context::ClContext,
    protocols::highway::round_success_meter::{config, round_index},
};

const TEST_ROUND_LEN: TimeDiff = TimeDiff::from_millis(1 << 13);
const TEST_MIN_ROUND_LEN: TimeDiff = TimeDiff::from_millis(1 << 8);
const TEST_MAX_ROUND_LEN: TimeDiff = TimeDiff::from_millis(1 << 19);

#[test]
fn new_length_steady() {
    let round_success_meter: super::RoundSuccessMeter<ClContext> = super::RoundSuccessMeter::new(
        TEST_ROUND_LEN,
        TEST_MIN_ROUND_LEN,
        TEST_MAX_ROUND_LEN,
        Timestamp::now(),
        Config::default(),
    );
    assert_eq!(round_success_meter.new_length(), TEST_ROUND_LEN);
}

#[test]
fn new_length_slow_down() {
    let mut round_success_meter: super::RoundSuccessMeter<ClContext> =
        super::RoundSuccessMeter::new(
            TEST_ROUND_LEN,
            TEST_MIN_ROUND_LEN,
            TEST_MAX_ROUND_LEN,
            Timestamp::now(),
            Config::default(),
        );
    // If there have been more rounds of failure than MAX_FAILED_ROUNDS, slow down
    round_success_meter.rounds = vec![false; MAX_FAILED_ROUNDS + 1].into();
    assert_eq!(round_success_meter.new_length(), TEST_ROUND_LEN * 2);
}

#[test]
fn new_length_can_not_slow_down_because_max_round_len() {
    // If the round length is the same as the maximum round length, can't go up
    let mut round_success_meter: super::RoundSuccessMeter<ClContext> =
        super::RoundSuccessMeter::new(
            TEST_MAX_ROUND_LEN,
            TEST_MIN_ROUND_LEN,
            TEST_MAX_ROUND_LEN,
            Timestamp::now(),
            Config::default(),
        );
    // If there have been more rounds of failure than MAX_FAILED_ROUNDS, slow down -- but can't
    // slow down because of ceiling
    round_success_meter.rounds = vec![false; MAX_FAILED_ROUNDS + 1].into();
    assert_eq!(round_success_meter.new_length(), TEST_MAX_ROUND_LEN);
}

#[test]
fn new_length_speed_up() {
    // If there's been enough successful rounds and it's an acceleration round, speed up
    let mut round_success_meter: super::RoundSuccessMeter<ClContext> =
        super::RoundSuccessMeter::new(
            TEST_ROUND_LEN,
            TEST_MIN_ROUND_LEN,
            TEST_MAX_ROUND_LEN,
            Timestamp::now(),
            Config::default(),
        );
    round_success_meter.rounds = vec![true; NUM_ROUNDS_TO_CONSIDER].into();
    // Increase our round index until we are at an acceleration round
    loop {
        let current_round_index = round_index(
            round_success_meter.current_round_id,
            round_success_meter.current_round_len,
        );
        if current_round_index % ACCELERATION_PARAMETER == 0 {
            break;
        };
        round_success_meter.current_round_id += TimeDiff::from_millis(1);
    }
    assert_eq!(round_success_meter.new_length(), TEST_ROUND_LEN / 2);
}

#[test]
fn new_length_can_not_speed_up_because_min_round_len() {
    // If there's been enough successful rounds and it's an acceleration round, but we are
    // already at the smallest round length possible, stay at the current round length
    let mut round_success_meter: super::RoundSuccessMeter<ClContext> =
        super::RoundSuccessMeter::new(
            TEST_MIN_ROUND_LEN,
            TEST_MIN_ROUND_LEN,
            TEST_MAX_ROUND_LEN,
            Timestamp::now(),
            Config::default(),
        );
    round_success_meter.rounds = vec![true; NUM_ROUNDS_TO_CONSIDER].into();
    // Increase our round index until we are at an acceleration round
    loop {
        let current_round_index = round_index(
            round_success_meter.current_round_id,
            round_success_meter.current_round_len,
        );
        if current_round_index % ACCELERATION_PARAMETER == 0 {
            break;
        };
        round_success_meter.current_round_id += TimeDiff::from_millis(1);
    }
    assert_eq!(round_success_meter.new_length(), TEST_MIN_ROUND_LEN);
}
