use datasize::DataSize;
use serde::{Deserialize, Serialize};

use crate::types::{EVM_TRANSACTION_BATCH_SIZE_CEILING, MIN_EVM_TRANSACTION_BATCH_SIZE};

const DEFAULT_MAX_BATCH_SIZE: u32 = 10;
const DEFAULT_MAX_IN_FLIGHT_BATCH_FETCHES: u32 = 10;

/// Configuration for the EVM transaction batcher.
#[derive(Copy, Clone, DataSize, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The maximum number of transactions in an EVM transaction batch, both in batches built from
    /// this node's clients and in batches fetched from peers. A batch announced by a peer with
    /// more transactions is ignored. Must be between 2 and 100.
    pub max_batch_size: u32,
    /// The maximum number of batches announced by peers whose transactions are being fetched at
    /// the same time. A batch announced while this many are being fetched is ignored. `0` means
    /// unlimited.
    pub max_in_flight_batch_fetches: u32,
}

impl Config {
    /// The maximum number of transactions in an EVM transaction batch.
    pub fn max_batch_size(&self) -> usize {
        self.max_batch_size as usize
    }

    /// Clamps `max_batch_size` to the range the protocol supports, returning the range if it was
    /// outside of it.
    pub(crate) fn clamp_max_batch_size(&mut self) -> Option<(usize, usize)> {
        let range = (
            MIN_EVM_TRANSACTION_BATCH_SIZE,
            EVM_TRANSACTION_BATCH_SIZE_CEILING,
        );
        let clamped = self.max_batch_size().clamp(range.0, range.1);
        if clamped == self.max_batch_size() {
            return None;
        }
        self.max_batch_size = clamped as u32;
        Some(range)
    }

    /// The maximum number of batches being fetched at the same time, or `None` if unlimited.
    pub fn max_in_flight_batch_fetches(&self) -> Option<usize> {
        match self.max_in_flight_batch_fetches {
            0 => None,
            max => Some(max as usize),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            max_batch_size: DEFAULT_MAX_BATCH_SIZE,
            max_in_flight_batch_fetches: DEFAULT_MAX_IN_FLIGHT_BATCH_FETCHES,
        }
    }
}
