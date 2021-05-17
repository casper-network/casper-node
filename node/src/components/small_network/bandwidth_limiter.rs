//! Bandwidth limiters
//!
//! Bandwidth limiters restrict the usable amount of bandwidth through slowing down the sending of
//! message by making each sender request an allowance for sending first.

use std::{collections::HashSet, sync::Arc, time::Duration};

use async_trait::async_trait;
use casper_types::PublicKey;
use tokio::{
    sync::{mpsc, oneshot},
    time::Instant,
};
use tracing::{debug, warn};

use crate::types::NodeId;

/// Amount of allowed bandwidth to buffer in `ClassBasedLimiter`.
const STORED_BUFFER_SECS: Duration = Duration::from_secs(2);

/// A bandwidth limiter.
///
/// Any sender for a specific connection is expected to call `create_handle` for a connection, and
/// using that handle to request a bandwidth allowance.
pub(crate) trait BandwidthLimiter: Send + Sync {
    /// Create a handle for a connection using the given peer and optional validator id.
    fn create_handle(
        &self,
        peer_id: NodeId,
        validator_id: Option<PublicKey>,
    ) -> Box<dyn BandwidthLimiterHandle>;

    /// Update the validator sets.
    fn update_validators(
        &self,
        active_validators: HashSet<PublicKey>,
        upcoming_validators: HashSet<PublicKey>,
    ) {
    }
}

/// A handle for connections that are bandwidth limited.
#[async_trait]
pub(crate) trait BandwidthLimiterHandle: Send + Sync {
    /// Waits until the sender is clear to send `num_bytes` additional bytes.
    async fn request_allowance(&self, num_bytes: u32);
}

/// An unlimited bandwidth "limiter".
///
/// Does not restrict outgoing bandwidth in any way (`create_handle` returns immediately).
#[derive(Debug)]
pub(super) struct Unlimited;

/// Handle for `Unlimited`.
struct UnlimitedHandle;

impl BandwidthLimiter for Unlimited {
    fn create_handle(
        &self,
        _peer_id: NodeId,
        _validator_id: Option<PublicKey>,
    ) -> Box<dyn BandwidthLimiterHandle> {
        Box::new(UnlimitedHandle)
    }
}

#[async_trait]
impl BandwidthLimiterHandle for UnlimitedHandle {
    async fn request_allowance(&self, _num_bytes: u32) {
        // No limit.
    }
}

/// A Bandwidth limiter dividing traffic into multiple classes based on their validator status.
///
/// Imposes a limit on non-validator traffic while not limiting active validator traffic at all.
#[derive(Debug)]
pub(super) struct ClassBasedLimiter {
    /// Sender for commands to the limiter.
    sender: mpsc::UnboundedSender<ClassBasedCommand>,
}

/// Bandwidth class for the `ClassBasedLimiter`.
enum BandwidthClass {
    /// Preferred traffic serving active validators.
    ActiveValidator,
    /// Traffic for upcoming validators that are not active yet.
    UpcomingValidator,
    /// Unclassified/low-priority traffic.
    Bulk,
}

/// Command sent to the `ClassBasedLimiter`.
enum ClassBasedCommand {
    /// Updates the set of active/upcoming validators.
    UpdateValidators {
        /// The new set of validators active in the current era.
        active_validators: HashSet<PublicKey>,
        /// The new set of validators in future eras.
        upcoming_validators: HashSet<PublicKey>,
    },
    /// Requests a certain amount of bandwidth.
    RequestBandwidth {
        /// Number of bytes requested.
        num_bytes: u32,
        /// Id of the requesting sender.
        id: Arc<BandwidthConsumerId>,
        /// Response channel.
        responder: oneshot::Sender<()>,
    },
    /// Shuts down the worker task
    Shutdown,
}

/// Handle for `ClassBasedLimiter`.
#[derive(Debug)]
struct ClassBasedHandle {
    /// Sender for commands.
    sender: mpsc::UnboundedSender<ClassBasedCommand>,
    /// Consumer ID for the sender holding this handle.
    consumer_id: Arc<BandwidthConsumerId>,
}

/// An identity for a bandwidth consumer.
#[derive(Debug)]
struct BandwidthConsumerId {
    /// The node ID data is sent to/from.
    peer_id: NodeId,
    /// The remote node's `validator_id`.
    validator_id: Option<PublicKey>,
}

impl ClassBasedLimiter {
    /// Creates a new class based limiter.
    ///
    /// Starts the background worker task as well.
    pub(crate) fn new(bytes_per_second: u32) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();

        tokio::spawn(worker(
            receiver,
            bytes_per_second,
            ((bytes_per_second as f64) * STORED_BUFFER_SECS.as_secs_f64()) as u32,
        ));

        ClassBasedLimiter { sender }
    }
}

impl BandwidthLimiter for ClassBasedLimiter {
    fn create_handle(
        &self,
        peer_id: NodeId,
        validator_id: Option<PublicKey>,
    ) -> Box<dyn BandwidthLimiterHandle> {
        Box::new(ClassBasedHandle {
            sender: self.sender.clone(),
            consumer_id: Arc::new(BandwidthConsumerId {
                peer_id,
                validator_id,
            }),
        })
    }

    fn update_validators(
        &self,
        active_validators: HashSet<PublicKey>,
        upcoming_validators: HashSet<PublicKey>,
    ) {
        if self
            .sender
            .send(ClassBasedCommand::UpdateValidators {
                active_validators,
                upcoming_validators,
            })
            .is_err()
        {
            debug!("could not update validator data set of limiter, channel closed");
        }
    }
}

