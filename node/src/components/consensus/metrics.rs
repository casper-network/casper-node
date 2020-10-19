use prometheus::{Gauge, IntCounter, Registry};

/// Network metrics to track Consensus
#[derive(Debug)]
pub struct ConsensusMetrics {
    /// Gauge to track time between proposal and finalization.
    pub finalization_time: Gauge,
    /// Amount of finalized blocks.
    pub finalized_block_count: IntCounter,
    /// Timestamp of the most recently accepted proto block.
    pub time_since_proto_block: Gauge,
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
        let time_since_proto_block = Gauge::new(
            "time_of_last_proto_block",
            "timestamp of the most recently accepted proto block",
        )?;
        registry.register(Box::new(finalization_time.clone()))?;
        registry.register(Box::new(finalized_block_count.clone()))?;
        Ok(ConsensusMetrics {
            finalization_time,
            finalized_block_count,
            time_since_proto_block,
            registry: registry.clone(),
        })
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
