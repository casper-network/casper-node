//! Resource limiters
//!
//! Resource limiters restrict the usable amount of a resource through slowing down the request rate
//! by making each user request an allowance first.

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};

use prometheus::Counter;
use tokio::sync::Mutex;
use tracing::{error, trace};

use casper_types::{EraId, PublicKey};

use crate::types::{NodeId, ValidatorMatrix};

/// Amount of resource allowed to buffer in `Limiter`.
const STORED_BUFFER_SECS: Duration = Duration::from_secs(2);

/// A limiter dividing resources into two classes based on their validator status.
///
/// Any consumer of a specific resource is expected to call `create_handle` for every peer and use
/// the returned handle to request a access to a resource.
///
/// Imposes a limit on non-validator resources while not limiting active validator resources at all.
#[derive(Debug)]
pub(super) struct Limiter {
    /// Shared data across all handles.
    data: Arc<LimiterData>,
    /// Set of active and upcoming validators shared across all handles.
    validator_matrix: ValidatorMatrix,
}

impl Limiter {
    /// Creates a new class based limiter.
    ///
    /// Starts the background worker task as well.
    pub(super) fn new(
        resources_per_second: u32,
        wait_time_sec: Counter,
        validator_matrix: ValidatorMatrix,
    ) -> Self {
        Limiter {
            data: Arc::new(LimiterData::new(resources_per_second, wait_time_sec)),
            validator_matrix,
        }
    }

    /// Create a handle for a connection using the given peer and optional validator id.
    pub(super) fn create_handle(
        &self,
        peer_id: NodeId,
        validator_id: Option<PublicKey>,
    ) -> LimiterHandle {
        if let Some(public_key) = validator_id.as_ref().cloned() {
            match self.data.connected_validators.write() {
                Ok(mut connected_validators) => {
                    let _ = connected_validators.insert(peer_id, public_key);
                }
                Err(_) => {
                    error!(
                        "could not update connected validator data set of limiter, lock poisoned"
                    );
                }
            }
        }
        LimiterHandle {
            data: self.data.clone(),
            validator_matrix: self.validator_matrix.clone(),
            consumer_id: ConsumerId {
                peer_id,
                validator_id,
            },
        }
    }

    pub(super) fn remove_connected_validator(&self, peer_id: &NodeId) {
        match self.data.connected_validators.write() {
            Ok(mut connected_validators) => {
                let _ = connected_validators.remove(peer_id);
            }
            Err(_) => {
                error!(
                    "could not remove connected validator from data set of limiter, lock poisoned"
                );
            }
        }
    }

    pub(super) fn is_validator_in_era(&self, era: EraId, peer_id: &NodeId) -> Option<bool> {
        let public_key = match self.data.connected_validators.read() {
            Ok(connected_validators) => connected_validators.get(peer_id)?.clone(),
            Err(_) => {
                error!("could not read from connected_validators of limiter, lock poisoned");
                return None;
            }
        };

        self.validator_matrix.is_validator_in_era(era, &public_key)
    }
}

/// The limiter's state.
#[derive(Debug)]
struct LimiterData {
    /// Number of resource units to allow for non-validators per second.
    resources_per_second: u32,
    /// A mapping from node IDs to public keys of validators to which we have an outgoing
    /// connection.
    connected_validators: RwLock<HashMap<NodeId, PublicKey>>,
    /// Information about available resources.
    resources: Mutex<ResourceData>,
    /// Total time spent waiting.
    wait_time_sec: Counter,
}

/// Resource data.
#[derive(Debug)]
struct ResourceData {
    /// How many resource units are buffered.
    ///
    /// May go negative in the case of a deficit.
    available: i64,
    /// Last time resource data was refilled.
    last_refill: Instant,
}

impl LimiterData {
    /// Creates a new set of class based limiter data.
    ///
    /// Initial resources will be initialized to 0, with the last refill set to the current time.
    fn new(resources_per_second: u32, wait_time_sec: Counter) -> Self {
        LimiterData {
            resources_per_second,
            connected_validators: Default::default(),
            resources: Mutex::new(ResourceData {
                available: 0,
                last_refill: Instant::now(),
            }),
            wait_time_sec,
        }
    }
}