#[async_trait]
impl BandwidthLimiterHandle for ClassBasedHandle {
    async fn request_allowance(&self, num_bytes: u32) {
        let (responder, waiter) = oneshot::channel();

        // Send a request to the limiter and await a response. If we do not receive one due to
        // errors, simply ignore it and do not restrict bandwidth.
        //
        // While ignoring it is suboptimal, it is likely that we can continue operation normally in
        // many circumstances, thus the message is downgraded from what would be a `warn!` to
        // `debug!`.
        if self
            .sender
            .send(ClassBasedCommand::RequestBandwidth {
                num_bytes,
                id: self.consumer_id.clone(),
                responder,
            })
            .is_err()
        {
            debug!("worker was shutdown, sending is unlimited");
        } else {
            if waiter.await.is_err() {
                debug!("failed to await bandwidth allowance, sending unlimited");
            }
        }
    }
}

impl Drop for ClassBasedLimiter {
    fn drop(&mut self) {
        if self.sender.send(ClassBasedCommand::Shutdown).is_err() {
            warn!("error sending shutdown command to class based limiter");
        }
    }
}

/// Background worker for the limiter.
///
/// Will permit any amount of current/future validator traffic, while restricting non-validators to
/// `bytes_per_second`, although with unlimited single-message burst. The latter guarantees that any
/// size message can be sent.
///
/// Stores up to `max_stored_bytes` during idle times to smooth this process.
async fn worker(
    mut receiver: mpsc::UnboundedReceiver<ClassBasedCommand>,
    bytes_per_second: u32,
    max_stored_bytes: u32,
) {
    let mut active_validators = HashSet::new();
    let mut upcoming_validators = HashSet::new();

    let mut bytes_available: i64 = 0;
    let mut last_refill: Instant = Instant::now();

    while let Some(msg) = receiver.recv().await {
        match msg {
            ClassBasedCommand::UpdateValidators {
                active_validators: new_active_validators,
                upcoming_validators: new_upcoming_validators,
            } => {
                active_validators = new_active_validators;
                upcoming_validators = new_upcoming_validators;
            }
            ClassBasedCommand::RequestBandwidth {
                num_bytes,
                id,
                responder,
            } => {
                let bandwidth_class = if let Some(ref validator_id) = id.validator_id {
                    if active_validators.contains(validator_id) {
                        BandwidthClass::ActiveValidator
                    } else if upcoming_validators.contains(validator_id) {
                        BandwidthClass::UpcomingValidator
                    } else {
                        BandwidthClass::Bulk
                    }
                } else {
                    BandwidthClass::Bulk
                };

                match bandwidth_class {
                    BandwidthClass::ActiveValidator | BandwidthClass::UpcomingValidator => {
                        // No limit imposed on validators.
                    }
                    BandwidthClass::Bulk => {
                        while bytes_available < 0 {
                            // Determine time delta since last refill.
                            let now = Instant::now();
                            let elapsed = Instant::now() - last_refill;
                            last_refill = now;

                            // Add the appropriate amount of bytes, capped at `max_stored_bytes`.
                            bytes_available += ((elapsed.as_nanos() * bytes_per_second as u128)
                                / 1_000_000_000)
                                as i64;
                            bytes_available = bytes_available.min(max_stored_bytes as i64);

                            // If we still do not have enough byte available, sleep until we do.
                            if bytes_available < 0 {
                                let estimated_time_remaining = Duration::from_millis(
                                    (-bytes_available) as u64 * 1000 / bytes_per_second as u64,
                                );

                                tokio::time::sleep(estimated_time_remaining).await;
                            }
                        }

                        // Subtract outgoing message.
                        bytes_available -= num_bytes as i64;
                    }
                }

                if responder.send(()).is_err() {
                    debug!("Bandwidth requester disappeared before we could answer.")
                }
            }
            ClassBasedCommand::Shutdown => {
                // Shutdown the channel, processing only the remaining messages.
                receiver.close();
            }
        }
    }
    debug!("class based worker exiting");
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, time::Duration};

    use tokio::time::Instant;

    use super::{BandwidthLimiter, ClassBasedLimiter, NodeId, PublicKey, Unlimited};
    use crate::crypto::AsymmetricKeyExt;

    /// Something that happens almost immediately, with some allowance for test jitter.
    const SHORT_TIME: Duration = Duration::from_millis(250);

    #[tokio::test]
    async fn unlimited_limiter_is_unlimited() {
        let mut rng = crate::new_rng();

        let unlimited = Unlimited;

        let handle = unlimited.create_handle(NodeId::random(&mut rng), None);

        let start = Instant::now();
        handle.request_allowance(0).await;
        handle.request_allowance(u32::MAX).await;
        handle.request_allowance(1).await;
        let end = Instant::now();

        assert!(end - start < SHORT_TIME);
    }

    #[tokio::test]
    async fn active_validator_is_unlimited() {
        let mut rng = crate::new_rng();

        let validator_id = PublicKey::random(&mut rng);
        let limiter = ClassBasedLimiter::new(1_000);

        let mut active_validators = HashSet::new();
        active_validators.insert(validator_id.clone());
        limiter.update_validators(active_validators, HashSet::new());

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

        let validator_id = PublicKey::random(&mut rng);
        let limiter = ClassBasedLimiter::new(1_000);

        limiter.update_validators(HashSet::new(), HashSet::new());

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

        let validator_id = PublicKey::random(&mut rng);
        let limiter = ClassBasedLimiter::new(1_000);

        let start = Instant::now();

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
    }
}
