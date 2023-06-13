use datasize::DataSize;

use tracing::error;

use casper_types::{TimeDiff, Timestamp};

#[derive(Debug, Default, DataSize)]
pub(super) struct Latch {
    #[data_size(skip)]
    latch: u8,
    timestamp: Option<Timestamp>,
}

impl Latch {
    pub(super) fn increment(&mut self, increment_by: u8) {
        match self.latch.checked_add(increment_by) {
            Some(val) => {
                self.latch = val;
                self.touch();
            }
            None => {
                error!("latch increment overflowed.");
            }
        }
    }

    pub(super) fn decrement(&mut self, decrement_by: u8) {
        match self.latch.checked_sub(decrement_by) {
            Some(val) => {
                self.latch = val;
                self.touch();
            }
            None => {
                error!("latch decrement overflowed.");
            }
        }
    }

    pub(super) fn unlatch(&mut self) {
        self.latch = 0;
        self.timestamp = None;
    }

    pub(super) fn check_latch(&mut self, interval: TimeDiff, checked: Timestamp) -> bool {
        match self.timestamp {
            None => false,
            Some(timestamp) => {
                if checked > timestamp + interval {
                    self.unlatch()
                }
                self.count() > 0
            }
        }
    }

    pub(super) fn count(&self) -> u8 {
        self.latch
    }

    pub(super) fn touch(&mut self) {
        self.timestamp = Some(Timestamp::now());
    }
}