/// Peer class for the `Limiter`.
enum PeerClass {
    /// A validator.
    Validator,
    /// Unclassified/low-priority peer.
    NonValidator,
}

/// A per-peer handle for `Limiter`.
#[derive(Debug)]
pub(super) struct LimiterHandle {
    /// Data shared between handles and limiter.
    data: Arc<LimiterData>,
    /// Set of active and upcoming validators.
    validator_matrix: ValidatorMatrix,
    /// Consumer ID for the sender holding this handle.
    consumer_id: ConsumerId,
}

impl LimiterHandle {
    /// Waits until the requester is allocated `amount` additional resources.
    pub(super) async fn request_allowance(&self, amount: u32) {
        // As a first step, determine the peer class by checking if our id is in the validator set.

        if self.validator_matrix.is_empty() {
            // It is likely that we have not been initialized, thus no node is getting the
            // reserved resources. In this case, do not limit at all.
            trace!("empty set of validators, not limiting resources at all");

            return;
        }

        let peer_class = if let Some(ref validator_id) = self.consumer_id.validator_id {
            if self
                .validator_matrix
                .is_validator_in_any_of_latest_n_eras(3, validator_id)
            {
                PeerClass::Validator
            } else {
                PeerClass::NonValidator
            }
        } else {
            PeerClass::NonValidator
        };

        match peer_class {
            PeerClass::Validator => {
                // No limit imposed on validators.
            }
            PeerClass::NonValidator => {
                if self.data.resources_per_second == 0 {
                    return;
                }

                let max_stored_resource = ((self.data.resources_per_second as f64)
                    * STORED_BUFFER_SECS.as_secs_f64())
                    as u32;

                // We are a low-priority sender. Obtain a lock on the resources and wait an
                // appropriate amount of time to fill them up.
                {
                    let mut resources = self.data.resources.lock().await;

                    while resources.available < 0 {
                        // Determine time delta since last refill.
                        let now = Instant::now();
                        let elapsed = now - resources.last_refill;
                        resources.last_refill = now;

                        // Add appropriate amount of resources, capped at `max_stored_bytes`. We
                        // are still maintaining the lock here to avoid issues with other
                        // low-priority requesters.
                        resources.available += ((elapsed.as_nanos()
                            * self.data.resources_per_second as u128)
                            / 1_000_000_000) as i64;
                        resources.available = resources.available.min(max_stored_resource as i64);

                        // If we do not have enough resources available, sleep until we do.
                        if resources.available < 0 {
                            let estimated_time_remaining = Duration::from_millis(
                                (-resources.available) as u64 * 1000
                                    / self.data.resources_per_second as u64,
                            );

                            // Note: This sleep call is the reason we are using a tokio mutex
                            //       instead of a regular `std` one, as we are holding it across the
                            //       await point here.
                            tokio::time::sleep(estimated_time_remaining).await;
                            self.data
                                .wait_time_sec
                                .inc_by(estimated_time_remaining.as_secs_f64());
                        }
                    }

                    // Subtract the amount. If available resources go negative as a result, it
                    // is the next sender's problem.
                    resources.available -= amount as i64;
                }
            }
        }
    }
}

/// An identity for a consumer.
#[derive(Debug)]
struct ConsumerId {
    /// The peer's ID.
    #[allow(dead_code)]
    peer_id: NodeId,
    /// The remote node's `validator_id`.
    validator_id: Option<PublicKey>,
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use casper_types::SecretKey;
    use num_rational::Ratio;
    use prometheus::Counter;
    use tokio::time::Instant;

    use super::{Limiter, NodeId, PublicKey};
    use crate::{testing::init_logging, types::ValidatorMatrix};

    /// Something that happens almost immediately, with some allowance for test jitter.
    const SHORT_TIME: Duration = Duration::from_millis(250);

