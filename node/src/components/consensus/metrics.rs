use crate::{types::FinalizedBlock, unregister_metric};
use casper_types::Timestamp;
use prometheus::{Gauge, Histogram, HistogramOpts, IntGauge, Registry};
use std::time::Duration;

/// Network metrics to track Consensus
#[derive(Debug)]
pub(super) struct Metrics {
    /// The current era.
    pub(super) consensus_current_era: IntGauge,
    /// Gauge to track time between proposal and finalization.
    finalization_time: Gauge,
    /// Amount of finalized blocks.
    finalized_block_count: IntGauge,
    /// Timestamp of the most recently accepted block payload.
    time_of_last_proposed_block: IntGauge,
    /// Timestamp of the most recently finalized block.
    time_of_last_finalized_block: IntGauge,

    /// INTENTIONALLY SKIPPED BLOCKS METRICS
    /// Histogram of frequency of skipped proposals.
    skipped_empty_proposal_hist: Histogram,
    /// Count of skipped empty proposal.
    skipped_empty_proposal: Gauge,
    /// Timestamp of skipped empty proposal.
    time_of_last_skipped_empty_proposal: IntGauge,

    /// Registry component.
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
        let time_of_last_finalized_block = IntGauge::new(
            "time_of_last_finalized_block",
            "timestamp of the most recently finalized block",
        )?;
        let consensus_current_era =
            IntGauge::new("consensus_current_era", "the current era in consensus")?;
        let time_of_last_proposed_block = IntGauge::new(
            "time_of_last_block_payload",
            "timestamp of the most recently accepted block payload",
        )?;
        registry.register(Box::new(finalization_time.clone()))?;
        registry.register(Box::new(finalized_block_count.clone()))?;
        registry.register(Box::new(consensus_current_era.clone()))?;
        registry.register(Box::new(time_of_last_proposed_block.clone()))?;
        registry.register(Box::new(time_of_last_finalized_block.clone()))?;

        // SKIPPED EMPTY PROPOSAL METRICS
        // HISTOGRAM, COUNT, and MOST RECENT INSTANCE (timestamp since epoch)

        /*

            In Prometheus, when using exponential_buckets to define histogram buckets, the count
            variable specifies the number of buckets to generate.
            Specifically:

                The exponential_buckets(start, factor, count) function creates count buckets
                for a histogram.

            The start parameter defines the upper bound of the lowest bucket.
            The factor parameter determines the multiplier for each subsequent bucket's upper bound.
            Each following bucket's upper bound is factor times the previous bucket's upper bound.
            The count variable directly dictates how many of these exponentially increasing buckets
            will be created, excluding the implicit +Inf bucket which is always present for
            histograms and handles observations exceeding the highest defined bucket.

            Example: `exponential_buckets(100, 1.2, 3)`

            This would create 3 buckets: Upper bound: 100, Upper bound: 100 * 1.2 = 120,
            and Upper bound: 120 * 1.2 = 144.

            Plus the +Inf bucket.
        */

        let skipped_empty_proposal_hist = Histogram::with_opts(
            HistogramOpts::new(
                "skipped_empty_proposal_hist",
                "histogram of skipped proposals start: 1s, factor: 1.75, buckets: 25",
            )
            // Create exponential buckets from one second to 1 minute with an off-step factor.
            // BUCKETS (set up to accommodate a range of block times from 1s to 32s):
            //  1s, 1.75s, 3.06s, 5.35s. 9.37s, 16.41s, 28.72s, 50.26s, 87.96s, 153.93s,
            //  269.38s, 471.43s, 825s, 1443.75s, 2526.57s, 4421.51s, 7737.64s, 13540.87s,
            //  23696.53s, 41468.93s, 72570.64s, 126998.62s, 222247.59s, 388933.29s, 680633.26s
            // A given node's entries should be consistent with their configured
            // empty_proposal_tolerance_interval setting and the chainspec minimum_block_time,
            // allowing for spillover into the smallest eligible bucket.
            // i.e. if the block time is 1s and config'd value is 0s there should be NO entries.
            // however if config'd value is 10s, there may be entries in the 1s to 16.41s buckets
            // and no entries in the 28.72s and up buckets; there may be entries in the 16.41s
            // bucket because it is the smallest bucket skips in the 9.38s to 10s range can fit in.
            .buckets(prometheus::exponential_buckets(1.0, 1.75, 25)?),
        )?;
        let time_of_last_skipped_empty_proposal = IntGauge::new(
            "time_of_last_skipped_empty_proposal",
            "timestamp of the most recently skipped empty proposal",
        )?;
        let skipped_empty_proposal =
            Gauge::new("skipped_empty_proposal", "count of skipped empty proposals")?;
        registry.register(Box::new(skipped_empty_proposal.clone()))?;
        registry.register(Box::new(skipped_empty_proposal_hist.clone()))?;
        registry.register(Box::new(time_of_last_skipped_empty_proposal.clone()))?;

        Ok(Metrics {
            consensus_current_era,
            finalization_time,
            finalized_block_count,
            time_of_last_proposed_block,
            time_of_last_finalized_block,
            skipped_empty_proposal_hist,
            skipped_empty_proposal,
            time_of_last_skipped_empty_proposal,
            registry: registry.clone(),
        })
    }

    /// Updates the metrics based on a newly finalized block.
    pub(super) fn finalized_block(&mut self, finalized_block: &FinalizedBlock) {
        let time_since_block_payload = finalized_block.timestamp.elapsed().millis() as f64;
        self.finalization_time.set(time_since_block_payload);
        self.time_of_last_finalized_block
            .set(finalized_block.timestamp.millis() as i64);
        self.finalized_block_count
            .set(finalized_block.height as i64);
    }

    /// Updates the metrics and records a newly proposed block.
    pub(super) fn proposed_block(&mut self) {
        self.time_of_last_proposed_block
            .set(Timestamp::now().millis() as i64);
    }

    /// Updates the metrics and records a skipped empty proposal.
    pub(super) fn skipping_empty_proposal(&mut self, now: Timestamp, last_block_time: Timestamp) {
        let elapsed =
            Duration::from_millis(now.saturating_diff(last_block_time).millis()).as_secs_f64();
        self.skipped_empty_proposal_hist.observe(elapsed);
        self.skipped_empty_proposal.inc();
        self.time_of_last_skipped_empty_proposal
            .set(now.millis() as i64);
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.finalization_time);
        unregister_metric!(self.registry, self.finalized_block_count);
        unregister_metric!(self.registry, self.consensus_current_era);
        unregister_metric!(self.registry, self.time_of_last_finalized_block);
        unregister_metric!(self.registry, self.time_of_last_proposed_block);

        // SKIPPED EMPTY PROPOSAL METRICS
        unregister_metric!(self.registry, self.skipped_empty_proposal_hist);
        unregister_metric!(self.registry, self.skipped_empty_proposal);
        unregister_metric!(self.registry, self.time_of_last_skipped_empty_proposal);
    }
}
