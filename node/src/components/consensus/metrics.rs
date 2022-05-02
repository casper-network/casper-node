use prometheus::{Gauge, IntGauge, Registry};

use casper_types::Timestamp;

use crate::{types::FinalizedBlock, unregister_metric};

/// Network metrics to track Consensus
#[derive(Debug)]
pub(super) struct Metrics {
    /// Gauge to track time between proposal and finalization.
    finalization_time: Gauge,
    /// Amount of finalized blocks.
    finalized_block_count: IntGauge,
    /// Timestamp of the most recently accepted block payload.
    time_of_last_proposed_block: IntGauge,
    /// Timestamp of the most recently finalized block.
    time_of_last_finalized_block: IntGauge,
    /// The Current era.
    pub(super) current_era: IntGauge,
    /// registry component.
    registry: Registry,
}

impl Metrics {
    pub(super) fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let finalization_time = Gauge::new(
            "finalization_time",
            "the amount of time, in milliseconds, between proposal and finalization of the latest finalized block",
        )?;
        let finalized_block_count =
            IntGauge::new("amount_of_blocks", "the number of blocks finalized so far")?;
        let time_of_last_proposed_block = IntGauge::new(
            "time_of_last_block_payload",
            "timestamp of the most recently accepted block payload",
        )?;
        let time_of_last_finalized_block = IntGauge::new(
            "time_of_last_finalized_block",
            "timestamp of the most recently finalized block",
        )?;
        let current_era = IntGauge::new("current_era", "the current era")?;
        registry.register(Box::new(finalization_time.clone()))?;
        registry.register(Box::new(finalized_block_count.clone()))?;
        registry.register(Box::new(current_era.clone()))?;
        registry.register(Box::new(time_of_last_proposed_block.clone()))?;
        registry.register(Box::new(time_of_last_finalized_block.clone()))?;
        Ok(Metrics {
            finalization_time,
            finalized_block_count,
            time_of_last_proposed_block,
            time_of_last_finalized_block,
            current_era,
            registry: registry.clone(),
        })
    }

    /// Updates the metrics based on a newly finalized block.
    pub(super) fn finalized_block(&mut self, finalized_block: &FinalizedBlock) {
        let time_since_block_payload = finalized_block.timestamp().elapsed().millis() as f64;
        self.finalization_time.set(time_since_block_payload);
        self.time_of_last_finalized_block
            .set(finalized_block.timestamp().millis() as i64);
        self.finalized_block_count
            .set(finalized_block.height() as i64);
    }

    /// Updates the metrics and records a newly proposed block.
    pub(super) fn proposed_block(&mut self) {
        self.time_of_last_proposed_block
            .set(Timestamp::now().millis() as i64);
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.finalization_time);
        unregister_metric!(self.registry, self.finalized_block_count);
        unregister_metric!(self.registry, self.current_era);
        unregister_metric!(self.registry, self.time_of_last_finalized_block);
        unregister_metric!(self.registry, self.time_of_last_proposed_block);
    }
}