    /// Creates a new counter for testing.
    fn new_wait_time_sec() -> Counter {
        Counter::new("test_time_waiting", "wait time counter used in tests")
            .expect("could not create new counter")
    }

    #[tokio::test]
    async fn unlimited_limiter_is_unlimited() {
        // let mut rng = crate::new_rng();
        //
        // let unlimited = Unlimited;
        //
        // let handle = unlimited.create_handle(NodeId::random(&mut rng), None);
        //
        // let start = Instant::now();
        // handle.request_allowance(0).await;
        // handle.request_allowance(u32::MAX).await;
        // handle.request_allowance(1).await;
        // let end = Instant::now();
        //
        // assert!(end - start < SHORT_TIME);
        panic!("ensure behaviour of setting limit to 0 is maintained");
    }

    #[tokio::test]
    async fn active_validator_is_unlimited() {
        let mut rng = crate::new_rng();

        let secret_key = SecretKey::random(&mut rng);
        let validator_id = PublicKey::from(&secret_key);
        // We insert one unrelated active validator to avoid triggering the automatic disabling of
        // the limiter in case there are no active validators.
        let validator_matrix =
            ValidatorMatrix::new_with_validator(Arc::new(secret_key), validator_id.clone());
        let limiter = Limiter::new(1_000, new_wait_time_sec(), validator_matrix);

        let handle = limiter.create_handle(NodeId::random(&mut rng), Some(validator_id));

        let start = Instant::now();
        handle.request_allowance(0).await;
        handle.request_allowance(u32::MAX).await;
        handle.request_allowance(1).await;
        let end = Instant::now();

        assert!(end - start < SHORT_TIME);
    }

    #[tokio::test]
    async fn inactive_validator_limited() {
        let mut rng = crate::new_rng();

        let secret_key = SecretKey::random(&mut rng);
        let validator_id = PublicKey::from(&secret_key);
        // We insert one unrelated active validator to avoid triggering the automatic disabling of
        // the limiter in case there are no active validators.
        let validator_matrix =
            ValidatorMatrix::new_with_validator(Arc::new(secret_key), validator_id.clone());
        let limiter = Limiter::new(1_000, new_wait_time_sec(), validator_matrix);

        // Try with non-validators or unknown nodes.
        let handles = vec![
            limiter.create_handle(NodeId::random(&mut rng), Some(validator_id)),
            limiter.create_handle(NodeId::random(&mut rng), None),
        ];

        for handle in handles {
            let start = Instant::now();

            // Send 9_0001 bytes, we expect this to take roughly 15 seconds.
            handle.request_allowance(1000).await;
            handle.request_allowance(1000).await;
            handle.request_allowance(1000).await;
            handle.request_allowance(2000).await;
            handle.request_allowance(4000).await;
            handle.request_allowance(1).await;
            let end = Instant::now();

            let diff = end - start;
            assert!(diff >= Duration::from_secs(9));
            assert!(diff <= Duration::from_secs(10));
        }
    }

    #[tokio::test]
    async fn nonvalidators_parallel_limited() {
        let mut rng = crate::new_rng();

        let secret_key = SecretKey::random(&mut rng);
        let validator_id = PublicKey::from(&secret_key);
        let wait_metric = new_wait_time_sec();

        let start = Instant::now();

        // We insert one unrelated active validator to avoid triggering the automatic disabling of
        // the limiter in case there are no active validators.
        let validator_matrix =
            ValidatorMatrix::new_with_validator(Arc::new(secret_key), validator_id.clone());
        let limiter = Limiter::new(1_000, new_wait_time_sec(), validator_matrix);

        // Parallel test, 5 non-validators sharing 1000 bytes per second. Each sends 1001 bytes, so
        // total time is expected to be just over 5 seconds.
        let join_handles = (0..5)
            .map(|_| limiter.create_handle(NodeId::random(&mut rng), Some(validator_id.clone())))
            .map(|handle| {
                tokio::spawn(async move {
                    handle.request_allowance(500).await;
                    handle.request_allowance(150).await;
                    handle.request_allowance(350).await;
                    handle.request_allowance(1).await;
                })
            });

        for join_handle in join_handles {
            join_handle.await.expect("could not join task");
        }

        let end = Instant::now();
        let diff = end - start;
        assert!(diff >= Duration::from_secs(5));
        assert!(diff <= Duration::from_secs(6));

        // Ensure metrics recorded the correct number of seconds.
        assert!(
            wait_metric.get() <= 6.0,
            "wait metric is too large: {}",
            wait_metric.get()
        );

        // Note: The limiting will not apply to all data, so it should be slightly below 5 seconds.
        assert!(
            wait_metric.get() >= 4.5,
            "wait metric is too small: {}",
            wait_metric.get()
        );
    }

