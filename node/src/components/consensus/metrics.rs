use prometheus::{Gauge, IntCounter, Registry};

use crate::types::{FinalizedBlock, Timestamp};

/// Network metrics to track Consensus
#[derive(Debug)]
pub struct ConsensusMetrics {
    /// Gauge to track time between proposal and finalization.
    finalization_time: Gauge,
    /// Amount of finalized blocks.
    finalized_block_count: IntCounter,
    /// Timestamp of the most recently accepted proto block.
    time_of_last_proposed_block: Gauge,
    /// registry component.
    registry: Registry,
}

impl ConsensusMetrics {
    pub fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let finalization_time = Gauge::new(
            "finalization_time",
            "the amount of time, in milliseconds, between proposal and finalization of a block",
        )?;
        let finalized_block_count =
            IntCounter::new("amount_of_blocks", "the number of blocks finalized so far")?;
        let time_of_last_proposed_block = Gauge::new(
            "time_of_last_proto_block",
            "timestamp of the most recently accepted proto block",
        )?;
        registry.register(Box::new(finalization_time.clone()))?;
        registry.register(Box::new(finalized_block_count.clone()))?;
        Ok(ConsensusMetrics {
            finalization_time,
            finalized_block_count,
            time_of_last_proposed_block,
            registry: registry.clone(),
        })
    }

    /// Updates the metrics based on a newly finalized block.
    pub(crate) fn finalized_block(&mut self, finalized_block: &FinalizedBlock) {
        let time_since_proto_block = finalized_block.timestamp().elapsed().millis() as f64;
        self.finalization_time.set(time_since_proto_block);
        self.finalized_block_count.inc();
    }

    /// Updates the metrics and records a newly proposed block.
    pub(crate) fn proposed_block(&mut self) {
        self.time_of_last_proposed_block
            .set(Timestamp::now().millis() as f64 / 1000.00);
    }
}

impl Drop for ConsensusMetrics {
    fn drop(&mut self) {
        self.registry
            .unregister(Box::new(self.finalization_time.clone()))
            .expect("did not expect deregistering rate to fail");
        self.registry
            .unregister(Box::new(self.finalized_block_count.clone()))
            .expect("did not expect deregisterting amount to fail");
    }
}
