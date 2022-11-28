use prometheus::{IntGauge, Registry};

use crate::unregister_metric;

/// Metrics for the block accumulator component.
#[derive(Debug)]
pub(super) struct Metrics {
    /// Total number of BlockAcceptors contained in the BlockAccumulator.
    pub(super) block_acceptors: IntGauge,
    /// Number of child block hashes that we know of and that will be used in order to request next
    /// blocks.
    pub(super) known_child_blocks: IntGauge,
    registry: Registry,
}

impl Metrics {
    /// Creates a new instance of the block accumulator metrics, using the given prefix.
    pub fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let block_acceptors = IntGauge::new(
            "block_accumulator_block_acceptors".to_string(),
            "number of block acceptors in the Block Accumulator".to_string(),
        )?;
        let known_child_blocks = IntGauge::new(
            "block_accumulator_known_child_blocks".to_string(),
            "number of blocks received by the Block Accumulator for which we know the hash of the child block".to_string(),
        )?;

        registry.register(Box::new(block_acceptors.clone()))?;
        registry.register(Box::new(known_child_blocks.clone()))?;

        Ok(Metrics {
            block_acceptors,
            known_child_blocks,
            registry: registry.clone(),
        })
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.block_acceptors);
        unregister_metric!(self.registry, self.known_child_blocks);
    }
}
