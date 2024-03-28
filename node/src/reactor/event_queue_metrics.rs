use std::collections::HashMap;

use itertools::Itertools;
use prometheus::{self, IntGauge, Registry};
use tracing::debug;

use crate::{
    reactor::{EventQueueHandle, QueueKind},
    utils::registered_metric::{RegisteredMetric, RegistryExt},
};

/// Metrics for event queue sizes.
#[derive(Debug)]
pub(super) struct EventQueueMetrics {
    /// Per queue kind gauges that measure number of event in the queue.
    event_queue_gauges: HashMap<QueueKind, RegisteredMetric<IntGauge>>,
    /// Total events count.
    event_total: RegisteredMetric<IntGauge>,
}

impl EventQueueMetrics {
    /// Initializes event queue sizes metrics.
    pub(super) fn new<REv: 'static>(
        registry: Registry,
        event_queue_handle: EventQueueHandle<REv>,
    ) -> Result<Self, prometheus::Error> {
        let mut event_queue_gauges = HashMap::new();
        for queue_kind in event_queue_handle.event_queues_counts().keys() {
            let key = format!("scheduler_queue_{}_count", queue_kind.metrics_name());
            let queue_event_counter = registry.new_int_gauge(
                key,
                format!(
                    "current number of events in the reactor {} queue",
                    queue_kind.metrics_name()
                ),
            )?;

            let result = event_queue_gauges.insert(*queue_kind, queue_event_counter);
            assert!(result.is_none(), "Map keys should not be overwritten.");
        }

        let event_total = registry.new_int_gauge(
            "scheduler_queue_total_count",
            "current total number of events in all reactor queues",
        )?;

        Ok(EventQueueMetrics {
            event_queue_gauges,
            event_total,
        })
    }

    /// Updates the event queues size metrics.
    /// NOTE: Count may be off by one b/c of the way locking works when elements are popped.
    /// It's fine for its purposes.
    pub(super) fn record_event_queue_counts<REv: 'static>(
        &self,
        event_queue_handle: &EventQueueHandle<REv>,
    ) {
        let event_queue_count = event_queue_handle.event_queues_counts();

        let total = event_queue_count.values().sum::<usize>() as i64;
        self.event_total.set(total);

        let event_counts: String = event_queue_count
            .iter()
            .sorted_by_key(|k| k.0)
            .map(|(queue, event_count)| {
                self.event_queue_gauges
                    .get(queue)
                    .map(|gauge| gauge.set(*event_count as i64))
                    .expect("queue exists.");
                format!("{}={}", queue, event_count)
            })
            .join(",");

        debug!(%total, %event_counts, "Collected new set of event queue sizes metrics.")
    }
}
