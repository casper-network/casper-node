use datasize::DataSize;
use serde::Serialize;

use casper_types::{TimeDiff, Timestamp};

use crate::components::consensus::{traits::Context, utils::Weight};

/// Protocol parameters for `SimpleConsensus`.
#[derive(Debug, DataSize, Clone, Serialize)]
pub(crate) struct Params<C>
where
    C: Context,
{
    instance_id: C::InstanceId,
    min_block_time: TimeDiff,
    start_timestamp: Timestamp,
    end_height: u64,
    end_timestamp: Timestamp,
    ftt: Weight,
}

impl<C: Context> Params<C> {
    /// Creates a new set of `SimpleConsensus` protocol parameters.
    pub(crate) fn new(
        instance_id: C::InstanceId,
        min_block_time: TimeDiff,
        start_timestamp: Timestamp,
        end_height: u64,
        end_timestamp: Timestamp,
        ftt: Weight,
    ) -> Params<C> {
        Params {
            instance_id,
            min_block_time,
            start_timestamp,
            end_height,
            end_timestamp,
            ftt,
        }
    }

    /// Returns the unique identifier for this protocol instance.
    pub(crate) fn instance_id(&self) -> &C::InstanceId {
        &self.instance_id
    }

    /// Returns the minimum difference between a block's and its child's timestamp.
    pub(crate) fn min_block_time(&self) -> TimeDiff {
        self.min_block_time
    }

    /// Returns the start timestamp of the era.
    pub(crate) fn start_timestamp(&self) -> Timestamp {
        self.start_timestamp
    }

    /// Returns the minimum height of the last block.
    pub(crate) fn end_height(&self) -> u64 {
        self.end_height
    }

    /// Returns the minimum timestamp of the last block.
    pub(crate) fn end_timestamp(&self) -> Timestamp {
        self.end_timestamp
    }

    /// The threshold weight above which we are not fault tolerant any longer.
    pub(crate) fn ftt(&self) -> Weight {
        self.ftt
    }
}
