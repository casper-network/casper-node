//! Testing utilities.
//!
//! Contains various parts and components to aid writing tests and simulations using the
//! `casper-node` library.

mod condition_check_reactor;
pub mod network;
mod test_rng;

use std::{
    collections::HashSet,
    fmt::Debug,
    sync::atomic::{AtomicU16, Ordering},
};

use serde::{de::DeserializeOwned, Serialize};

use crate::logging;
pub(crate) use condition_check_reactor::ConditionCheckReactor;
pub(crate) use test_rng::TestRng;

// Lower bound for the port, below there's a high chance of hitting a system service.
const PORT_LOWER_BOUND: u16 = 10_000;

pub fn rmp_serde_roundtrip<T: Serialize + DeserializeOwned + Eq + Debug>(value: &T) {
    let serialized = rmp_serde::to_vec(value).unwrap();
    let deserialized = rmp_serde::from_read_ref(serialized.as_slice()).unwrap();
    assert_eq!(*value, deserialized);
}

/// Create an unused port on localhost.
#[allow(clippy::assertions_on_constants)]
pub(crate) fn unused_port_on_localhost() -> u16 {
    // Prime used for the LCG.
    const PRIME: u16 = 54101;
    // Generating member of prime group.
    const GENERATOR: u16 = 35892;

    // This assertion can never fail, but the compiler should output a warning if the constants
    // combined exceed the valid values of `u16`.
    assert!(PORT_LOWER_BOUND + PRIME + 10 < u16::MAX);

    // Poor man's linear congurential random number generator:
    static RNG_STATE: AtomicU16 = AtomicU16::new(GENERATOR);

    // Attempt 10k times to swap the atomic with the next generator value.
    for _ in 0..10_000 {
        if let Ok(fresh_port) =
            RNG_STATE.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |state| {
                let new_value = (state as u32 + GENERATOR as u32) % (PRIME as u32);
                Some(new_value as u16 + PORT_LOWER_BOUND)
            })
        {
            return fresh_port;
        }
    }

    // Give up - likely we're in a very tight, oscillatory race with another thread.
    panic!("could not generate random new port after 10_000 tries");
}

/// Sets up logging for testing.
///
/// Can safely be called multiple times.
pub(crate) fn init_logging() {
    // TODO: Write logs to file by default for each test.
    logging::init()
        // Ignore the return value, setting the global subscriber will fail if `init_logging` has
        // been called before, which we don't care about.
        .ok();
}

/// Test that the random port generator produce at least 40k values without duplicates.
#[test]
fn test_random_port_gen() {
    const NUM_ROUNDS: usize = 40_000;

    let values: HashSet<_> = (0..NUM_ROUNDS)
        .map(|_| {
            let port = unused_port_on_localhost();
            assert!(port >= PORT_LOWER_BOUND);
            port
        })
        .collect();

    assert_eq!(values.len(), NUM_ROUNDS);
}
