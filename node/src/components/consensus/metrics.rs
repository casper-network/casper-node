use prometheus::{Gauge, IntGauge, Registry};

use casper_types::Timestamp;

use crate::{
    types::FinalizedBlock,
    utils::registered_metric::{RegisteredMetric, RegistryExt},
};

/// Network metrics to track Consensus
#[derive(Debug)]
pub(super) struct Metrics {
    /// Gauge to track time between proposal and finalization.
    finalization_time: RegisteredMetric<Gauge>,
    /// Amount of finalized blocks.
    finalized_block_count: RegisteredMetric<IntGauge>,
    /// Timestamp of the most recently accepted block payload.
    time_of_last_proposed_block: RegisteredMetric<IntGauge>,
    /// Timestamp of the most recently finalized block.
    time_of_last_finalized_block: RegisteredMetric<IntGauge>,
    /// The current era.
    pub(super) consensus_current_era: RegisteredMetric<IntGauge>,
}

impl Metrics {
    pub(super) fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let finalization_time = registry.new_gauge(
            "finalization_time",
            "the amount of time, in milliseconds, between proposal and finalization of the latest finalized block",
        )?;
        let finalized_block_count =
            registry.new_int_gauge("amount_of_blocks", "the number of blocks finalized so far")?;
        let time_of_last_proposed_block = registry.new_int_gauge(
            "time_of_last_block_payload",
            "timestamp of the most recently accepted block payload",
        )?;
        let time_of_last_finalized_block = registry.new_int_gauge(
            "time_of_last_finalized_block",
            "timestamp of the most recently finalized block",
        )?;
        let consensus_current_era =
            registry.new_int_gauge("consensus_current_era", "the current era in consensus")?;

        Ok(Metrics {
            finalization_time,
            finalized_block_count,
            time_of_last_proposed_block,
            time_of_last_finalized_block,
            consensus_current_era,
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