    #[tokio::test]
    async fn inactive_validators_unlimited_when_no_validators_known() {
        init_logging();

        let mut rng = crate::new_rng();

        let secret_key = SecretKey::random(&mut rng);
        let validator_id = PublicKey::from(&secret_key);
        let wait_metric = new_wait_time_sec();
        let limiter = Limiter::new(
            1_000,
            wait_metric.clone(),
            ValidatorMatrix::new(Ratio::new(1, 3), Arc::new(secret_key), validator_id.clone()),
        );

        // Try with non-validators or unknown nodes.
        let handles = vec![
            limiter.create_handle(NodeId::random(&mut rng), Some(validator_id)),
            limiter.create_handle(NodeId::random(&mut rng), None),
        ];

        for handle in handles {
            let start = Instant::now();

            // Send 9_0001 bytes, should now finish instantly.
            handle.request_allowance(1000).await;
            handle.request_allowance(1000).await;
            handle.request_allowance(1000).await;
            handle.request_allowance(2000).await;
            handle.request_allowance(4000).await;
            handle.request_allowance(1).await;
            let end = Instant::now();

            let diff = end - start;
            assert!(diff <= SHORT_TIME);
        }

        // There should have been no time spent waiting.
        assert!(
            wait_metric.get() < SHORT_TIME.as_secs_f64(),
            "wait_metric is too large: {}",
            wait_metric.get()
        );
    }

    /// Regression test for #2929.
    #[tokio::test]
    async fn throttling_of_non_validators_does_not_affect_validators() {
        init_logging();

        let mut rng = crate::new_rng();

        let secret_key = SecretKey::random(&mut rng);
        let validator_id = PublicKey::from(&secret_key);
        let validator_matrix =
            ValidatorMatrix::new_with_validator(Arc::new(secret_key), validator_id.clone());
        let limiter = Limiter::new(1_000, new_wait_time_sec(), validator_matrix);

        let non_validator_handle = limiter.create_handle(NodeId::random(&mut rng), None);
        let validator_handle = limiter.create_handle(NodeId::random(&mut rng), Some(validator_id));

        // We request a large resource at once using a non-validator handle. At the same time,
        // validator requests should be still served, even while waiting for the long-delayed
        // request still blocking.
        let start = Instant::now();
        let background_nv_request = tokio::spawn(async move {
            non_validator_handle.request_allowance(5000).await;
            non_validator_handle.request_allowance(5000).await;

            Instant::now()
        });

        // Allow for a little bit of time to pass to ensure the background task is running.
        tokio::time::sleep(Duration::from_secs(1)).await;

        validator_handle.request_allowance(10000).await;
        validator_handle.request_allowance(10000).await;

        let v_finished = Instant::now();

        let nv_finished = background_nv_request
            .await
            .expect("failed to join background nv task");

        let nv_completed = nv_finished.duration_since(start);
        assert!(
            nv_completed >= Duration::from_millis(4500),
            "non-validator did not delay sufficiently: {:?}",
            nv_completed
        );

        let v_completed = v_finished.duration_since(start);
        assert!(
            v_completed <= Duration::from_millis(1500),
            "validator did not finish quickly enough: {:?}",
            v_completed
        );
    }
}
